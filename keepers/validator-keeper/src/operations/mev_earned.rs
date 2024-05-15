/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/

use crate::state::keeper_state::KeeperState;
use crate::{
    derive_validator_history_address, derive_validator_history_config_address, KeeperError,
    PRIORITY_FEE,
};
use anchor_lang::AccountDeserialize;
use anchor_lang::{InstructionData, ToAccountMetas};
use jito_tip_distribution::sdk::derive_tip_distribution_account_address;
use jito_tip_distribution::state::TipDistributionAccount;
use keeper_core::{submit_instructions, SubmitStats, TransactionExecutionError};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::{datapoint_error, datapoint_info};
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::{collections::HashMap, sync::Arc};
use validator_history::ValidatorHistory;
use validator_history::ValidatorHistoryEntry;

use super::keeper_operations::KeeperOperations;

fn _get_operation() -> KeeperOperations {
    return KeeperOperations::MevEarned;
}

fn _should_run() -> bool {
    true
}

async fn _process(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    tip_distribution_program_id: &Pubkey,
    keeper_state: &KeeperState,
) -> Result<SubmitStats, KeeperError> {
    update_mev_earned(
        client,
        keypair,
        program_id,
        tip_distribution_program_id,
        keeper_state,
    )
    .await
}

fn _emit(stats: &SubmitStats, runs_for_epoch: i64, errors_for_epoch: i64) {
    datapoint_info!(
        "mev-earned-stats",
        ("num_updates_success", stats.successes, i64),
        ("num_updates_error", stats.errors, i64),
        ("runs_for_epoch", runs_for_epoch, i64),
        ("errors_for_epoch", errors_for_epoch, i64),
    );
}

pub async fn fire_and_emit(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    tip_distribution_program_id: &Pubkey,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64) {
    let operation = _get_operation();

    let (mut runs_for_epoch, mut errors_for_epoch) =
        keeper_state.copy_runs_and_errors_for_epoch(operation.clone());

    let stats = match _process(
        client,
        keypair,
        program_id,
        tip_distribution_program_id,
        keeper_state,
    )
    .await
    {
        Ok(stats) => {
            for message in stats.results.iter().chain(stats.results.iter()) {
                if let Err(e) = message {
                    datapoint_error!("vote-account-error", ("error", e.to_string(), String),);
                    errors_for_epoch += 1;
                }
            }

            runs_for_epoch += 1;
            stats
        }
        Err(e) => {
            let mut stats = SubmitStats::default();
            if let KeeperError::TransactionExecutionError(
                TransactionExecutionError::TransactionClientError(_, results),
            ) = &e
            {
                stats.successes = results.iter().filter(|r| r.is_ok()).count() as u64;
                stats.errors = results.iter().filter(|r| r.is_err()).count() as u64;
            }
            datapoint_error!("mev-earned-error", ("error", e.to_string(), String),);
            errors_for_epoch += 1;
            stats
        }
    };

    _emit(&stats, runs_for_epoch as i64, errors_for_epoch as i64);

    (operation, runs_for_epoch, errors_for_epoch)
}

// ----------------- OPERATION SPECIFIC FUNCTIONS -----------------

fn create_update_instruction(
    program_id: &Pubkey,
    keypair: &Pubkey,
    vote_account: &Pubkey,
    tip_distribution_program_id: &Pubkey,
    epoch: u64,
) -> Instruction {
    let validator_history_account = derive_validator_history_address(program_id, vote_account);
    let (tip_distribution_account, _) =
        derive_tip_distribution_account_address(tip_distribution_program_id, vote_account, epoch);

    let config = derive_validator_history_config_address(program_id);

    Instruction {
        program_id: program_id.clone(),
        accounts: validator_history::accounts::CopyTipDistributionAccount {
            validator_history_account: validator_history_account,
            vote_account: vote_account.clone(),
            tip_distribution_account: tip_distribution_account,
            config: config,
            signer: keypair.clone(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::CopyTipDistributionAccount { epoch: epoch }.data(),
    }
}

pub async fn update_mev_earned(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    tip_distribution_program_id: &Pubkey,
    keeper_state: &KeeperState,
) -> Result<SubmitStats, KeeperError> {
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
                Some(address.clone())
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
            create_update_instruction(
                program_id,
                &keypair.pubkey(),
                vote_account,
                tip_distribution_program_id,
                epoch_info.epoch.saturating_sub(1),
            )
        })
        .collect::<Vec<_>>();

    let submit_result =
        submit_instructions(client, update_instructions, keypair, PRIORITY_FEE).await;

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
