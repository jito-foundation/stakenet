/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/

use crate::state::keeper_state::{KeeperFlag, KeeperFlags, KeeperState};
use crate::{
    entries::copy_vote_account_entry::CopyVoteAccountEntry, state::keeper_config::KeeperConfig,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::datapoint_error;
use solana_sdk::{
    epoch_info::EpochInfo,
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
    KeeperOperations::VoteAccount
}

fn _should_run(epoch_info: &EpochInfo, runs_for_epoch: u64) -> bool {
    // Run at 0.1%, 50% and 90% completion of epoch
    (epoch_info.slot_index > epoch_info.slots_in_epoch / 1000 && runs_for_epoch < 1)
        || (epoch_info.slot_index > epoch_info.slots_in_epoch * 5 / 10 && runs_for_epoch < 2)
        || (epoch_info.slot_index > epoch_info.slots_in_epoch * 9 / 10 && runs_for_epoch < 3)
}

async fn _process(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    priority_fee_in_microlamports: u64,
    retry_count: u16,
    confirmation_time: u64,
    keeper_state: &KeeperState,
    no_pack: bool,
) -> Result<SubmitStats, JitoTransactionError> {
    update_vote_accounts(
        client,
        keypair,
        program_id,
        priority_fee_in_microlamports,
        retry_count,
        confirmation_time,
        keeper_state,
        no_pack,
    )
    .await
}

pub async fn fire(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64, u64, KeeperFlags) {
    let client = &keeper_config.client;
    let keypair = &keeper_config.keypair;
    let program_id = &keeper_config.validator_history_program_id;
    let priority_fee_in_microlamports = keeper_config.priority_fee_in_microlamports;
    let retry_count = keeper_config.tx_retry_count;
    let confirmation_time = keeper_config.tx_confirmation_seconds;

    let operation = _get_operation();
    let epoch_info = &keeper_state.epoch_info;
    let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) =
        keeper_state.copy_runs_errors_and_txs_for_epoch(operation);

    let should_run = (_should_run(epoch_info, runs_for_epoch)
        || keeper_state.keeper_flags.check_flag(KeeperFlag::RerunVote))
        && check_flag(keeper_config.run_flags, operation);

    let mut keeper_flags = keeper_state.keeper_flags;
    keeper_flags.unset_flag(KeeperFlag::RerunVote);

    if should_run {
        match _process(
            client,
            keypair,
            program_id,
            priority_fee_in_microlamports,
            retry_count,
            confirmation_time,
            keeper_state,
            keeper_config.no_pack,
        )
        .await
        {
            Ok(stats) => {
                for message in stats.results.iter().chain(stats.results.iter()) {
                    if let Err(e) = message {
                        datapoint_error!("vote-account-error", ("error", e.to_string(), String),);
                    } else {
                        txs_for_epoch += 1;
                    }
                }
                if stats.errors == 0 {
                    runs_for_epoch += 1;
                }
            }
            Err(e) => {
                datapoint_error!("vote-account-error", ("error", e.to_string(), String),);
                errors_for_epoch += 1;
            }
        };
    }

    (
        operation,
        runs_for_epoch,
        errors_for_epoch,
        txs_for_epoch,
        keeper_flags,
    )
}

// SPECIFIC TO THIS OPERATION
pub async fn update_vote_accounts(
    rpc_client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    priority_fee_in_microlamports: u64,
    retry_count: u16,
    confirmation_time: u64,
    keeper_state: &KeeperState,
    no_pack: bool,
) -> Result<SubmitStats, JitoTransactionError> {
    let validator_history_map = &keeper_state.validator_history_map;
    let epoch_info = &keeper_state.epoch_info;

    // Update all open vote accounts, less the ones that have been recently updated
    let mut vote_accounts_to_update = keeper_state.get_all_open_vote_accounts();
    if !keeper_state.keeper_flags.check_flag(KeeperFlag::Startup)
        && !keeper_state.keeper_flags.check_flag(KeeperFlag::RerunVote)
    {
        vote_accounts_to_update.retain(|vote_account| {
            !vote_account_uploaded_recently(
                validator_history_map,
                vote_account,
                epoch_info.epoch,
                epoch_info.absolute_slot,
            )
        });
    }

    let entries = vote_accounts_to_update
        .iter()
        .map(|vote_account| CopyVoteAccountEntry::new(vote_account, program_id, &keypair.pubkey()))
        .collect::<Vec<_>>();

    let update_instructions = entries
        .iter()
        .map(|copy_vote_account_entry| copy_vote_account_entry.update_instruction())
        .collect::<Vec<_>>();

    let submit_result = submit_instructions(
        rpc_client,
        update_instructions,
        keypair,
        priority_fee_in_microlamports,
        retry_count,
        confirmation_time,
        Some(300_000),
        no_pack,
    )
    .await;

    submit_result.map_err(|e| e.into())
}

fn vote_account_uploaded_recently(
    validator_history_map: &HashMap<Pubkey, ValidatorHistory>,
    vote_account: &Pubkey,
    epoch: u64,
    slot: u64,
) -> bool {
    if let Some(validator_history) = validator_history_map.get(vote_account) {
        if let Some(entry) = validator_history.history.last() {
            if entry.epoch == epoch as u16
                && entry.vote_account_last_update_slot
                    != ValidatorHistoryEntry::default().vote_account_last_update_slot
                && entry.vote_account_last_update_slot > slot - 50000
            {
                return true;
            }
        }
    }
    false
}
