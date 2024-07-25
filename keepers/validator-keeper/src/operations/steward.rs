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

use super::keeper_operations::{check_flag, KeeperOperations};

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

pub enum StewardErrorCodes {
    ExceededRetries = 0x00,
    TransactionError = 0x10,                    // Up to 0x9F
    UnknownRpcSimulateTransactionResult = 0xA0, // Raise Flag
    ValidatorAlreadyMarkedForRemoval = 0xA1,    // Don't Raise Flag
    InvalidState = 0xA2,                        // Don't Raise Flag
    IndexesDontMatch = 0xA3,                    // Raise Flag
    VoteHistoryNotRecentEnough = 0xA4,          // Don't Raise Flag
}

pub async fn fire(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64, u64, bool) {
    let operation = _get_operation();

    let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) =
        keeper_state.copy_runs_errors_and_txs_for_epoch(operation.clone());

    let should_run = _should_run() && check_flag(keeper_config.run_flags, operation);
    let mut should_rerun = false;

    if should_run {
        match _process(keeper_config, keeper_state).await {
            Ok(stats) => {
                for message in stats.results.iter() {
                    if let Err(e) = message {
                        let error_code: i64 = match e {
                            keeper_core::SendTransactionError::ExceededRetries => {
                                StewardErrorCodes::ExceededRetries as i64
                            }
                            keeper_core::SendTransactionError::TransactionError(_) => {
                                // Just returns a string, so we can't really do anything with it
                                StewardErrorCodes::TransactionError as i64
                            }
                            keeper_core::SendTransactionError::RpcSimulateTransactionResult(_) => {
                                let error_string = format_steward_error_log(e);

                                let error_code = match error_string.as_str() {
                                    s if s.contains("Validator is already marked for removal") => {
                                        StewardErrorCodes::ValidatorAlreadyMarkedForRemoval as i64
                                    }
                                    s if s.contains("Invalid state") => {
                                        StewardErrorCodes::InvalidState as i64
                                    }
                                    s if s.contains("ListStateMismatch") => {
                                        StewardErrorCodes::IndexesDontMatch as i64
                                    }
                                    s if s.contains("VoteHistoryNotRecentEnough") => {
                                        should_rerun = true;
                                        StewardErrorCodes::VoteHistoryNotRecentEnough as i64
                                    }
                                    _ => {
                                        StewardErrorCodes::UnknownRpcSimulateTransactionResult
                                            as i64
                                    }
                                };

                                error_code
                            }
                        };

                        datapoint_error!(
                            "steward-error",
                            ("error", format_steward_error_log(e), String),
                            ("error_code", error_code, i64),
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

    (
        operation,
        runs_for_epoch,
        errors_for_epoch,
        txs_for_epoch,
        should_rerun,
    )
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
