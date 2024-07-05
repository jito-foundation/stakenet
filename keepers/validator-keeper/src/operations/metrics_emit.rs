/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/
use crate::state::keeper_state::{self, KeeperState};
use log::*;
use solana_metrics::datapoint_info;
use validator_history::ValidatorHistoryEntry;

use super::keeper_operations::KeeperOperations;

fn _get_operation() -> KeeperOperations {
    KeeperOperations::EmitHistory
}

fn _should_run() -> bool {
    true
}

fn _process(keeper_state: &KeeperState) -> Result<(), Box<dyn std::error::Error>> {
    emit_validator_history_metrics(keeper_state)?;
    emit_keeper_stats(keeper_state)?;
    emit_steward_stats(keeper_state)?;
    Ok(())
}

pub fn fire(keeper_state: &KeeperState) -> (KeeperOperations, u64, u64, u64) {
    let operation = _get_operation();
    let (mut runs_for_epoch, mut errors_for_epoch, txs_for_epoch) =
        keeper_state.copy_runs_errors_and_txs_for_epoch(operation.clone());

    let should_run = _should_run();

    if should_run {
        match _process(keeper_state) {
            Ok(_) => {
                runs_for_epoch += 1;
            }
            Err(e) => {
                errors_for_epoch += 1;
                error!("Failed to emit metrics: {}", e);
            }
        }
    }

    (operation, runs_for_epoch, errors_for_epoch, txs_for_epoch)
}

// ----------------- OPERATION SPECIFIC FUNCTIONS -----------------
pub fn emit_validator_history_metrics(
    keeper_state: &KeeperState,
) -> Result<(), Box<dyn std::error::Error>> {
    let epoch_info = &keeper_state.epoch_info;
    let get_vote_accounts = keeper_state.vote_account_map.values().collect::<Vec<_>>();
    let validator_histories = &keeper_state
        .validator_history_map
        .values()
        .collect::<Vec<_>>();
    let cluster_history = &keeper_state.cluster_history;

    let mut ips = 0;
    let mut versions = 0;
    let mut types = 0;
    let mut mev_comms = 0;
    let mut comms = 0;
    let mut epoch_credits = 0;
    let mut stakes = 0;
    let num_validators = validator_histories.len();
    let default = ValidatorHistoryEntry::default();

    let mut all_history_vote_accounts = Vec::new();
    for validator_history in validator_histories {
        if let Some(entry) = validator_history.history.last() {
            if entry.epoch as u64 != epoch_info.epoch {
                continue;
            }
            if entry.ip != default.ip {
                ips += 1;
            }
            if !(entry.version.major == default.version.major
                && entry.version.minor == default.version.minor
                && entry.version.patch == default.version.patch)
            {
                versions += 1;
            }
            if entry.client_type != default.client_type {
                types += 1;
            }
            if entry.mev_commission != default.mev_commission {
                mev_comms += 1;
            }
            if entry.commission != default.commission {
                comms += 1;
            }
            if entry.epoch_credits != default.epoch_credits {
                epoch_credits += 1;
            }
            if entry.activated_stake_lamports != default.activated_stake_lamports {
                stakes += 1;
            }
        }

        all_history_vote_accounts.push(validator_history.vote_account);
    }

    let mut cluster_history_blocks: i64 = 0;
    let cluster_history_entry = cluster_history.history.last();
    if let Some(cluster_history) = cluster_history_entry {
        // Looking for previous epoch to be updated
        if cluster_history.epoch as u64 == epoch_info.epoch - 1 {
            cluster_history_blocks = 1;
        }
    }

    let get_vote_accounts_count = get_vote_accounts.len() as i64;

    let live_validator_histories_count = keeper_state.get_live_vote_accounts().len();

    let get_vote_accounts_voting = get_vote_accounts
        .iter()
        .filter(|x| {
            // Check if the last epoch credit ( most recent ) is the current epoch
            x.epoch_credits.last().unwrap().0 == epoch_info.epoch
        })
        .count();

    datapoint_info!(
        "validator-history-stats",
        ("num_validator_histories", num_validators, i64),
        (
            "num_live_validator_histories",
            live_validator_histories_count,
            i64
        ),
        ("num_ips", ips, i64),
        ("num_versions", versions, i64),
        ("num_client_types", types, i64),
        ("num_mev_commissions", mev_comms, i64),
        ("num_commissions", comms, i64),
        ("num_epoch_credits", epoch_credits, i64),
        ("num_stakes", stakes, i64),
        ("cluster_history_blocks", cluster_history_blocks, i64),
        ("slot_index", epoch_info.slot_index, i64),
        (
            "num_get_vote_accounts_responses",
            get_vote_accounts_count,
            i64
        ),
        (
            "num_get_vote_accounts_voting",
            get_vote_accounts_voting,
            i64
        ),
    );

    Ok(())
}

pub fn emit_keeper_stats(keeper_state: &KeeperState) -> Result<(), Box<dyn std::error::Error>> {
    let keeper_balance = keeper_state.keeper_balance;

    datapoint_info!(
        "stakenet-keeper-stats",
        ("balance_lamports", keeper_balance, i64),
    );

    Ok(())
}

pub fn emit_steward_stats(keeper_state: &KeeperState) -> Result<(), Box<dyn std::error::Error>> {
    //    - Progress
    // - Current State
    // - Num pool validators
    // - Validator List length
    // - Validators added
    //     - num_pool_validators ≠ validator list length
    // - Validators removed
    //     - Check ValidatorList Deactivating* state
    //     - Marked to remove
    // - Total activating stake
    // - Total deactivating stake

    if keeper_state.all_steward_accounts.is_none() {
        return Ok(());
    }

    let steward_state = &keeper_state
        .all_steward_accounts
        .as_ref()
        .unwrap()
        .state_account
        .state;
    let state = steward_state.state_tag.to_string();
    let progress_count = steward_state.progress.count();
    let num_pool_validators = steward_state.num_pool_validators;
    let current_epoch = steward_state.current_epoch;
    let actual_epoch = keeper_state.epoch_info.epoch;
    let validators_to_remove_count = steward_state.validators_to_remove.count();
    let checked_validators_to_remove_flag =
        steward_state.checked_validators_removed_from_list.value;
    let compute_delegations_complete_flag = steward_state.compute_delegations_completed.value;
    let instant_unstake_count = steward_state.instant_unstake.count();
    let instant_unstake_total = steward_state.instant_unstake_total;
    let validators_added = steward_state.validators_added;
    let next_cycle_epoch = steward_state.next_cycle_epoch;

    let validator_list_account = &keeper_state
        .all_steward_accounts
        .as_ref()
        .unwrap()
        .validator_list_account;
    let validator_list_len = validator_list_account.validators.len();

    datapoint_info!(
        "steward-stats",
        ("state", state, String),
        ("progress_count", progress_count, i64),
        ("num_pool_validators", num_pool_validators, i64),
        ("current_epoch", current_epoch, i64),
        ("actual_epoch", actual_epoch, i64),
        (
            "validators_to_remove_count",
            validators_to_remove_count,
            i64
        ),
        (
            "checked_validators_to_remove_flag",
            checked_validators_to_remove_flag,
            i64
        ),
        (
            "compute_delegations_complete_flag",
            compute_delegations_complete_flag,
            i64
        ),
        ("instant_unstake_count", instant_unstake_count, i64),
        ("instant_unstake_total", instant_unstake_total, i64),
        ("validators_added", validators_added, i64),
        ("next_cycle_epoch", next_cycle_epoch, i64),
        ("validator_list_len", validator_list_len, i64),
    );

    Ok(())
}
