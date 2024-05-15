/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/
use clap::{arg, command, Parser};
use keeper_core::Cluster;
use log::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::set_host_id;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair},
};
use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::time::sleep;
use validator_keeper::{
    operations::{self, keeper_operations::KeeperOperations},
    state::{
        keeper_state::KeeperState,
        update_state::{create_missing_accounts, post_create_update, pre_create_update},
    },
};

#[derive(Parser, Debug)]
#[command(about = "Keeps commission history accounts up to date")]
struct Args {
    /// RPC URL for the cluster
    #[arg(
        short,
        long,
        env,
        default_value = "https://api.mainnet-beta.solana.com"
    )]
    json_rpc_url: String,

    /// Gossip entrypoint in the form of URL:PORT
    #[arg(short, long, env)]
    gossip_entrypoint: Option<String>,

    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "./credentials/keypair.json")]
    keypair: PathBuf,

    /// Path to keypair used specifically for submitting permissioned transactions
    #[arg(short, long, env)]
    oracle_authority_keypair: Option<PathBuf>,

    /// Validator history program ID (Pubkey as base58 string)
    #[arg(short, long, env)]
    program_id: Pubkey,

    /// Tip distribution program ID (Pubkey as base58 string)
    #[arg(short, long, env)]
    tip_distribution_program_id: Pubkey,

    // DEPRECIATED: Use validator_history_interval instead
    #[arg(short, long, env, default_value = "300")]
    interval: u64,

    // Interval to update Validator History Accounts (default 300 sec)
    #[arg(short, long, env, default_value = "300")]
    validator_history_interval: u64,

    // Interval to emit metrics (default 60 sec)
    #[arg(short, long, env, default_value = "60")]
    metrics_interval: u64,

    #[arg(short, long, env, default_value_t = Cluster::Mainnet)]
    cluster: Cluster,
}

