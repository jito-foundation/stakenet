/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/

use std::{net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc, time::Duration};

use clap::{arg, command, Parser};
use keeper_core::{
    get_multiple_accounts_batched, get_vote_accounts_with_retry, submit_instructions, submit_transactions, Cluster, CreateUpdateStats, SubmitStats, TransactionExecutionError
};
use log::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::{datapoint_error, set_host_id};
use solana_sdk::{
    instruction::Instruction, pubkey::Pubkey, signature::{read_keypair_file, Keypair, Signer}
};
use tokio::time::sleep;
use validator_history::{constants::MIN_VOTE_EPOCHS, ValidatorHistory};
use validator_keeper::{
    cluster_info::update_cluster_info, emit_cluster_history_datapoint, emit_mev_commission_datapoint, emit_mev_earned_datapoint, emit_validator_commission_datapoint, emit_validator_history_metrics, get_create_validator_history_instructions, get_validator_history_address, gossip::{emit_gossip_datapoint, upload_gossip_values}, mev_commission::{update_mev_commission, update_mev_earned}, stake::{emit_stake_history_datapoint, update_stake_history}, vote_account::update_vote_accounts, KeeperError
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

async fn monitoring_loop(
    client: Arc<RpcClient>,
    program_id: Pubkey,
    keeper_address: Pubkey,
    interval: u64,
) {
    loop {
        match emit_validator_history_metrics(&client, program_id, keeper_address).await {
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
    loop {
        // Continuously runs throughout an epoch, polling for new tip distribution accounts
        // and submitting update txs when new accounts are detected
        let stats = match update_mev_commission(
            client.clone(),
            keypair.clone(),
            &commission_history_program_id,
            &tip_distribution_program_id,
        )
        .await
        {
            Ok(stats) => {
                for message in stats
                    .creates
                    .results
                    .iter()
                    .chain(stats.updates.results.iter())
                {
                    if let Err(e) = message {
                        datapoint_error!("vote-account-error", ("error", e.to_string(), String),);
                    }
                }
                stats
            }
            Err(e) => {
                let mut stats = CreateUpdateStats::default();
                if let KeeperError::TransactionExecutionError(
                    TransactionExecutionError::TransactionClientError(_, results),
                ) = &e
                {
                    stats.updates.successes = results.iter().filter(|r| r.is_ok()).count() as u64;
                    stats.updates.errors = results.iter().filter(|r| r.is_err()).count() as u64;
                }
                datapoint_error!("mev-commission-error", ("error", e.to_string(), String),);
                stats
            }
        };
        emit_mev_commission_datapoint(stats);
        sleep(Duration::from_secs(interval)).await;
    }
}

async fn mev_earned_loop(
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    commission_history_program_id: Pubkey,
    tip_distribution_program_id: Pubkey,
    interval: u64,
) {
    loop {
        // Continuously runs throughout an epoch, polling for tip distribution accounts from the prev epoch with uploaded merkle roots
        // and submitting update_mev_earned (technically update_mev_comission) txs when the uploaded merkle roots are detected
        let stats = match update_mev_earned(
            &client,
            &keypair,
            &commission_history_program_id,
            &tip_distribution_program_id,
        )
        .await
        {
            Ok(stats) => {
                for message in stats
                    .creates
                    .results
                    .iter()
                    .chain(stats.updates.results.iter())
                {
                    if let Err(e) = message {
                        datapoint_error!("vote-account-error", ("error", e.to_string(), String),);
                    }
                }
                stats
            }
            Err(e) => {
                let mut stats = CreateUpdateStats::default();
                if let KeeperError::TransactionExecutionError(
                    TransactionExecutionError::TransactionClientError(_, results),
                ) = &e
                {
                    stats.updates.successes = results.iter().filter(|r| r.is_ok()).count() as u64;
                    stats.updates.errors = results.iter().filter(|r| r.is_err()).count() as u64;
                }
                datapoint_error!("mev-earned-error", ("error", e.to_string(), String),);
                stats
            }
        };
        emit_mev_earned_datapoint(stats);
        sleep(Duration::from_secs(interval)).await;
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
        let should_run = (epoch_info.slot_index > epoch_info.slots_in_epoch / 1000
            && runs_for_epoch < 1)
            || (epoch_info.slot_index > epoch_info.slots_in_epoch / 2 && runs_for_epoch < 2)
            || (epoch_info.slot_index > epoch_info.slots_in_epoch * 9 / 10 && runs_for_epoch < 3);

        if should_run {
            stats = match update_vote_accounts(rpc_client.clone(), keypair.clone(), program_id)
                .await
            {
                Ok(stats) => {
                    for message in stats
                        .creates
                        .results
                        .iter()
                        .chain(stats.updates.results.iter())
                    {
                        if let Err(e) = message {
                            datapoint_error!(
                                "vote-account-error",
                                ("error", e.to_string(), String),
                            );
                        }
                    }
                    if stats.updates.errors == 0 && stats.creates.errors == 0 {
                        runs_for_epoch += 1;
                    }
                    sleep(Duration::from_secs(interval)).await;
                    stats
                }
                Err(e) => {
                    let mut stats = CreateUpdateStats::default();
                    if let KeeperError::TransactionExecutionError(
                        TransactionExecutionError::TransactionClientError(_, results),
                    ) = &e
                    {
                        stats.updates.successes =
                            results.iter().filter(|r| r.is_ok()).count() as u64;
                        stats.updates.errors = results.iter().filter(|r| r.is_err()).count() as u64;
                    }
                    datapoint_error!("vote-account-error", ("error", e.to_string(), String),);
                    stats
                }
            };
        }
        current_epoch = epoch_info.epoch;
        emit_validator_commission_datapoint(stats.clone(), runs_for_epoch);
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
                    for message in stats
                        .creates
                        .results
                        .iter()
                        .chain(stats.updates.results.iter())
                    {
                        if let Err(e) = message {
                            datapoint_error!(
                                "stake-history-error",
                                ("error", e.to_string(), String),
                            );
                        }
                    }

                    if stats.creates.errors == 0 && stats.updates.errors == 0 {
                        runs_for_epoch += 1;
                    }
                    run_stats
                }
                Err(e) => {
                    let mut stats = CreateUpdateStats::default();
                    if let KeeperError::TransactionExecutionError(
                        TransactionExecutionError::TransactionClientError(_, results),
                    ) = &e
                    {
                        stats.updates.successes =
                            results.iter().filter(|r| r.is_ok()).count() as u64;
                        stats.updates.errors = results.iter().filter(|r| r.is_err()).count() as u64;
                    }
                    datapoint_error!("stake-history-error", ("error", e.to_string(), String),);
                    stats
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
                    for message in stats
                        .creates
                        .results
                        .iter()
                        .chain(stats.updates.results.iter())
                    {
                        if let Err(e) = message {
                            datapoint_error!(
                                "gossip-upload-error",
                                ("error", e.to_string(), String),
                            );
                        }
                    }
                    if stats.creates.errors == 0 && stats.updates.errors == 0 {
                        runs_for_epoch += 1;
                    }
                    stats
                }
                Err(e) => {
                    let mut stats = CreateUpdateStats::default();
                    if let Some(TransactionExecutionError::TransactionClientError(_, results)) =
                        e.downcast_ref::<TransactionExecutionError>()
                    {
                        stats.updates.successes =
                            results.iter().filter(|r| r.is_ok()).count() as u64;
                        stats.updates.errors = results.iter().filter(|r| r.is_err()).count() as u64;
                    }

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
                    for message in run_stats.results.iter() {
                        if let Err(e) = message {
                            datapoint_error!(
                                "cluster-history-error",
                                ("error", e.to_string(), String),
                            );
                        }
                    }
                    if run_stats.errors == 0 {
                        runs_for_epoch += 1;
                    }
                    run_stats
                }
                Err(e) => {
                    let mut stats = SubmitStats::default();
                    if let TransactionExecutionError::TransactionClientError(_, results) = &e {
                        stats.successes = results.iter().filter(|r| r.is_ok()).count() as u64;
                        stats.errors = results.iter().filter(|r| r.is_err()).count() as u64;
                    }
                    datapoint_error!("cluster-history-error", ("error", e.to_string(), String),);
                    stats
                }
            };
        }

        current_epoch = epoch;
        emit_cluster_history_datapoint(stats, runs_for_epoch);
        sleep(Duration::from_secs(interval)).await;
    }
}

