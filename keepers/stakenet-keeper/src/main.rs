/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/
use clap::Parser;
use dotenvy::dotenv;
use log::*;
use rusqlite::Connection;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::set_host_id;
use solana_sdk::signature::read_keypair_file;
use stakenet_keeper::{
    operations::{
        self,
        block_metadata::db::create_sqlite_tables,
        keeper_operations::{set_flag, KeeperOperations},
    },
    state::{
        keeper_config::{Args, KeeperConfig},
        keeper_state::{KeeperFlag, KeeperState},
        operation::{OperationQueue, OperationState},
    },
};
use std::{process::Command, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio::time::sleep;

fn set_run_flags(args: &Args) -> u32 {
    let mut run_flags = 0;

    run_flags = set_flag(run_flags, KeeperOperations::PreCreateUpdate);
    run_flags = set_flag(run_flags, KeeperOperations::CreateMissingAccounts);
    run_flags = set_flag(run_flags, KeeperOperations::PostCreateUpdate);

    if args.run_cluster_history {
        run_flags = set_flag(run_flags, KeeperOperations::ClusterHistory);
    }
    if args.run_copy_vote_accounts {
        run_flags = set_flag(run_flags, KeeperOperations::VoteAccount);
    }
    if args.run_mev_commission {
        run_flags = set_flag(run_flags, KeeperOperations::MevCommission);
    }
    if args.run_mev_earned {
        run_flags = set_flag(run_flags, KeeperOperations::MevEarned);
    }
    if args.run_stake_upload {
        run_flags = set_flag(run_flags, KeeperOperations::StakeUpload);
    }
    if args.run_gossip_upload {
        run_flags = set_flag(run_flags, KeeperOperations::GossipUpload);
    }
    if args.run_steward {
        run_flags = set_flag(run_flags, KeeperOperations::Steward);
    }
    if args.run_emit_metrics {
        run_flags = set_flag(run_flags, KeeperOperations::EmitMetrics);
    }
    if args.run_block_metadata {
        run_flags = set_flag(run_flags, KeeperOperations::BlockMetadataKeeper);
    }
    if args.run_priority_fee_commission {
        run_flags = set_flag(run_flags, KeeperOperations::PriorityFeeCommission);
    }

    run_flags
}

fn should_clear_startup_flag(tick: u64, intervals: &[u64]) -> bool {
    let max_interval = intervals.iter().max().unwrap();
    tick % (max_interval + 1) == 0
}

fn advance_tick(tick: &mut u64) {
    *tick += 1;
}

async fn sleep_and_tick(tick: &mut u64) {
    sleep(Duration::from_secs(1)).await;
    advance_tick(tick);
}

async fn run_keeper(keeper_config: KeeperConfig) {
    // Intervals
    let metrics_interval = keeper_config.metrics_interval;
    let validator_history_interval = keeper_config.validator_history_interval;
    let steward_interval = keeper_config.steward_interval;
    let block_metadata_interval = keeper_config.block_metadata_interval;

    let intervals = vec![
        validator_history_interval,
        metrics_interval,
        steward_interval,
        block_metadata_interval,
    ];

    // Stateful data
    let mut keeper_state = KeeperState::default();
    keeper_state.set_cluster_name(&keeper_config.cluster_name);

    let mut operation_queue = OperationQueue::new(
        keeper_config.validator_history_interval,
        keeper_config.steward_interval,
        keeper_config.block_metadata_interval,
        keeper_config.metrics_interval,
        keeper_config.run_flags,
    );
    let mut last_seen_epoch = keeper_config
        .client
        .get_epoch_info()
        .await
        .map(|epoch_info| Some(epoch_info.epoch))
        .unwrap_or(None);

    info!(
        "Operations: {}",
        operation_queue
            .tasks
            .iter()
            .map(|o| o.operation.to_string())
            .collect::<Vec<String>>()
            .join(",")
    );

    let smallest_interval = intervals.iter().min().unwrap();
    let mut tick: u64 = *smallest_interval; // 1 second ticks - start at metrics interval

    if keeper_config.full_startup {
        keeper_state.keeper_flags.set_flag(KeeperFlag::Startup);
    }

    loop {
        if let Some(seen_epoch) = last_seen_epoch {
            match keeper_config.client.get_epoch_info().await {
                Ok(epoch_info) => {
                    let current_epoch = epoch_info.epoch;
                    keeper_state.epoch_info = epoch_info;

                    if current_epoch > seen_epoch {
                        info!(
                        "EPOCH TRANSITION! {seen_epoch} -> {current_epoch} - IMMEDIATE STEWARD!",
                    );
                        last_seen_epoch = Some(current_epoch);

                        // Fire Steward immediately
                        keeper_state.set_runs_errors_txs_and_flags_for_epoch(
                            operations::steward::fire(&keeper_config, &keeper_state).await,
                        );

                        info!("Epoch start Steward crank completed");
                    }
                }
                Err(e) => error!("Failed to check epoch: {e:?}"),
            }
        }

        operation_queue.mark_should_fire(tick);

        while let Some(task) = operation_queue.get_next_pending() {
            let operation = task.operation;

            if let Err(e) = operation
                .execute(&keeper_config, &mut keeper_state, &mut operation_queue)
                .await
            {
                error!("Operation {operation:?} failed, stopping execution: {e:?}",);
                break;
            }

            // After sending many tx, check epoch info
            if operation.is_heavy_operation() {
                check_and_fire_steward_on_epoch_transition(
                    &keeper_config,
                    &mut keeper_state,
                    &mut last_seen_epoch,
                )
                .await;
            }
        }

        for task in operation_queue.tasks.iter() {
            if matches!(task.state, OperationState::Failed) {
                error!("Operation failed: {}", task.operation.to_string());
            }
        }

        operation_queue.reset_for_next_cycle();

        if should_clear_startup_flag(tick, &intervals) {
            keeper_state.keeper_flags.unset_flag(KeeperFlag::Startup);
        }

        // ---------- SLEEP ----------
        sleep_and_tick(&mut tick).await;
    }
}

async fn check_and_fire_steward_on_epoch_transition(
    keeper_config: &KeeperConfig,
    keeper_state: &mut KeeperState,
    last_seen_epoch: &mut Option<u64>,
) {
    if let Ok(epoch_info) = keeper_config.client.get_epoch_info().await {
        if let Some(last_seen_epoch) = last_seen_epoch {
            if epoch_info.epoch > *last_seen_epoch {
                info!(
                    "EPOCH TRANSITION DETECTED DURING OPERATION! {last_seen_epoch} -> {}",
                    epoch_info.epoch
                );

                *last_seen_epoch = epoch_info.epoch;
                keeper_state.epoch_info = epoch_info;

                keeper_state.set_runs_errors_txs_and_flags_for_epoch(
                    operations::steward::fire(keeper_config, keeper_state).await,
                );

                info!("Epoch transition Steward crank completed, resuming operations");
            }
        }
    }
}

fn main() {
    info!("\n👋 Welcome to the Jito Stakenet Keeper!\n\n");

    dotenv().ok();
    env_logger::init();
    let args = Args::parse();

    let flag_args = Args::parse();
    let run_flags = set_run_flags(&flag_args);

    info!("{}\n\n", args);

    let gossip_entrypoints =
        args.gossip_entrypoints
            .map(|gossip_entrypoints| {
                gossip_entrypoints
                    .iter()
                    .enumerate()
                    .filter_map(|(index, gossip_entrypoint)| {
                        if gossip_entrypoint.is_empty() {
                            None
                        } else {
                            Some(
                                solana_net_utils::parse_host_port(gossip_entrypoint)
                                    .unwrap_or_else(|err| {
                                        panic!(
                                            "Failed to parse gossip entrypoint #{} '{}': {}",
                                            index + 1,
                                            gossip_entrypoint,
                                            err
                                        )
                                    }),
                            )
                        }
                    })
                    .collect()
            })
            .expect("Failed to create socket addresses from gossip entrypoints");

    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let hostname_cmd = Command::new("hostname")
            .output()
            .expect("Failed to execute hostname command");

        let hostname = String::from_utf8_lossy(&hostname_cmd.stdout)
            .trim()
            .to_string();

        set_host_id(format!(
            "stakenet-keeper_{}_{}_{}",
            args.region, args.cluster, hostname
        ));

        let client = Arc::new(RpcClient::new_with_timeout(
            args.json_rpc_url.clone(),
            Duration::from_secs(60),
        ));

        let keypair =
            Arc::new(read_keypair_file(args.keypair).expect("Failed reading keypair file"));

        let oracle_authority_keypair =
            args.oracle_authority_keypair
                .map(|oracle_authority_keypair| {
                    Arc::new(
                        read_keypair_file(oracle_authority_keypair)
                            .expect("Failed reading oracle_authority_keypair keypair file"),
                    )
                });

        let priority_fee_oracle_authority_keypair = args.priority_fee_oracle_authority_keypair.map(
            |priority_fee_oracle_authority_keypair| {
                Arc::new(
                    read_keypair_file(priority_fee_oracle_authority_keypair)
                        .expect("Failed reading priority_fee_oracle_authority_keypair file"),
                )
            },
        );

        let redundant_rpc_urls = args
            .redundant_rpc_urls
            .map(|x| Arc::new(x.into_iter().map(RpcClient::new).collect()));

        let connection = Connection::open(args.sqlite_path.clone()).unwrap();
        create_sqlite_tables(&connection).expect("SQLite tables created");

        let config = KeeperConfig {
            client,
            keypair,
            validator_history_program_id: args.validator_history_program_id,
            tip_distribution_program_id: args.tip_distribution_program_id,
            priority_fee_distribution_program_id: args.priority_fee_distribution_program_id,
            priority_fee_in_microlamports: args.priority_fees,
            steward_program_id: args.steward_program_id,
            steward_config: args.steward_config,
            token_mint: args.token_mint,
            oracle_authority_keypair,
            gossip_entrypoints: Some(gossip_entrypoints),
            validator_history_interval: args.validator_history_interval,
            metrics_interval: args.metrics_interval,
            steward_interval: args.steward_interval,
            block_metadata_interval: args.block_metadata_interval,
            run_flags,
            full_startup: args.full_startup,
            no_pack: args.no_pack,
            pay_for_new_accounts: args.pay_for_new_accounts,
            cool_down_range: args.cool_down_range,
            tx_retry_count: args.tx_retry_count,
            tx_confirmation_seconds: args.tx_confirmation_seconds,
            sqlite_connection: Arc::new(Mutex::new(connection)),
            priority_fee_oracle_authority_keypair,
            redundant_rpc_urls,
            cluster: args.cluster,
            cluster_name: args.cluster.to_string(),
            lookback_epochs: args.lookback_epochs,
            lookback_start_offset_epochs: args.lookback_start_offset_epochs,
            validator_history_min_stake: args.validator_history_min_stake,
        };

        run_keeper(config).await;
    });
}