fn should_update(tick: u64, intervals: &Vec<u64>) -> bool {
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

async fn run_loop(
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    program_id: Pubkey,
    tip_distribution_program_id: Pubkey,
    oracle_authority_keypair: Option<Arc<Keypair>>,
    gossip_entrypoint: Option<SocketAddr>,
    validator_history_interval: u64,
    metrics_interval: u64,
) {
    let intervals = vec![validator_history_interval, metrics_interval];

    // Stateful data
    let mut keeper_state = KeeperState::new();
    let mut tick: u64 = 0; // 1 second ticks

    loop {
        // ---------------------- FETCH -----------------------------------
        // The fetch ( update ) functions fetch everything we need for the operations from the blockchain
        // Additionally, this function will update the keeper state. If update fails - it will skip the fire functions.
        if should_update(tick, &intervals) {
            info!("Pre-fetching data for update...");
            match pre_create_update(&client, &keypair, &program_id, &mut keeper_state).await {
                Ok(_) => {
                    keeper_state.increment_update_run_for_epoch(KeeperOperations::PreCreateUpdate);
                }
                Err(e) => {
                    error!("Failed to pre create update: {:?}", e);
                    advance_tick(&mut tick);
                    keeper_state
                        .increment_update_error_for_epoch(KeeperOperations::PreCreateUpdate);
                    continue;
                }
            }

            info!("Creating missing accounts...");
            match create_missing_accounts(&client, &keypair, &program_id, &keeper_state).await {
                Ok(_) => {
                    keeper_state
                        .increment_update_run_for_epoch(KeeperOperations::CreateMissingAccounts);
                }
                Err(e) => {
                    error!("Failed to create missing accounts: {:?}", e);
                    advance_tick(&mut tick);
                    keeper_state
                        .increment_update_error_for_epoch(KeeperOperations::CreateMissingAccounts);
                    continue;
                }
            }

            info!("Post-fetching data for update...");
            match post_create_update(
                &client,
                &program_id,
                &tip_distribution_program_id,
                &mut keeper_state,
            )
            .await
            {
                Ok(_) => {
                    keeper_state.increment_update_run_for_epoch(KeeperOperations::PostCreateUpdate);
                }
                Err(e) => {
                    error!("Failed to post create update: {:?}", e);
                    advance_tick(&mut tick);
                    keeper_state
                        .increment_update_error_for_epoch(KeeperOperations::PostCreateUpdate);
                    continue;
                }
            }
        }

        // ---------------------- FIRE -----------------------------------
        if should_fire(tick, validator_history_interval) {
            info!("Firing operations...");

            info!("Updating cluster history...");
            keeper_state.set_runs_and_errors_for_epoch(
                operations::cluster_history::fire_and_emit(
                    &client,
                    &keypair,
                    &program_id,
                    &keeper_state,
                )
                .await,
            );

            info!("Updating copy vote accounts...");
            keeper_state.set_runs_and_errors_for_epoch(
                operations::vote_account::fire_and_emit(
                    &client,
                    &keypair,
                    &program_id,
                    &keeper_state,
                )
                .await,
            );

            info!("Updating mev commission...");
            keeper_state.set_runs_and_errors_for_epoch(
                operations::mev_commission::fire_and_emit(
                    &client,
                    &keypair,
                    &program_id,
                    &tip_distribution_program_id,
                    &keeper_state,
                )
                .await,
            );

            info!("Updating mev earned...");
            keeper_state.set_runs_and_errors_for_epoch(
                operations::mev_earned::fire_and_emit(
                    &client,
                    &keypair,
                    &program_id,
                    &tip_distribution_program_id,
                    &keeper_state,
                )
                .await,
            );

            if let Some(oracle_authority_keypair) = &oracle_authority_keypair {
                info!("Updating stake accounts...");
                keeper_state.set_runs_and_errors_for_epoch(
                    operations::stake_upload::fire_and_emit(
                        &client,
                        &oracle_authority_keypair,
                        &program_id,
                        &keeper_state,
                    )
                    .await,
                );
            }

            if let (Some(gossip_entrypoint), Some(oracle_authority_keypair)) =
                (gossip_entrypoint, &oracle_authority_keypair)
            {
                info!("Updating gossip accounts...");
                keeper_state.set_runs_and_errors_for_epoch(
                    operations::gossip_upload::fire_and_emit(
                        &client,
                        &oracle_authority_keypair,
                        &program_id,
                        &gossip_entrypoint,
                        &keeper_state,
                    )
                    .await,
                );
            }
        }

        // ---------------------- EMIT METRICS -----------------------------------

        if should_fire(tick, metrics_interval) {
            info!("Emitting metrics...");
            keeper_state.set_runs_and_errors_for_epoch(
                operations::metrics_emit::fire_and_emit(&keeper_state).await,
            );
        }

        // ---------- SLEEP ----------
        sleep_and_tick(&mut tick).await;
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();
    set_host_id(format!("{}", args.cluster));

    let client = Arc::new(RpcClient::new_with_timeout(
        args.json_rpc_url.clone(),
        Duration::from_secs(60),
    ));

    let keypair = Arc::new(read_keypair_file(args.keypair).expect("Failed reading keypair file"));

    let oracle_authority_keypair = {
        if let Some(oracle_authority_keypair) = args.oracle_authority_keypair {
            Some(Arc::new(
                read_keypair_file(oracle_authority_keypair)
                    .expect("Failed reading stake keypair file"),
            ))
        } else {
            None
        }
    };

    let gossip_entrypoint = {
        if let Some(gossip_entrypoint) = args.gossip_entrypoint {
            Some(
                solana_net_utils::parse_host_port(&gossip_entrypoint)
                    .expect("Failed to parse host and port from gossip entrypoint"),
            )
        } else {
            None
        }
    };

    info!("Starting validator history keeper...");

    run_loop(
        client,
        keypair,
        args.program_id,
        args.tip_distribution_program_id,
        oracle_authority_keypair,
        gossip_entrypoint,
        args.validator_history_interval,
        args.metrics_interval,
    )
    .await;
}
