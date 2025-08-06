/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/
use clap::Parser;
use dotenvy::dotenv;
use log::*;
use rand::Rng;
use rusqlite::Connection;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::set_host_id;
use solana_sdk::signature::read_keypair_file;
use stakenet_keeper::{
    operations::{
        self,
        block_metadata::db::create_sqlite_tables,
        keeper_operations::{set_flag, KeeperCreates, KeeperOperations},
    },
    state::{
        keeper_config::{Args, KeeperConfig},
        keeper_state::{KeeperFlag, KeeperState},
        update_state::{create_missing_accounts, post_create_update, pre_create_update},
    },
};
use std::{process::Command, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio::time::sleep;

fn set_run_flags(args: &Args) -> u32 {
    let mut run_flags = 0;

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

fn should_emit(tick: u64, intervals: &[u64]) -> bool {
    intervals.iter().any(|interval| tick % (interval + 1) == 0)
}

fn should_update(tick: u64, intervals: &[u64]) -> bool {
    intervals.iter().any(|interval| tick % interval == 0)
}

fn should_fire(tick: u64, interval: u64) -> bool {
    tick % interval == 0
}

fn advance_tick(tick: &mut u64) {
    *tick += 1;
}

async fn sleep_and_tick(tick: &mut u64) {
    sleep(Duration::from_secs(1)).await;
    advance_tick(tick);
}

/// To reduce transaction collisions, we sleep a random amount after any emit
async fn random_cooldown(range: u8) {
    let mut rng = rand::thread_rng();
    let sleep_duration = rng.gen_range(0..=60 * (range as u64 + 1));

    info!("\n\n⏰ Cooldown for {} seconds\n", sleep_duration);
    sleep(Duration::from_secs(sleep_duration)).await;
}

async fn run_keeper(
    keeper_config: KeeperConfig,
    // gossip_ips: &[IpAddr]
) {
    // Intervals
    let metrics_interval = keeper_config.metrics_interval;
    let validator_history_interval = 60;
    let steward_interval = keeper_config.steward_interval;
    let block_metadata_interval = 60;

    let intervals = vec![
        validator_history_interval,
        metrics_interval,
        steward_interval,
        block_metadata_interval,
    ];

    // Stateful data
    let mut keeper_state = KeeperState::default();
    keeper_state.set_cluster_name(&keeper_config.cluster_name);

    let smallest_interval = intervals.iter().min().unwrap();
    let mut tick: u64 = *smallest_interval; // 1 second ticks - start at metrics interval

    if keeper_config.full_startup {
        keeper_state.keeper_flags.set_flag(KeeperFlag::Startup);
    }

    loop {
        // ---------------------- FETCH -----------------------------------
        // The fetch ( update ) functions fetch everything we need for the operations from the blockchain
        // Additionally, this function will update the keeper state. If update fails - it will skip the fire functions.
        if should_update(tick, &intervals) {
            info!("Pre-fetching data for update...");
            match pre_create_update(&keeper_config, &mut keeper_state).await {
                Ok(_) => {
                    keeper_state.increment_update_run_for_epoch(KeeperOperations::PreCreateUpdate);
                }
                Err(e) => {
                    error!("Failed to pre create update: {:?}", e);

                    keeper_state
                        .increment_update_error_for_epoch(KeeperOperations::PreCreateUpdate);

                    advance_tick(&mut tick);
                    continue;
                }
            }

            if keeper_config.pay_for_new_accounts {
                info!("Creating missing accounts...");
                match create_missing_accounts(&keeper_config, &keeper_state).await {
                    Ok(new_accounts_created) => {
                        keeper_state.increment_update_run_for_epoch(
                            KeeperOperations::CreateMissingAccounts,
                        );

                        let total_txs: usize =
                            new_accounts_created.iter().map(|(_, txs)| txs).sum();
                        keeper_state.increment_update_txs_for_epoch(
                            KeeperOperations::CreateMissingAccounts,
                            total_txs as u64,
                        );

                        new_accounts_created
                            .iter()
                            .for_each(|(operation, created_accounts)| {
                                keeper_state.increment_creations_for_epoch((
                                    operation.clone(),
                                    *created_accounts as u64,
                                ));
                            });
                    }
                    Err(e) => {
                        error!("Failed to create missing accounts: {:?}", e);

                        keeper_state.increment_update_error_for_epoch(
                            KeeperOperations::CreateMissingAccounts,
                        );

                        advance_tick(&mut tick);
                        continue;
                    }
                }
            }

            info!("Post-fetching data for update...");
            match post_create_update(&keeper_config, &mut keeper_state).await {
                Ok(_) => {
                    keeper_state.increment_update_run_for_epoch(KeeperOperations::PostCreateUpdate);
                }
                Err(e) => {
                    error!("Failed to post create update: {:?}", e);

                    keeper_state
                        .increment_update_error_for_epoch(KeeperOperations::PostCreateUpdate);

                    advance_tick(&mut tick);
                    continue;
                }
            }
        }

        // ---------------------- FIRE ------------------------------------

        // VALIDATOR HISTORY
        if should_fire(tick, validator_history_interval) {
            info!("Firing operations...");

            info!("Updating cluster history...");
            keeper_state.set_runs_errors_and_txs_for_epoch(
                operations::cluster_history::fire(&keeper_config, &keeper_state).await,
            );

            info!("Updating copy vote accounts...");
            keeper_state.set_runs_errors_txs_and_flags_for_epoch(
                operations::vote_account::fire(&keeper_config, &keeper_state).await,
            );

            info!("Updating mev commission...");
            keeper_state.set_runs_errors_and_txs_for_epoch(
                operations::mev_commission::fire(&keeper_config, &keeper_state).await,
            );

            info!("Updating mev earned...");
            keeper_state.set_runs_errors_and_txs_for_epoch(
                operations::mev_earned::fire(&keeper_config, &keeper_state).await,
            );

            if keeper_config.oracle_authority_keypair.is_some() {
                info!("Updating stake accounts...");
                keeper_state.set_runs_errors_and_txs_for_epoch(
                    operations::stake_upload::fire(&keeper_config, &keeper_state).await,
                );
            }

            // if keeper_config.oracle_authority_keypair.is_some()
            //     && keeper_config.gossip_entrypoints.is_some()
            // {
            info!("Updating gossip accounts...");
            keeper_state.set_runs_errors_and_txs_for_epoch(
                // operations::gossip_upload::fire(&keeper_config, &keeper_state, gossip_ips).await,
                operations::gossip_upload::fire(&keeper_config, &keeper_state).await,
            );
            // }

            info!("Updating priority fee commission...");
            keeper_state.set_runs_errors_and_txs_for_epoch(
                operations::priority_fee_commission::fire(&keeper_config, &keeper_state).await,
            );

            if !keeper_state.keeper_flags.check_flag(KeeperFlag::Startup) {
                random_cooldown(keeper_config.cool_down_range).await;
            }
        }

        // STEWARD
        if should_fire(tick, steward_interval) {
            info!("Cranking Steward...");
            keeper_state.set_runs_errors_txs_and_flags_for_epoch(
                operations::steward::fire(&keeper_config, &keeper_state).await,
            );

            if !keeper_state.keeper_flags.check_flag(KeeperFlag::Startup) {
                random_cooldown(keeper_config.cool_down_range).await;
            }
        }

        // PRIORITY FEE BLOCK METADATA
        if should_fire(tick, block_metadata_interval)
            && keeper_config
                .priority_fee_oracle_authority_keypair
                .is_some()
        {
            info!("Updating priority fee block metadata...");
            keeper_state.set_runs_errors_and_txs_for_epoch(
                operations::block_metadata::operations::fire(&keeper_config, &keeper_state).await,
            );
        }

        // ---------------------- EMIT ---------------------------------

        if should_fire(tick, metrics_interval) {
            keeper_state.set_runs_errors_and_txs_for_epoch(operations::metrics_emit::fire(
                &keeper_config,
                &keeper_state,
                keeper_config.cluster_name.as_str(),
            ));
        }

        if should_emit(tick, &intervals) {
            info!("Emitting metrics...");
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
        }

        // ---------- CLEAR STARTUP ----------
        if should_clear_startup_flag(tick, &intervals) {
            keeper_state.keeper_flags.unset_flag(KeeperFlag::Startup);
        }

        // ---------- SLEEP ----------
        sleep_and_tick(&mut tick).await;
    }
}

fn main() {
    info!("\n👋 Welcome to the Jito Stakenet Keeper!\n\n");

    dotenv().ok();
    env_logger::init();
    let args = Args::parse();

    let flag_args = Args::parse();
    let run_flags = set_run_flags(&flag_args);

    info!("{}\n\n", args.to_string());

    let gossip_data = args
        .gossip_entrypoints
        .iter()
        .map(|entrypoint| {
            let socket_addr = solana_net_utils::parse_host_port(entrypoint.as_str())
                .expect("Failed to parse host and port from gossip entrypoint");
            let ip_addr = solana_net_utils::get_public_ip_addr(&socket_addr)
                .expect("Failed to get public ip address for gossip node");

            (socket_addr, ip_addr)
        })
        .collect();

    // let gossip_ips: Vec<IpAddr> = gossip_entrypoints
    //     .iter()
    //     .map(|gossip_entrypoint| {
    //         solana_net_utils::get_public_ip_addr(&gossip_entrypoint)
    //             .expect("Failed to get public ip address for gossip node")
    //     })
    //     .collect();

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
            oracle_authority_keypair,
            gossip_data,
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
        };

        // run_keeper(config, gossip_ips).await;
        run_keeper(config).await;
    });
}
