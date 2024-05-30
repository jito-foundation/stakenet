/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/

use crate::entries::copy_vote_account_entry::CopyVoteAccountEntry;
use crate::state::keeper_state::KeeperState;
use crate::{KeeperError, PRIORITY_FEE};
use keeper_core::{submit_instructions, SubmitStats, UpdateInstruction};
use log::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::datapoint_error;
use solana_sdk::{
    epoch_info::EpochInfo,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::{collections::HashMap, sync::Arc};
use validator_history::ValidatorHistory;
use validator_history::ValidatorHistoryEntry;

use super::keeper_operations::KeeperOperations;

fn _get_operation() -> KeeperOperations {
    KeeperOperations::VoteAccount
}

fn _should_run(epoch_info: &EpochInfo, runs_for_epoch: u64) -> bool {
    // Run at 10%, 50% and 90% completion of epoch
    (epoch_info.slot_index > epoch_info.slots_in_epoch / 1000 && runs_for_epoch < 1)
        || (epoch_info.slot_index > epoch_info.slots_in_epoch / 2 && runs_for_epoch < 2)
        || (epoch_info.slot_index > epoch_info.slots_in_epoch * 9 / 10 && runs_for_epoch < 3)
}

async fn _process(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    keeper_state: &KeeperState,
) -> Result<SubmitStats, KeeperError> {
    update_vote_accounts(client, keypair, program_id, keeper_state).await
}

pub async fn fire(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64) {
    let operation = _get_operation();
    let epoch_info = &keeper_state.epoch_info;
    let (mut runs_for_epoch, mut errors_for_epoch) =
        keeper_state.copy_runs_and_errors_for_epoch(operation.clone());

    let should_run = _should_run(epoch_info, runs_for_epoch);

    if should_run {
        match _process(client, keypair, program_id, keeper_state).await {
            Ok(stats) => {
                for message in stats.results.iter().chain(stats.results.iter()) {
                    if let Err(e) = message {
                        datapoint_error!("vote-account-error", ("error", e.to_string(), String),);
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

    info!(
        "Vote account, runs: {}, errors: {}",
        runs_for_epoch, errors_for_epoch
    );

    (operation, runs_for_epoch, errors_for_epoch)
}

// SPECIFIC TO THIS OPERATION
pub async fn update_vote_accounts(
    rpc_client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    keeper_state: &KeeperState,
) -> Result<SubmitStats, KeeperError> {
    let validator_history_map = &keeper_state.validator_history_map;
    let closed_vote_accounts = &keeper_state.get_closed_vote_accounts();
    let epoch_info = &keeper_state.epoch_info;

    // Remove closed vote accounts from all vote accounts
    // Remove vote accounts for which this instruction has been called within 50,000 slots
    let mut vote_accounts_to_update = keeper_state.vote_account_map.keys().collect::<Vec<_>>();

    vote_accounts_to_update.retain(|vote_account| {
        !closed_vote_accounts.contains(vote_account)
            && !vote_account_uploaded_recently(
                validator_history_map,
                vote_account,
                epoch_info.epoch,
                epoch_info.absolute_slot,
            )
    });

    let entries = vote_accounts_to_update
        .iter()
        .map(|vote_account| CopyVoteAccountEntry::new(vote_account, program_id, &keypair.pubkey()))
        .collect::<Vec<_>>();

    let update_instructions = entries
        .iter()
        .map(|copy_vote_account_entry| copy_vote_account_entry.update_instruction())
        .collect::<Vec<_>>();

    info!(
        "Running vote account update for {} vote accounts",
        update_instructions.len()
    );

    let submit_result =
        submit_instructions(rpc_client, update_instructions, keypair, PRIORITY_FEE, None).await;

    info!("Vote account update result: {:?}", submit_result);

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