async fn fire_emit_metrics(
    runs_per_epoch: &u64,
    client: &RpcClient,
    program_id: Pubkey,
    keeper_address: Pubkey,
) {
    match emit_validator_history_metrics(client, program_id, keeper_address).await {
        Ok(_) => {}
        Err(e) => {
            error!("Failed to emit validator history metrics: {}", e);
        }
    }
}

async fn run_create_validator_history_accounts(
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    keypair: &Arc<Keypair>,
) {

    let active_vote_accounts = match get_vote_accounts_with_retry(&client, MIN_VOTE_EPOCHS, None).await {
        Ok(vote_accounts) => vote_accounts,
        Err(e) => {
            error!("Failed to get vote accounts: {}", e);
            return;
        }
    }
    .iter()
    .filter_map(|vote_account_info| {
        match Pubkey::from_str(vote_account_info.vote_pubkey.as_str()) {
            Ok(vote_pubkey) => Some(vote_pubkey),
            Err(e) => {
                error!("Failed to parse vote pubkey: {}", e);
                None
            }
        }
    }).collect::<Vec<Pubkey>>();

    let all_history_addresses = &active_vote_accounts
        .iter()
        .map(|vote_pubkey| get_validator_history_address(vote_pubkey, program_id).clone())
        .collect::<Vec<Pubkey>>();

    let history_accounts = match get_multiple_accounts_batched(
        &all_history_addresses, 
        client
    ).await {
        Ok(history_accounts) => history_accounts,
        Err(e) => {
            error!("Failed to get validator history accounts: {}", e);
            return;
        }
    };

    assert!(active_vote_accounts.len() == history_accounts.len());

    let create_transactions = active_vote_accounts.iter()
        .zip(history_accounts)
        .filter_map(|(vote_pubkey, history_account)| {
            match history_account {
                Some(_) => { None }
                None => {
                    // Create accounts that don't exist
                    let ix = get_create_validator_history_instructions(vote_pubkey, program_id, keypair);
                    Some(ix)
                }
            }
        }).collect::<Vec<Vec<Instruction>>>();


    match submit_transactions(client, create_transactions, keypair).await {
        Ok(_) => {}
        Err(e) => {
            error!("Failed to create validator history accounts: {}", e);
        }
    }

}

