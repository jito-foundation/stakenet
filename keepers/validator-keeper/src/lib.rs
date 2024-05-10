use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{atomic::AtomicBool, Arc},
};

use anchor_lang::{AccountDeserialize, Discriminator};
use keeper_core::{
    get_multiple_accounts_batched, get_vote_accounts_with_retry, CreateUpdateStats,
    MultipleAccountsError, SubmitStats, TransactionExecutionError,
};
use log::error;
use solana_account_decoder::UiDataSliceConfig;
use solana_client::{
    client_error::ClientError,
    nonblocking::rpc_client::RpcClient,
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    rpc_filter::{Memcmp, RpcFilterType},
};
use solana_gossip::{
    cluster_info::ClusterInfo, gossip_service::GossipService,
    legacy_contact_info::LegacyContactInfo,
};
use solana_metrics::datapoint_info;
use solana_net_utils::bind_in_range;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    vote::program::id as get_vote_program_id,
};
use solana_streamer::socket::SocketAddrSpace;

use jito_tip_distribution::state::TipDistributionAccount;
use thiserror::Error as ThisError;
use validator_history::{
    constants::MIN_VOTE_EPOCHS, ClusterHistory, ValidatorHistory, ValidatorHistoryEntry,
};

pub mod cluster_info;
pub mod gossip;
pub mod mev_commission;
pub mod stake;
pub mod vote_account;

pub type Error = Box<dyn std::error::Error>;

pub const PRIORITY_FEE: u64 = 500_000;

#[derive(ThisError, Debug)]
pub enum KeeperError {
    #[error(transparent)]
    ClientError(#[from] ClientError),
    #[error(transparent)]
    TransactionExecutionError(#[from] TransactionExecutionError),
    #[error(transparent)]
    MultipleAccountsError(#[from] MultipleAccountsError),
    #[error("Custom: {0}")]
    Custom(String),
}

pub async fn get_tip_distribution_accounts(
    rpc_client: &RpcClient,
    tip_distribution_program: &Pubkey,
    epoch: u64,
) -> Result<Vec<Pubkey>, Error> {
    const EPOCH_OFFSET: usize = 8 + 32 + 32 + 1; // Discriminator + Pubkey + Pubkey + size of "None" Option<T>
    let config = RpcProgramAccountsConfig {
        filters: Some(vec![
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                0,
                TipDistributionAccount::discriminator().into(),
            )),
            RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                EPOCH_OFFSET,
                epoch.to_le_bytes().to_vec(),
            )),
        ]),
        account_config: RpcAccountInfoConfig {
            encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
            data_slice: Some(UiDataSliceConfig {
                offset: EPOCH_OFFSET,
                length: 8,
            }),
            ..RpcAccountInfoConfig::default()
        },
        ..RpcProgramAccountsConfig::default()
    };
    let res = rpc_client
        .get_program_accounts_with_config(tip_distribution_program, config)
        .await?;

    // we actually don't care about the data slice, we just want the pubkey
    Ok(res.into_iter().map(|x| x.0).collect::<Vec<Pubkey>>())
}

pub fn emit_mev_commission_datapoint(stats: CreateUpdateStats) {
    datapoint_info!(
        "mev-commission-stats",
        ("num_creates_success", stats.creates.successes, i64),
        ("num_creates_error", stats.creates.errors, i64),
        ("num_updates_success", stats.updates.successes, i64),
        ("num_updates_error", stats.updates.errors, i64),
    );
}

pub fn emit_mev_earned_datapoint(stats: CreateUpdateStats) {
    datapoint_info!(
        "mev-earned-stats",
        ("num_creates_success", stats.creates.successes, i64),
        ("num_creates_error", stats.creates.errors, i64),
        ("num_updates_success", stats.updates.successes, i64),
        ("num_updates_error", stats.updates.errors, i64),
    );
}

pub fn emit_validator_commission_datapoint(stats: CreateUpdateStats, runs_for_epoch: i64) {
    datapoint_info!(
        "vote-account-stats",
        ("num_creates_success", stats.creates.successes, i64),
        ("num_creates_error", stats.creates.errors, i64),
        ("num_updates_success", stats.updates.successes, i64),
        ("num_updates_error", stats.updates.errors, i64),
        ("runs_for_epoch", runs_for_epoch, i64),
    );
}

pub fn emit_cluster_history_datapoint(stats: SubmitStats, runs_for_epoch: i64) {
    datapoint_info!(
        "cluster-history-stats",
        ("num_success", stats.successes, i64),
        ("num_errors", stats.errors, i64),
        ("runs_for_epoch", runs_for_epoch, i64),
    );
}

