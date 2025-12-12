//! This program starts several threads to manage the creation of validator history accounts,
//! and the updating of the various data feeds within the accounts.
//! It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.

use std::str::FromStr;

use crate::state::{keeper_config::KeeperConfig, keeper_state::KeeperState};
use log::*;
use solana_metrics::datapoint_info;
use solana_pubkey::Pubkey;
use spl_stake_pool::state::StakeStatus;
use stakenet_sdk::utils::{
    accounts::get_directed_stake_meta,
    debug::{
        format_simple_steward_state_string, format_steward_state_string,
        steward_state_to_state_code,
    },
};
use validator_history::ValidatorHistoryEntry;

use super::keeper_operations::{check_flag, KeeperOperations};

fn _get_operation() -> KeeperOperations {
    KeeperOperations::EmitMetrics
}

fn _should_run() -> bool {
    true
}

async fn _process(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
    cluster: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    emit_validator_history_metrics(keeper_state, cluster)?;
    emit_keeper_stats(keeper_state, cluster)?;
    emit_steward_stats(keeper_state, cluster)?;
    emit_directed_stake_stats(keeper_config, keeper_state).await?;
    Ok(())
}

pub async fn fire(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
    cluster: &str,
) -> (KeeperOperations, u64, u64, u64) {
    let operation = _get_operation();
    let (mut runs_for_epoch, mut errors_for_epoch, txs_for_epoch) =
        keeper_state.copy_runs_errors_and_txs_for_epoch(operation);

    let should_run = _should_run() && check_flag(keeper_config.run_flags, operation);

    if should_run {
        match _process(keeper_config, keeper_state, cluster).await {
            Ok(_) => {
                runs_for_epoch += 1;
            }
            Err(e) => {
                errors_for_epoch += 1;
                error!("Failed to emit metrics: {e}");
            }
        }
    }

    (operation, runs_for_epoch, errors_for_epoch, txs_for_epoch)
}

// ----------------- OPERATION SPECIFIC FUNCTIONS -----------------