async fn run_loop(
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    program_id: Pubkey,
    tip_distribution_program_id: Pubkey,
    interval: u64,
    oracle_authority_keypair: Option<Keypair>,
    gossip_entrypoint: Option<SocketAddr>,
) {
    // Stateful data
    let mut current_epoch = 0;
    let mut metric_runs_per_epoch: u64 = 0;
    let mut cluster_history_runs_per_epoch: u64 = 0;

    // let mut ixs = Vec::new();

    let mut tick: u64 = 0; // 1 second ticks
    loop {
        // ---------- CREATE VALIDATOR HISTORY ACCOUNTS ----------
        if tick % 10 == 0 {
            run_create_validator_history_accounts(&client, &program_id, &keypair).await;
        }

        // ---------- VALIDATOR HISTORY KEEPER FUNCTIONS ----------

        // ---------- STEWARD FUNCTIONS ----------

        // ---------- EMIT METRICS ----------
        if tick % 10 == 0 {
            fire_emit_metrics(
                &metric_runs_per_epoch,
                &client,
                program_id,
                keypair.pubkey(),
            )
            .await;
        }

        // ---------- SLEEP ----------
        sleep(Duration::from_secs(1)).await;
        tick += 1;
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();
    set_host_id(format!("{}", args.cluster));

    let client = Arc::new(RpcClient::new_with_timeout(args.json_rpc_url.clone(), Duration::from_secs(60)));
    let keypair = Arc::new(read_keypair_file(args.keypair).expect("Failed reading keypair file"));

    let oracle_authority_keypair = {
        if let Some(oracle_authority_keypair) = args.oracle_authority_keypair {
            Some(
                read_keypair_file(oracle_authority_keypair)
                    .expect("Failed reading stake keypair file"),
            )
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

    let initial_epoch = client
        .get_epoch_info()
        .await
        .expect("Failed to get first epoch info")
        .epoch;

    info!("Starting validator history keeper...");

    run_loop(
        client,
        keypair,
        args.program_id,
        args.tip_distribution_program_id,
        args.interval,
        oracle_authority_keypair,
        gossip_entrypoint,
    )
    .await;

    // tokio::spawn(monitoring_loop(
    //     Arc::clone(&client),
    //     args.program_id,
    //     keypair.pubkey(),
    //     args.interval,
    // ));

    // tokio::spawn(cluster_history_loop(
    //     Arc::clone(&client),
    //     Arc::clone(&keypair),
    //     args.program_id,
    //     args.interval,
    // ));

    // tokio::spawn(vote_account_loop(
    //     Arc::clone(&client),
    //     Arc::clone(&keypair),
    //     args.program_id,
    //     args.interval,
    // ));

    // tokio::spawn(mev_commission_loop(
    //     client.clone(),
    //     keypair.clone(),
    //     args.program_id,
    //     args.tip_distribution_program_id,
    //     args.interval,
    // ));

    // tokio::spawn(mev_earned_loop(
    //     client.clone(),
    //     keypair.clone(),
    //     args.program_id,
    //     args.tip_distribution_program_id,
    //     args.interval,
    // ));

    // if let Some(oracle_authority_keypair) = args.oracle_authority_keypair {
    //     let oracle_authority_keypair = Arc::new(
    //         read_keypair_file(oracle_authority_keypair).expect("Failed reading stake keypair file"),
    //     );
    //     tokio::spawn(stake_upload_loop(
    //         Arc::clone(&client),
    //         Arc::clone(&oracle_authority_keypair),
    //         args.program_id,
    //         args.interval,
    //     ));

    //     if let Some(gossip_entrypoint) = args.gossip_entrypoint {
    //         let entrypoint = solana_net_utils::parse_host_port(&gossip_entrypoint)
    //             .expect("Failed to parse host and port from gossip entrypoint");
    //         // Cannot be sent to thread because there's a Box<dyn Error> inside
    //         gossip_upload_loop(
    //             client.clone(),
    //             oracle_authority_keypair,
    //             args.program_id,
    //             entrypoint,
    //             args.interval,
    //         )
    //         .await;
    //     }
    // }
    // Need final infinite loop to keep all threads alive
    // loop {
    //     sleep(Duration::from_secs(60)).await;
    // }
}
