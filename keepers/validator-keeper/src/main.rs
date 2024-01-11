/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/

use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use clap::{arg, command, Parser};
use keeper_core::{Cluster, CreateUpdateStats, SubmitStats};
use log::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::{datapoint_error, set_host_id};
use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair},
};
use tokio::time::sleep;
use validator_keeper::{
    cluster_info::update_cluster_info,
    emit_cluster_history_datapoint, emit_mev_commission_datapoint, emit_mev_earned_datapoint,
    emit_validator_commission_datapoint, emit_validator_history_metrics,
    gossip::{emit_gossip_datapoint, upload_gossip_values},
    mev_commission::{update_mev_commission, update_mev_earned},
    stake::{emit_stake_history_datapoint, update_stake_history},
    vote_account::update_vote_accounts,
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
    gossip_entrypoint: String,

    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair: PathBuf,

    /// Path to keypair used specifically for submitting stake upload transactions
    #[arg(short, long, env)]
    stake_upload_keypair: Option<PathBuf>,

    /// Validator history program ID (Pubkey as base58 string)
    #[arg(short, long, env)]
    program_id: Pubkey,

    /// Tip distribution program ID (Pubkey as base58 string)
    #[arg(short, long, env)]
    tip_distribution_program_id: Pubkey,

    // Loop interval time (default 300 sec)
    #[arg(short, long, env, default_value = "300")]
    interval: u64,

    #[arg(short, long, env, default_value_t = Cluster::Mainnet)]
    cluster: Cluster,
}

async fn monitoring_loop(client: Arc<RpcClient>, program_id: Pubkey, interval: u64) {
    loop {
        match emit_validator_history_metrics(&client, program_id).await {
            Ok(_) => {}
            Err(e) => {
                error!("Failed to emit validator history metrics: {}", e);
            }
        }
        sleep(Duration::from_secs(interval)).await;
    }
}

async fn mev_commission_loop(
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    commission_history_program_id: Pubkey,
    tip_distribution_program_id: Pubkey,
    interval: u64,
) {
    let mut prev_epoch = 0;
    // {TipDistributionAccount : VoteAccount}
    let mut validators_updated: HashMap<Pubkey, Pubkey> = HashMap::new();

    loop {
        // Continuously runs throughout an epoch, polling for new tip distribution accounts
        // and submitting update txs when new accounts are detected
        match update_mev_commission(
            client.clone(),
            keypair.clone(),
            &commission_history_program_id,
            &tip_distribution_program_id,
            &mut validators_updated,
            &mut prev_epoch,
        )
        .await
        {
            Ok(stats) => {
                emit_mev_commission_datapoint(stats);
                sleep(Duration::from_secs(interval)).await;
            }
            Err((e, stats)) => {
                emit_mev_commission_datapoint(stats);
                datapoint_error!("mev-commission-error", ("error", e.to_string(), String),);
                sleep(Duration::from_secs(5)).await;
            }
        };
    }
}

async fn mev_earned_loop(
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    commission_history_program_id: Pubkey,
    tip_distribution_program_id: Pubkey,
    interval: u64,
) {
    let mut curr_epoch = 0;
    // {TipDistributionAccount : VoteAccount}
    let mut validators_updated: HashMap<Pubkey, Pubkey> = HashMap::new();

    loop {
        // Continuously runs throughout an epoch, polling for tip distribution accounts from the prev epoch with uploaded merkle roots
        // and submitting update_mev_earned (technically update_mev_comission) txs when the uploaded merkle roots are detected
        match update_mev_earned(
            client.clone(),
            keypair.clone(),
            &commission_history_program_id,
            &tip_distribution_program_id,
            &mut validators_updated,
            &mut curr_epoch,
        )
        .await
        {
            Ok(stats) => {
                emit_mev_earned_datapoint(stats);
                sleep(Duration::from_secs(interval)).await;
            }
            Err((e, stats)) => {
                emit_mev_earned_datapoint(stats);
                datapoint_error!("mev-earned-error", ("error", e.to_string(), String),);
                sleep(Duration::from_secs(5)).await;
            }
        };
    }
}

