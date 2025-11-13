use std::time::Duration;

use log::*;
use rand::Rng;
use solana_metrics::datapoint_info;
use tokio::time::sleep;

use crate::{
    operations,
    state::{
        keeper_config::KeeperConfig,
        keeper_state::{KeeperFlag, KeeperState},
        operation::OperationQueue,
        update_state::{create_missing_accounts, post_create_update, pre_create_update},
    },
};

#[derive(Clone)]
pub enum KeeperCreates {
    CreateValidatorHistory,
}

impl KeeperCreates {
    pub const LEN: usize = 1;

    pub fn emit(created_accounts_for_epoch: &[u64; KeeperCreates::LEN], cluster: &str) {
        let aggregate_creates = created_accounts_for_epoch.iter().sum::<u64>();

        datapoint_info!(
            "keeper-create-stats",
            // AGGREGATE
            ("num-aggregate-creates", aggregate_creates, i64),
            // CREATE VALIDATOR HISTORY
            (
                "num-validator-history-creates",
                created_accounts_for_epoch[KeeperCreates::CreateValidatorHistory as usize],
                i64
            ),
            "cluster" => cluster,
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeeperOperations {
    PreCreateUpdate,
    CreateMissingAccounts,
    PostCreateUpdate,
    ClusterHistory,
    GossipUpload,
    StakeUpload,
    VoteAccount,
    MevEarned,
    MevCommission,
    Steward,
    EmitMetrics,
    BlockMetadataKeeper,
    PriorityFeeCommission,
}

pub fn set_flag(run_flags: u32, flag: KeeperOperations) -> u32 {
    run_flags | (0x01 << flag as u32)
}

pub fn unset_flag(run_flags: u32, flag: KeeperOperations) -> u32 {
    run_flags & !(0x01 << flag as u32)
}

pub fn check_flag(run_flags: u32, flag: KeeperOperations) -> bool {
    run_flags & (0x01 << flag as u32) == (0x01 << flag as u32)
}

impl KeeperOperations {
    pub const LEN: usize = 13;

    pub fn emit(
        runs_for_epoch: &[u64; KeeperOperations::LEN],
        errors_for_epoch: &[u64; KeeperOperations::LEN],
        txs_for_epoch: &[u64; KeeperOperations::LEN],
        cluster: &str,
    ) {
        let aggregate_actions = runs_for_epoch.iter().sum::<u64>();
        let aggregate_errors = errors_for_epoch.iter().sum::<u64>();
        let aggregate_txs = txs_for_epoch.iter().sum::<u64>();

        datapoint_info!(
            "keeper-operation-stats",
            // AGGREGATE
            ("num-aggregate-actions", aggregate_actions, i64),
            ("num-aggregate-errors", aggregate_errors, i64),
            ("num-aggregate-txs", aggregate_txs, i64),
            // PRE CREATE UPDATE
            (
                "num-pre-create-update-runs",
                runs_for_epoch[KeeperOperations::PreCreateUpdate as usize],
                i64
            ),
            (
                "num-pre-create-update-errors",
                errors_for_epoch[KeeperOperations::PreCreateUpdate as usize],
                i64
            ),
            (
                "num-pre-create-update-txs",
                txs_for_epoch[KeeperOperations::PreCreateUpdate as usize],
                i64
            ),
            // CREATE MISSING ACCOUNTS
            (
                "num-create-missing-accounts-runs",
                runs_for_epoch[KeeperOperations::CreateMissingAccounts as usize],
                i64
            ),
            (
                "num-create-missing-accounts-errors",
                errors_for_epoch[KeeperOperations::CreateMissingAccounts as usize],
                i64
            ),
            (
                "num-create-missing-accounts-txs",
                txs_for_epoch[KeeperOperations::CreateMissingAccounts as usize],
                i64
            ),
            // POST CREATE UPDATE
            (
                "num-post-create-update-runs",
                runs_for_epoch[KeeperOperations::PostCreateUpdate as usize],
                i64
            ),
            (
                "num-post-create-update-errors",
                errors_for_epoch[KeeperOperations::PostCreateUpdate as usize],
                i64
            ),
            (
                "num-post-create-update-txs",
                txs_for_epoch[KeeperOperations::PostCreateUpdate as usize],
                i64
            ),
            // CLUSTER HISTORY
            (
                "num-cluster-history-runs",
                runs_for_epoch[KeeperOperations::ClusterHistory as usize],
                i64
            ),
            (
                "num-cluster-history-errors",
                errors_for_epoch[KeeperOperations::ClusterHistory as usize],
                i64
            ),
            (
                "num-cluster-history-txs",
                txs_for_epoch[KeeperOperations::ClusterHistory as usize],
                i64
            ),
            // GOSSIP UPLOAD
            (
                "num-gossip-upload-runs",
                runs_for_epoch[KeeperOperations::GossipUpload as usize],
                i64
            ),
            (
                "num-gossip-upload-errors",
                errors_for_epoch[KeeperOperations::GossipUpload as usize],
                i64
            ),
            (
                "num-gossip-upload-txs",
                txs_for_epoch[KeeperOperations::GossipUpload as usize],
                i64
            ),
            // STAKE UPLOAD
            (
                "num-stake-upload-runs",
                runs_for_epoch[KeeperOperations::StakeUpload as usize],
                i64
            ),
            (
                "num-stake-upload-errors",
                errors_for_epoch[KeeperOperations::StakeUpload as usize],
                i64
            ),
            (
                "num-stake-upload-txs",
                txs_for_epoch[KeeperOperations::StakeUpload as usize],
                i64
            ),
            // VOTE ACCOUNT
            (
                "num-vote-account-runs",
                runs_for_epoch[KeeperOperations::VoteAccount as usize],
                i64
            ),
            (
                "num-vote-account-errors",
                errors_for_epoch[KeeperOperations::VoteAccount as usize],
                i64
            ),
            (
                "num-vote-account-txs",
                txs_for_epoch[KeeperOperations::VoteAccount as usize],
                i64
            ),
            // MEV EARNED
            (
                "num-mev-earned-runs",
                runs_for_epoch[KeeperOperations::MevEarned as usize],
                i64
            ),
            (
                "num-mev-earned-errors",
                errors_for_epoch[KeeperOperations::MevEarned as usize],
                i64
            ),
            (
                "num-mev-earned-txs",
                txs_for_epoch[KeeperOperations::MevEarned as usize],
                i64
            ),
            // MEV COMMISSION
            (
                "num-mev-commission-runs",
                runs_for_epoch[KeeperOperations::MevCommission as usize],
                i64
            ),
            (
                "num-mev-commission-errors",
                errors_for_epoch[KeeperOperations::MevCommission as usize],
                i64
            ),
            (
                "num-mev-commission-txs",
                txs_for_epoch[KeeperOperations::MevCommission as usize],
                i64
            ),
            // EMIT HISTORY
            (
                "num-emit-metrics-runs",
                runs_for_epoch[KeeperOperations::EmitMetrics as usize],
                i64
            ),
            (
                "num-emit-metrics-errors",
                errors_for_epoch[KeeperOperations::EmitMetrics as usize],
                i64
            ),
            (
                "num-emit-metrics-txs",
                txs_for_epoch[KeeperOperations::EmitMetrics as usize],
                i64
            ),
            // STEWARD
            (
                "num-steward-runs",
                runs_for_epoch[KeeperOperations::Steward as usize],
                i64
            ),
            (
                "num-steward-errors",
                errors_for_epoch[KeeperOperations::Steward as usize],
                i64
            ),
            (
                "num-steward-txs",
                txs_for_epoch[KeeperOperations::Steward as usize],
                i64
            ),
            // PRIORITY FEE COMMISSION
            (
                "num-pf-commission-runs",
                runs_for_epoch[KeeperOperations::PriorityFeeCommission as usize],
                i64
            ),
            (
                "num-pf-commission-errors",
                errors_for_epoch[KeeperOperations::PriorityFeeCommission as usize],
                i64
            ),
            (
                "num-stewpf-commissionard-txs",
                txs_for_epoch[KeeperOperations::PriorityFeeCommission as usize],
                i64
            ),
            "cluster" => cluster,
        );
    }

    pub async fn execute(
        &self,
        keeper_config: &KeeperConfig,
        keeper_state: &mut KeeperState,
        operation_queue: &mut OperationQueue,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let operation = *self;

        log::info!("Executing operation: {operation:?}");

        // Execute the operation
        match self {
            KeeperOperations::PreCreateUpdate => {
                match pre_create_update(keeper_config, keeper_state).await {
                    Ok(_) => {
                        keeper_state.increment_update_run_for_epoch(operation);
                        operation_queue.mark_completed(operation);
                    }
                    Err(e) => {
                        error!("Failed to pre create update: {:?}", e);
                        keeper_state.increment_update_error_for_epoch(operation);
                        operation_queue.mark_failed(operation);
                    }
                }
            }

            KeeperOperations::CreateMissingAccounts => {
                if keeper_config.pay_for_new_accounts {
                    match create_missing_accounts(keeper_config, keeper_state).await {
                        Ok(new_accounts_created) => {
                            keeper_state.increment_update_run_for_epoch(operation);
                            let total_txs: usize =
                                new_accounts_created.iter().map(|(_, txs)| txs).sum();
                            keeper_state
                                .increment_update_txs_for_epoch(operation, total_txs as u64);
                            new_accounts_created.iter().for_each(|(op, created)| {
                                keeper_state
                                    .increment_creations_for_epoch((op.clone(), *created as u64));
                            });
                            operation_queue.mark_completed(operation);
                        }
                        Err(e) => {
                            error!("Failed to create missing accounts: {e:?}");
                            keeper_state.increment_update_error_for_epoch(operation);
                            operation_queue.mark_failed(operation);
                        }
                    }
                } else {
                    operation_queue.mark_completed(operation);
                }
            }

            KeeperOperations::PostCreateUpdate => {
                match post_create_update(keeper_config, keeper_state).await {
                    Ok(_) => {
                        keeper_state.increment_update_run_for_epoch(operation);
                        operation_queue.mark_completed(operation);
                    }
                    Err(e) => {
                        error!("Failed to post create update: {e:?}");
                        keeper_state.increment_update_error_for_epoch(operation);
                        operation_queue.mark_failed(operation);
                    }
                }
            }

            KeeperOperations::ClusterHistory => {
                keeper_state.set_runs_errors_and_txs_for_epoch(
                    operations::cluster_history::fire(keeper_config, keeper_state).await,
                );
                operation_queue.mark_completed(operation);
            }

            KeeperOperations::VoteAccount => {
                keeper_state.set_runs_errors_txs_and_flags_for_epoch(
                    operations::vote_account::fire(keeper_config, keeper_state).await,
                );
                operation_queue.mark_completed(operation);
            }

            KeeperOperations::MevCommission => {
                keeper_state.set_runs_errors_and_txs_for_epoch(
                    operations::mev_commission::fire(keeper_config, keeper_state).await,
                );
                operation_queue.mark_completed(operation);
            }

            KeeperOperations::MevEarned => {
                keeper_state.set_runs_errors_and_txs_for_epoch(
                    operations::mev_earned::fire(keeper_config, keeper_state).await,
                );
                operation_queue.mark_completed(operation);
            }

            KeeperOperations::StakeUpload => {
                if keeper_config.oracle_authority_keypair.is_some() {
                    keeper_state.set_runs_errors_and_txs_for_epoch(
                        operations::stake_upload::fire(keeper_config, keeper_state).await,
                    );
                }
                operation_queue.mark_completed(operation);
            }

            KeeperOperations::GossipUpload => {
                if keeper_config.oracle_authority_keypair.is_some()
                    && keeper_config.gossip_entrypoints.is_some()
                {
                    keeper_state.set_runs_errors_and_txs_for_epoch(
                        operations::gossip_upload::fire(keeper_config, keeper_state).await,
                    );
                }
                operation_queue.mark_completed(operation);
            }

            KeeperOperations::PriorityFeeCommission => {
                keeper_state.set_runs_errors_and_txs_for_epoch(
                    operations::priority_fee_commission::fire(keeper_config, keeper_state).await,
                );
                operation_queue.mark_completed(operation);

                // Cooldown after validator history operations complete
                if !keeper_state.keeper_flags.check_flag(KeeperFlag::Startup) {
                    random_cooldown(keeper_config.cool_down_range).await;
                }
            }

            KeeperOperations::Steward => {
                // Check if we already fired at epoch start
                // let slot_index = keeper_state.get_slot_index_in_epoch();

                // if slot_index > 30 {
                info!("Cranking Steward (normal interval)...");
                keeper_state.set_runs_errors_txs_and_flags_for_epoch(
                    operations::steward::fire(keeper_config, keeper_state).await,
                );

                if !keeper_state.keeper_flags.check_flag(KeeperFlag::Startup) {
                    random_cooldown(keeper_config.cool_down_range).await;
                }
                // } else {
                //     info!(
                //         "Skipping Steward - already fired at epoch start (slot {})",
                //         slot_index
                //     );
                // }
                operation_queue.mark_completed(operation);
            }

            KeeperOperations::BlockMetadataKeeper => {
                if keeper_config
                    .priority_fee_oracle_authority_keypair
                    .is_some()
                {
                    keeper_state.set_runs_errors_and_txs_for_epoch(
                        operations::block_metadata::operations::fire(keeper_config, keeper_state)
                            .await,
                    );
                }
                operation_queue.mark_completed(operation);
            }

            KeeperOperations::EmitMetrics => {
                keeper_state.set_runs_errors_and_txs_for_epoch(
                    operations::metrics_emit::fire(
                        keeper_config,
                        keeper_state,
                        keeper_config.cluster_name.as_str(),
                    )
                    .await,
                );

                keeper_state.emit();

                KeeperOperations::emit(
                    &keeper_state.runs_for_epoch,
                    &keeper_state.errors_for_epoch,
                    &keeper_state.txs_for_epoch,
                    keeper_config.cluster_name.as_str(),
                );

                KeeperCreates::emit(
                    &keeper_state.created_accounts_for_epoch,
                    &keeper_state.cluster_name,
                );

                operation_queue.mark_completed(operation);
            }
        }

        Ok(())
    }
}

/// To reduce transaction collisions, we sleep a random amount after any emit
async fn random_cooldown(range: u8) {
    let mut rng = rand::thread_rng();
    let sleep_duration = rng.gen_range(0..=60 * (range as u64 + 1));

    info!("\n\n⏰ Cooldown for {} seconds\n", sleep_duration);
    sleep(Duration::from_secs(sleep_duration)).await;
}
