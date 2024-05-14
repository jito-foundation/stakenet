/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/

use std::{
    collections::HashMap, default, error::Error, fmt, net::SocketAddr, path::PathBuf, str::FromStr,
    sync::Arc, time::Duration,
};

use anchor_lang::AccountDeserialize;
use clap::{arg, command, Parser};
use keeper_core::{
    get_multiple_accounts_batched, get_vote_accounts_with_retry, submit_instructions,
    submit_transactions, Cluster, CreateUpdateStats, SubmitStats, TransactionExecutionError,
};
use log::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::{datapoint_error, set_host_id};
use solana_sdk::{
    epoch_info::{self, EpochInfo},
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
};
use tokio::time::sleep;
use validator_history::{constants::MIN_VOTE_EPOCHS, ValidatorHistory};
use validator_keeper::{
    cluster_info::update_cluster_info,
    derive_validator_history_address, emit_cluster_history_datapoint,
    emit_mev_commission_datapoint, emit_mev_earned_datapoint, emit_validator_commission_datapoint,
    emit_validator_history_metrics, get_create_validator_history_instructions,
    gossip::{emit_gossip_datapoint, upload_gossip_values},
    mev_commission::{update_mev_commission, update_mev_earned},
    operations::{self, keeper_operations::KeeperOperations},
    stake::{emit_stake_history_datapoint, update_stake_history},
    state::{self, keeper_state::KeeperState},
    vote_account::update_vote_accounts,
    KeeperError,
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
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
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

    // Loop interval time (default 300 sec)
    #[arg(short, long, env, default_value = "300")]
    interval: u64,

    #[arg(short, long, env, default_value_t = Cluster::Mainnet)]
    cluster: Cluster,
}

async fn run_loop(
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    program_id: Pubkey,
    tip_distribution_program_id: Pubkey,
    oracle_authority_keypair: Option<Arc<Keypair>>,
    gossip_entrypoint: Option<SocketAddr>,
) {
    // Stateful data
    let mut keeper_state = KeeperState::new();

    let mut tick: u64 = 0; // 1 second ticks

    loop {
        // ---------- SLEEP ----------
        sleep(Duration::from_secs(1)).await;
        tick += 1;

        if tick % 10 == 0 {
            // ---------------------- FETCH -----------------------------------
            // The fetch ( update ) functions fetch everything we need for the operations from the blockchain
            // These functions will update the keeper_state. If anything fails, no operations will be ran.
            match state::update_epoch::update_epoch(&client, &mut keeper_state).await {
                Ok(_) => {
                    *keeper_state.get_mut_runs_for_epoch(KeeperOperations::UpdateEpoch) += 1;
                }
                Err(e) => {
                    error!("Failed to update epoch: {}", e);
                    *keeper_state.get_mut_errors_for_epoch(KeeperOperations::UpdateEpoch) += 1;
                    continue;
                }
            }

            match state::update_accounts::update_validator_history_map(
                &client,
                &keypair,
                &program_id,
                &mut keeper_state,
            )
            .await
            {
                Ok(_) => {
                    *keeper_state
                        .get_mut_runs_for_epoch(KeeperOperations::CreateValidatorHistory) += 1;
                }
                Err(e) => {
                    error!("Failed to update validator history map: {}", e);
                    *keeper_state
                        .get_mut_errors_for_epoch(KeeperOperations::CreateValidatorHistory) += 1;
                    continue;
                }
            }

            // ---------------------- FIRE -----------------------------------
            // The fire functions will run the operations on the blockchain

            keeper_state.set_runs_and_errors_for_epoch(
                operations::cluster_history::fire_and_emit(
                    &client,
                    &keypair,
                    &program_id,
                    &keeper_state,
                )
                .await,
            );

            keeper_state.set_runs_and_errors_for_epoch(
                operations::vote_account::fire_and_emit(
                    &client,
                    &keypair,
                    &program_id,
                    &keeper_state,
                )
                .await,
            );

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
        if tick % 60 == 0 {
            keeper_state.set_runs_and_errors_for_epoch(
                operations::metrics_emit::fire_and_emit(
                    &client,
                    &keypair,
                    &program_id,
                    &keeper_state,
                )
                .await,
            );
        }
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
    )
    .await;
}