async fn vote_account_loop(
    rpc_client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    program_id: Pubkey,
    interval: u64,
) {
    let mut runs_for_epoch = 0;
    let mut current_epoch = 0;
    let mut stats = CreateUpdateStats::default();
    loop {
        let epoch_info = match rpc_client.get_epoch_info().await {
            Ok(epoch_info) => epoch_info,
            Err(e) => {
                error!("Failed to get epoch info: {}", e);
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        if current_epoch != epoch_info.epoch {
            runs_for_epoch = 0;
        }
        // Run at 10%, 50% and 90% completion of epoch
        let should_run = (epoch_info.slot_index > epoch_info.slots_in_epoch / 10
            && runs_for_epoch < 1)
            || (epoch_info.slot_index > epoch_info.slots_in_epoch / 2 && runs_for_epoch < 2)
            || (epoch_info.slot_index > epoch_info.slots_in_epoch * 9 / 10 && runs_for_epoch < 3);

        if should_run {
            stats =
                match update_vote_accounts(rpc_client.clone(), keypair.clone(), program_id).await {
                    Ok(stats) => {
                        runs_for_epoch += 1;
                        sleep(Duration::from_secs(interval)).await;
                        stats
                    }
                    Err((e, stats)) => {
                        datapoint_error!("vote-account-error", ("error", e.to_string(), String),);
                        stats
                    }
                };
        }
        current_epoch = epoch_info.epoch;
        emit_validator_commission_datapoint(stats, runs_for_epoch);
        sleep(Duration::from_secs(interval)).await;
    }
}

async fn stake_upload_loop(
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    program_id: Pubkey,
    interval: u64,
) {
    let mut runs_for_epoch = 0;
    let mut current_epoch = 0;

    loop {
        let epoch_info = match client.get_epoch_info().await {
            Ok(epoch_info) => epoch_info,
            Err(e) => {
                error!("Failed to get epoch info: {}", e);
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        let epoch = epoch_info.epoch;
        let mut stats = CreateUpdateStats::default();

        if current_epoch != epoch {
            runs_for_epoch = 0;
        }
        // Run at 0.1%, 50% and 90% completion of epoch
        let should_run = (epoch_info.slot_index > epoch_info.slots_in_epoch / 1000
            && runs_for_epoch < 1)
            || (epoch_info.slot_index > epoch_info.slots_in_epoch / 2 && runs_for_epoch < 2)
            || (epoch_info.slot_index > epoch_info.slots_in_epoch * 9 / 10 && runs_for_epoch < 3);
        if should_run {
            stats = match update_stake_history(client.clone(), keypair.clone(), &program_id).await {
                Ok(run_stats) => {
                    runs_for_epoch += 1;
                    run_stats
                }
                Err((e, run_stats)) => {
                    datapoint_error!("stake-history-error", ("error", e.to_string(), String),);
                    run_stats
                }
            };
        }

        current_epoch = epoch;
        emit_stake_history_datapoint(stats, runs_for_epoch);
        sleep(Duration::from_secs(interval)).await;
    }
}

async fn gossip_upload_loop(
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    program_id: Pubkey,
    entrypoint: SocketAddr,
    interval: u64,
) {
    let mut runs_for_epoch = 0;
    let mut current_epoch = 0;
    loop {
        let epoch_info = match client.get_epoch_info().await {
            Ok(epoch_info) => epoch_info,
            Err(e) => {
                error!("Failed to get epoch info: {}", e);
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        let epoch = epoch_info.epoch;
        if current_epoch != epoch {
            runs_for_epoch = 0;
        }
        // Run at 0%, 50% and 90% completion of epoch
        let should_run = runs_for_epoch < 1
            || (epoch_info.slot_index > epoch_info.slots_in_epoch / 2 && runs_for_epoch < 2)
            || (epoch_info.slot_index > epoch_info.slots_in_epoch * 9 / 10 && runs_for_epoch < 3);

        let mut stats = CreateUpdateStats::default();
        if should_run {
            stats = match upload_gossip_values(
                client.clone(),
                keypair.clone(),
                entrypoint,
                &program_id,
            )
            .await
            {
                Ok(stats) => {
                    runs_for_epoch += 1;
                    stats
                }
                Err((e, stats)) => {
                    datapoint_error!("gossip-upload-error", ("error", e.to_string(), String),);
                    stats
                }
            };
        }
        current_epoch = epoch;
        emit_gossip_datapoint(stats, runs_for_epoch);
        sleep(Duration::from_secs(interval)).await;
    }
}

async fn cluster_history_loop(
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    program_id: Pubkey,
    interval: u64,
) {
    let mut runs_for_epoch = 0;
    let mut current_epoch = 0;

    loop {
        let epoch_info = match client.get_epoch_info().await {
            Ok(epoch_info) => epoch_info,
            Err(e) => {
                error!("Failed to get epoch info: {}", e);
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };
        let epoch = epoch_info.epoch;

        let mut stats = SubmitStats::default();

        if current_epoch != epoch {
            runs_for_epoch = 0;
        }

        // Run at 0.1%, 50% and 90% completion of epoch
        let should_run = (epoch_info.slot_index > epoch_info.slots_in_epoch / 1000
            && runs_for_epoch < 1)
            || (epoch_info.slot_index > epoch_info.slots_in_epoch / 2 && runs_for_epoch < 2)
            || (epoch_info.slot_index > epoch_info.slots_in_epoch * 9 / 10 && runs_for_epoch < 3);
        if should_run {
            stats = match update_cluster_info(client.clone(), keypair.clone(), &program_id).await {
                Ok(run_stats) => {
                    runs_for_epoch += 1;
                    run_stats
                }
                Err((e, run_stats)) => {
                    datapoint_error!("cluster-history-error", ("error", e.to_string(), String),);
                    run_stats
                }
            };
        }

        current_epoch = epoch;
        emit_cluster_history_datapoint(stats, runs_for_epoch);
        sleep(Duration::from_secs(interval)).await;
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
    let entrypoint = solana_net_utils::parse_host_port(&args.gossip_entrypoint)
        .expect("Failed to parse host and port from gossip entrypoint");

    info!("Starting validator history keeper...");

    tokio::spawn(monitoring_loop(
        Arc::clone(&client),
        args.program_id,
        args.interval,
    ));

    tokio::spawn(cluster_history_loop(
        Arc::clone(&client),
        Arc::clone(&keypair),
        args.program_id,
        args.interval,
    ));

    tokio::spawn(vote_account_loop(
        Arc::clone(&client),
        Arc::clone(&keypair),
        args.program_id,
        args.interval,
    ));

    if let Some(stake_upload_keypair) = args.stake_upload_keypair {
        let stake_upload_keypair = Arc::new(
            read_keypair_file(stake_upload_keypair).expect("Failed reading stake keypair file"),
        );
        tokio::spawn(stake_upload_loop(
            Arc::clone(&client),
            Arc::clone(&stake_upload_keypair),
            args.program_id,
            args.interval,
        ));
    }

    tokio::spawn(mev_commission_loop(
        Arc::clone(&client),
        Arc::clone(&keypair),
        args.program_id,
        args.tip_distribution_program_id,
        args.interval,
    ));

    tokio::spawn(mev_earned_loop(
        Arc::clone(&client),
        Arc::clone(&keypair),
        args.program_id,
        args.tip_distribution_program_id,
        args.interval,
    ));
    gossip_upload_loop(client, keypair, args.program_id, entrypoint, args.interval).await;
}