/// Emit validator history metrics
///
/// # Overview
///
/// Every `metrics_interval` (default 60 sec), the keeper emits metrics of validator history
/// information. This function is called after the validator history map has been updated with
/// the latest validator data for the current epoch.
///
/// This function only processes validators that have corresponding entries in the
/// validator history map, effectively filtering out validators that don't meet the minimum
/// stake requirement (`validator_history_min_stake`, default: 500 SOL) since we do not create
/// [`ValidatorHistory`] accounts for such validators.
///
/// # Metrics Emitted
///
/// ## Validator Counts
/// - `num_validator_histories`: Total number of validator history accounts
/// - `num_live_validator_histories`: Number of currently active validator
/// - `num_get_vote_accounts_responses`: Vote accounts that have validator history entries
/// - `num_get_vote_accounts_voting`: Subset of above that are actively voting in current epoch
///
/// ## Validator Properties (for current epoch)
/// - `num_ips`: Validators with non-default IP addresses
/// - `num_versions`: Validators with non-default client versions
/// - `num_client_types`: Validators with non-default client types
/// - `num_mev_commissions`: Validators with non-default MEV commissions
/// - `num_commissions`: Validators with non-default commissions
/// - `num_epoch_credits`: Validators with non-default epoch credits
/// - `num_stakes`: Validators with non-default stake amounts
///
/// ## Cluster State
/// - `cluster_history_blocks`: Whether cluster history is updated for current epoch (0 or 1)
/// - `slot_index`: Current slot index from epoch info
pub fn emit_validator_history_metrics(
    keeper_state: &KeeperState,
    cluster: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let epoch_info = &keeper_state.epoch_info;
    let get_vote_accounts = keeper_state
        .vote_account_map
        .values()
        .filter(|x| {
            Pubkey::from_str(&x.vote_pubkey)
                .map(|pubkey| keeper_state.validator_history_map.contains_key(&pubkey))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let validator_histories = &keeper_state
        .validator_history_map
        .values()
        .collect::<Vec<_>>();
    let cluster_history = &keeper_state.cluster_history;

    let mut ips = 0;
    let mut versions = 0;
    let mut types = 0;
    let mut mev_comms = 0;
    let mut mev_earns = 0;
    let mut comms = 0;
    let mut epoch_credits = 0;
    let mut stakes = 0;
    let num_validators = validator_histories.len();
    let default = ValidatorHistoryEntry::default();

    let mut all_history_vote_accounts = Vec::new();
    for validator_history in validator_histories {
        // Check current epoch for state
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
        // Check previous epoch for state
        let previous_epoch = (epoch_info.epoch - 1) as u16;
        if let Some(entry) = validator_history
            .history
            .arr
            .into_iter()
            .find(|entry| entry.epoch == previous_epoch)
        {
            if entry.mev_earned != default.mev_earned {
                mev_earns += 1;
            }
        }

        all_history_vote_accounts.push(validator_history.vote_account);
    }

    let mut cluster_history_blocks: i64 = 0;
    let cluster_history_entry = cluster_history.history.last();
    if let Some(cluster_history) = cluster_history_entry {
        // Looking for current epoch to be updated, implies previous is complete as well
        if cluster_history.epoch as u64 == epoch_info.epoch {
            cluster_history_blocks = 1;
        }
    }

    let get_vote_accounts_count = get_vote_accounts.len() as i64;

    let live_validator_histories_count = keeper_state.get_live_vote_accounts().len();

    let get_vote_accounts_voting = get_vote_accounts
        .iter()
        .filter(|x| {
            x.epoch_credits
                .last()
                .map(|credit| credit.0 == epoch_info.epoch)
                .unwrap_or(false)
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
        ("num_mev_earns", mev_earns, i64),
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
        "cluster" => cluster,
    );

    Ok(())
}

pub fn emit_keeper_stats(
    keeper_state: &KeeperState,
    cluster: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let keeper_balance = keeper_state.keeper_balance;

    datapoint_info!(
        "stakenet-keeper-stats",
        ("balance_lamports", keeper_balance, i64),
        "cluster" => cluster,
    );

    Ok(())
}

pub fn emit_steward_stats(
    keeper_state: &KeeperState,
    cluster: &str,
) -> Result<(), Box<dyn std::error::Error>> {
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

    let reserve_stake = &keeper_state
        .all_steward_accounts
        .as_ref()
        .unwrap()
        .reserve_stake_account;

    let stake_pool = &keeper_state
        .all_steward_accounts
        .as_ref()
        .unwrap()
        .stake_pool_account;

    let state = steward_state.state_tag.to_string();
    let progress_count = steward_state.progress.count();
    let num_pool_validators = steward_state.num_pool_validators;
    let current_epoch = steward_state.current_epoch;
    let actual_epoch = keeper_state.epoch_info.epoch;
    let validators_to_remove_count = steward_state.validators_to_remove.count();
    let instant_unstake_count = steward_state.instant_unstake.count();
    let stake_deposit_unstake_total = steward_state.stake_deposit_unstake_total;
    let instant_unstake_total = steward_state.instant_unstake_total;
    let scoring_unstake_total = steward_state.scoring_unstake_total;
    let validators_added = steward_state.validators_added;
    let next_cycle_epoch = steward_state.next_cycle_epoch;
    let state_progress = format_steward_state_string(steward_state);
    let simple_state_progress = format_simple_steward_state_string(steward_state);
    let state_code = steward_state_to_state_code(steward_state);
    let status_flags = steward_state.status_flags;

    let validator_list_account = &keeper_state
        .all_steward_accounts
        .as_ref()
        .unwrap()
        .validator_list_account;
    let validator_list_len = validator_list_account.validators.len();

    let reserve_stake_lamports = reserve_stake.lamports;
    let stake_pool_lamports = stake_pool.total_lamports;

    let mut total_staked_lamports = 0;
    let mut total_transient_lamports = 0;
    let mut active_validators = 0;
    let mut deactivating_validators = 0;
    let mut ready_for_removal_validators = 0;
    let mut deactivating_all_validators = 0;
    let mut deactivating_transient_validators = 0;
    validator_list_account
        .clone()
        .validators
        .iter()
        .for_each(|validator| {
            total_staked_lamports += u64::from(validator.active_stake_lamports);
            total_transient_lamports += u64::from(validator.transient_stake_lamports);

            match StakeStatus::try_from(validator.status).unwrap() {
                StakeStatus::Active => {
                    active_validators += 1;
                }
                StakeStatus::DeactivatingTransient => {
                    deactivating_transient_validators += 1;
                }
                StakeStatus::ReadyForRemoval => {
                    ready_for_removal_validators += 1;
                }
                StakeStatus::DeactivatingValidator => {
                    deactivating_validators += 1;
                }
                StakeStatus::DeactivatingAll => {
                    deactivating_all_validators += 1;
                }
            }
        });

    let mut non_zero_score_count = 0;
    for i in 0..steward_state.num_pool_validators {
        if let Some(score) = steward_state.scores.get(i as usize) {
            if *score != 0 {
                non_zero_score_count += 1;
            }
        }
    }

    datapoint_info!(
        "steward-stats",
        ("state", state, String),
        ("state_progress", state_progress, String),
        ("simple_state_progress", simple_state_progress, String),
        ("state_code", state_code, i64),
        ("status_flags", status_flags, i64),
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
            "stake_deposit_unstake_total",
            stake_deposit_unstake_total,
            i64
        ),
        ("scoring_unstake_total", scoring_unstake_total, i64),
        ("instant_unstake_count", instant_unstake_count, i64),
        ("instant_unstake_total", instant_unstake_total, i64),
        ("validators_added", validators_added, i64),
        ("next_cycle_epoch", next_cycle_epoch, i64),
        ("validator_list_len", validator_list_len, i64),
        ("stake_pool_lamports", stake_pool_lamports, i64),
        ("reserve_stake_lamports", reserve_stake_lamports, i64),
        ("total_staked_lamports", total_staked_lamports, i64),
        ("total_transient_lamports", total_transient_lamports, i64),
        ("active_validators", active_validators, i64),
        ("deactivating_validators", deactivating_validators, i64),
        (
            "ready_for_removal_validators",
            ready_for_removal_validators,
            i64
        ),
        (
            "deactivating_all_validators",
            deactivating_all_validators,
            i64
        ),
        (
            "deactivating_transient_validators",
            deactivating_transient_validators,
            i64
        ),
        ("non_zero_score_count", non_zero_score_count, i64),
        "cluster" => cluster,
    );

    let parameters = &keeper_state
        .all_steward_accounts
        .as_ref()
        .unwrap()
        .config_account
        .parameters;

    let mev_commission_range = parameters.mev_commission_range;
    let epoch_credits_range = parameters.epoch_credits_range;
    let commission_range = parameters.commission_range;
    let mev_commission_bps_threshold = parameters.mev_commission_bps_threshold;
    let scoring_delinquency_threshold_ratio = parameters.scoring_delinquency_threshold_ratio;
    let instant_unstake_delinquency_threshold_ratio =
        parameters.instant_unstake_delinquency_threshold_ratio;
    let commission_threshold = parameters.commission_threshold;
    let historical_commission_threshold = parameters.historical_commission_threshold;
    let num_delegation_validators = parameters.num_delegation_validators;
    let scoring_unstake_cap_bps = parameters.scoring_unstake_cap_bps;
    let instant_unstake_cap_bps = parameters.instant_unstake_cap_bps;
    let stake_deposit_unstake_cap_bps = parameters.stake_deposit_unstake_cap_bps;
    let compute_score_slot_range = parameters.compute_score_slot_range;
    let instant_unstake_epoch_progress = parameters.instant_unstake_epoch_progress;
    let instant_unstake_inputs_epoch_progress = parameters.instant_unstake_inputs_epoch_progress;
    let num_epochs_between_scoring = parameters.num_epochs_between_scoring;
    let minimum_stake_lamports = parameters.minimum_stake_lamports;
    let minimum_voting_epochs = parameters.minimum_voting_epochs;

    datapoint_info!(
        "steward-config",
        ("mev_commission_range", mev_commission_range, i64),
        ("epoch_credits_range", epoch_credits_range, i64),
        ("commission_range", commission_range, i64),
        (
            "mev_commission_bps_threshold",
            mev_commission_bps_threshold,
            i64
        ),
        (
            "scoring_delinquency_threshold_ratio",
            scoring_delinquency_threshold_ratio,
            f64
        ),
        (
            "instant_unstake_delinquency_threshold_ratio",
            instant_unstake_delinquency_threshold_ratio,
            f64
        ),
        ("commission_threshold", commission_threshold, i64),
        (
            "historical_commission_threshold",
            historical_commission_threshold,
            i64
        ),
        ("num_delegation_validators", num_delegation_validators, i64),
        ("scoring_unstake_cap_bps", scoring_unstake_cap_bps, i64),
        ("instant_unstake_cap_bps", instant_unstake_cap_bps, i64),
        (
            "stake_deposit_unstake_cap_bps",
            stake_deposit_unstake_cap_bps,
            i64
        ),
        ("compute_score_slot_range", compute_score_slot_range, i64),
        (
            "instant_unstake_epoch_progress",
            instant_unstake_epoch_progress,
            f64
        ),
        (
            "instant_unstake_inputs_epoch_progress",
            instant_unstake_inputs_epoch_progress,
            f64
        ),
        (
            "num_epochs_between_scoring",
            num_epochs_between_scoring,
            i64
        ),
        ("minimum_stake_lamports", minimum_stake_lamports, i64),
        ("minimum_voting_epochs", minimum_voting_epochs, i64),
        "cluster" => cluster,
    );

    Ok(())
}

/// Emit directed stake metrics
pub async fn emit_directed_stake_stats(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(ref all_steward_accounts) = keeper_state.all_steward_accounts {
        let steward_state = all_steward_accounts.state_account.state;
        let meta = get_directed_stake_meta(
            keeper_config.client.clone(),
            &all_steward_accounts.config_address,
            &keeper_config.steward_config,
        )
        .await?;

        let current_epoch = steward_state.current_epoch;

        let active_targets = meta
            .targets
            .iter()
            .filter(|target| target.vote_pubkey.ne(&Pubkey::default()));
        let stake_updated_count = active_targets
            .clone()
            .filter(|target| target.staked_last_updated_epoch.eq(&current_epoch))
            .count();
        let target_updated_count = active_targets
            .filter(|target| target.target_last_updated_epoch.eq(&current_epoch))
            .count();

        datapoint_info!(
            "directed-stake-stats",
            ("state", steward_state.state_tag.to_string(), String),
            ("meta-total-stake-targets", meta.total_stake_targets, i64),
            ("stake-updated-current-epoch", stake_updated_count, i64),
            ("target-updated-current-epoch", target_updated_count, i64),
            "cluster" => keeper_config.cluster_name,
        );
    }

    Ok(())
}
