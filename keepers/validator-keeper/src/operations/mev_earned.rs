/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/

use crate::state::keeper_state::KeeperState;
use crate::{
    entries::mev_commission_entry::ValidatorMevCommissionEntry, state::keeper_config::KeeperConfig,
};
use anchor_lang::AccountDeserialize;
use jito_tip_distribution::state::TipDistributionAccount;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::datapoint_error;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use stakenet_sdk::models::entries::UpdateInstruction;
use stakenet_sdk::models::errors::JitoTransactionError;
use stakenet_sdk::models::submit_stats::SubmitStats;
use stakenet_sdk::utils::transactions::submit_instructions;
use std::{collections::HashMap, sync::Arc};
use validator_history::ValidatorHistory;
use validator_history::ValidatorHistoryEntry;

use super::keeper_operations::{check_flag, KeeperOperations};

fn _get_operation() -> KeeperOperations {
    KeeperOperations::MevEarned
}

fn _should_run() -> bool {
    true
}

#[allow(clippy::too_many_arguments)]
async fn _process(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    tip_distribution_program_id: &Pubkey,
    priority_fee_in_microlamports: u64,
    retry_count: u16,
    confirmation_time: u64,
    keeper_state: &KeeperState,
    no_pack: bool,
) -> Result<SubmitStats, JitoTransactionError> {
    update_mev_earned(
        client,
        keypair,
        program_id,
        priority_fee_in_microlamports,
        retry_count,
        confirmation_time,
        tip_distribution_program_id,
        keeper_state,
        no_pack,
    )
    .await
}

pub async fn fire(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64, u64) {
    let client = &keeper_config.client;
    let keypair = &keeper_config.keypair;
    let program_id = &keeper_config.validator_history_program_id;
    let tip_distribution_program_id = &keeper_config.tip_distribution_program_id;
    let priority_fee_in_microlamports = keeper_config.priority_fee_in_microlamports;
    let operation = _get_operation();

    let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) =
        keeper_state.copy_runs_errors_and_txs_for_epoch(operation);

    let should_run = _should_run() && check_flag(keeper_config.run_flags, operation);

    if should_run {
        match _process(
            client,
            keypair,
            program_id,
            tip_distribution_program_id,
            priority_fee_in_microlamports,
            keeper_config.tx_retry_count,
            keeper_config.tx_confirmation_seconds,
            keeper_state,
            keeper_config.no_pack,
        )
        .await
        {
            Ok(stats) => {
                for message in stats.results.iter().chain(stats.results.iter()) {
                    if let Err(e) = message {
                        datapoint_error!("vote-account-error", ("error", e.to_string(), String),);
                        errors_for_epoch += 1;
                    } else {
                        txs_for_epoch += 1;
                    }
                }
                if stats.errors == 0 {
                    runs_for_epoch += 1;
                }
            }
            Err(e) => {
                datapoint_error!("mev-earned-error", ("error", e.to_string(), String),);
                errors_for_epoch += 1;
            }
        };
    }

    (operation, runs_for_epoch, errors_for_epoch, txs_for_epoch)
}

// ----------------- OPERATION SPECIFIC FUNCTIONS -----------------

#[allow(clippy::too_many_arguments)]
pub async fn update_mev_earned(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    priority_fee_in_microlamports: u64,
    retry_count: u16,
    confirmation_time: u64,
    tip_distribution_program_id: &Pubkey,
    keeper_state: &KeeperState,
    no_pack: bool,
) -> Result<SubmitStats, JitoTransactionError> {
    let epoch_info = &keeper_state.epoch_info;
    let validator_history_map = &keeper_state.validator_history_map;
    let previous_epoch_tip_distribution_map = &keeper_state.previous_epoch_tip_distribution_map;

    let uploaded_merkleroot_entries = previous_epoch_tip_distribution_map
        .iter()
        .filter_map(|(address, account)| {
            let account_data = account.as_ref()?;
            let mut data: &[u8] = &account_data.data;
            let tda = TipDistributionAccount::try_deserialize(&mut data).ok()?;
            if tda.merkle_root.is_some() {
                Some(*address)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let entries_to_update = uploaded_merkleroot_entries
        .into_iter()
        .filter(|entry| {
            !mev_earned_uploaded(
                validator_history_map,
                entry,
                epoch_info.epoch.saturating_sub(1),
            )
        })
        .collect::<Vec<_>>();

    let update_instructions = entries_to_update
        .iter()
        .map(|vote_account| {
            ValidatorMevCommissionEntry::new(
                vote_account,
                epoch_info.epoch.saturating_sub(1),
                program_id,
                tip_distribution_program_id,
                &keypair.pubkey(),
            )
            .update_instruction()
        })
        .collect::<Vec<_>>();

    let submit_result = submit_instructions(
        client,
        update_instructions,
        keypair,
        priority_fee_in_microlamports,
        retry_count,
        confirmation_time,
        None,
        no_pack,
    )
    .await;

    submit_result.map_err(|e| e.into())
}

fn mev_earned_uploaded(
    validator_history_map: &HashMap<Pubkey, ValidatorHistory>,
    vote_account: &Pubkey,
    epoch: u64,
) -> bool {
    if let Some(validator_history) = validator_history_map.get(vote_account) {
        if let Some(latest_entry) = validator_history
            .history
            .epoch_range(epoch as u16, epoch as u16)[0]
        {
            return latest_entry.epoch == epoch as u16
                && latest_entry.mev_earned != ValidatorHistoryEntry::default().mev_earned;
        }
    };
    false
}
