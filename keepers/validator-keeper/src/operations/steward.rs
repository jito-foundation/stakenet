/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/

use crate::state::keeper_config::KeeperConfig;
use crate::state::keeper_state::KeeperState;
use keeper_core::SubmitStats;
use solana_metrics::datapoint_error;
use steward_cli::{
    commands::monkey::crank::{crank_monkey, MonkeyCrankError},
    utils::transactions::format_steward_error_log,
};

use super::keeper_operations::KeeperOperations;

fn _get_operation() -> KeeperOperations {
    KeeperOperations::Steward
}

fn _should_run() -> bool {
    true
}

async fn _process(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
) -> Result<SubmitStats, MonkeyCrankError> {
    run_crank_monkey(keeper_config, keeper_state).await
}

pub async fn fire(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64, u64) {
    let operation = _get_operation();

    let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) =
        keeper_state.copy_runs_errors_and_txs_for_epoch(operation.clone());

    let should_run = _should_run();

    if should_run {
        match _process(keeper_config, keeper_state).await {
            Ok(stats) => {
                for message in stats.results.iter() {
                    if let Err(e) = message {
                        datapoint_error!(
                            "steward-error",
                            ("error", format_steward_error_log(e), String),
                        );
                    } else {
                        txs_for_epoch += 1;
                    }
                }

                if stats.errors == 0 {
                    runs_for_epoch += 1;
                }
            }
            Err(e) => {
                datapoint_error!("steward-error", ("error", e.to_string(), String),);
                errors_for_epoch += 1;
            }
        };
    }

    (operation, runs_for_epoch, errors_for_epoch, txs_for_epoch)
}

// ----------------- OPERATION SPECIFIC FUNCTIONS -----------------

pub async fn run_crank_monkey(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
) -> Result<SubmitStats, MonkeyCrankError> {
    crank_monkey(
        &keeper_config.client,
        &keeper_config.keypair,
        &keeper_config.steward_program_id,
        keeper_state.epoch_info.epoch,
        keeper_state.all_steward_accounts.as_ref().unwrap(),
        keeper_state
            .all_steward_validator_accounts
            .as_ref()
            .unwrap(),
        keeper_state.all_active_validator_accounts.as_ref().unwrap(),
        Some(keeper_config.priority_fee_in_microlamports),
    )
    .await
}