pub async fn emit_validator_history_metrics(
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<(), Box<dyn std::error::Error>> {
    let epoch = client.get_epoch_info().await?;

    let validator_histories = get_validator_history_accounts(client, program_id).await?;

    let mut ips = 0;
    let mut versions = 0;
    let mut types = 0;
    let mut mev_comms = 0;
    let mut comms = 0;
    let mut epoch_credits = 0;
    let mut stakes = 0;
    let num_validators = validator_histories.len();
    let default = ValidatorHistoryEntry::default();

    let mut all_history_vote_accounts = Vec::new();
    for validator_history in validator_histories {
        if let Some(entry) = validator_history.history.last() {
            if entry.epoch as u64 != epoch.epoch {
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

        all_history_vote_accounts.push(validator_history.vote_account);
    }

    let (cluster_history_address, _) =
        Pubkey::find_program_address(&[ClusterHistory::SEED], &program_id);
    let cluster_history_account = client.get_account(&cluster_history_address).await?;
    let cluster_history =
        ClusterHistory::try_deserialize(&mut cluster_history_account.data.as_slice())?;

    let mut cluster_history_blocks: i64 = 0;
    let cluster_history_entry = cluster_history.history.last();
    if let Some(cluster_history) = cluster_history_entry {
        // Looking for previous epoch to be updated
        if cluster_history.epoch as u64 == epoch.epoch - 1 {
            cluster_history_blocks = 1;
        }
    }

    let get_vote_accounts = get_vote_accounts_with_retry(client, MIN_VOTE_EPOCHS, None).await?;

    let get_vote_accounts_count = get_vote_accounts.len() as i64;

    let vote_program_id = get_vote_program_id();
    let live_validator_histories_count =
        get_multiple_accounts_batched(&all_history_vote_accounts, client)
            .await
            .expect("Cannot fetch validator history vote accounts")
            .iter()
            .filter(|&account| {
                account
                    .as_ref()
                    .map_or(false, |acc| acc.owner == vote_program_id)
            })
            .count();

    let get_vote_accounts_voting = get_vote_accounts
        .iter()
        .filter(|x| {
            // Check if the last epoch credit ( most recent ) is the current epoch
            x.epoch_credits.last().unwrap().0 == epoch.epoch
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
        ("num_commissions", comms, i64),
        ("num_epoch_credits", epoch_credits, i64),
        ("num_stakes", stakes, i64),
        ("cluster_history_blocks", cluster_history_blocks, i64),
        ("slot_index", epoch.slot_index, i64),
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
    );

    Ok(())
}

pub async fn get_validator_history_accounts(
    client: &RpcClient,
    program_id: Pubkey,
) -> Result<Vec<ValidatorHistory>, ClientError> {
    let gpa_config = RpcProgramAccountsConfig {
        filters: Some(vec![RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            0,
            ValidatorHistory::discriminator().into(),
        ))]),
        account_config: RpcAccountInfoConfig {
            encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
            ..RpcAccountInfoConfig::default()
        },
        ..RpcProgramAccountsConfig::default()
    };
    let mut validator_history_accounts = client
        .get_program_accounts_with_config(&program_id, gpa_config)
        .await?;

    let validator_histories = validator_history_accounts
        .iter_mut()
        .filter_map(|(_, account)| {
            ValidatorHistory::try_deserialize(&mut account.data.as_slice()).ok()
        })
        .collect::<Vec<_>>();

    Ok(validator_histories)
}

pub async fn get_validator_history_accounts_with_retry(
    client: &RpcClient,
    program_id: Pubkey,
) -> Result<Vec<ValidatorHistory>, ClientError> {
    for _ in 0..4 {
        if let Ok(validator_histories) = get_validator_history_accounts(client, program_id).await {
            return Ok(validator_histories);
        }
    }
    get_validator_history_accounts(client, program_id).await
}

pub fn start_spy_server(
    cluster_entrypoint: SocketAddr,
    gossip_port: u16,
    spy_socket_addr: SocketAddr,
    keypair: &Arc<Keypair>,
    exit: &Arc<AtomicBool>,
) -> (GossipService, Arc<ClusterInfo>) {
    // bind socket to expected port
    let (_, gossip_socket) = bind_in_range(
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        (gossip_port, gossip_port + 1),
    )
    .map_err(|e| {
        error!("Failed to bind to expected port");
        e
    })
    .expect("Failed to bind to expected gossip port");

    // connect to entrypoint and start spying on gossip
    let node = ClusterInfo::gossip_contact_info(keypair.pubkey(), spy_socket_addr, 0);
    let cluster_info = Arc::new(ClusterInfo::new(
        node,
        keypair.clone(),
        SocketAddrSpace::Unspecified,
    ));

    cluster_info.set_entrypoint(LegacyContactInfo::new_gossip_entry_point(
        &cluster_entrypoint,
    ));
    let gossip_service =
        GossipService::new(&cluster_info, None, gossip_socket, None, true, None, exit);
    (gossip_service, cluster_info)
}
