use crate::entries::gossip_entry::GossipEntry;
/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/
use crate::state::keeper_config::KeeperConfig;
use crate::state::keeper_state::KeeperState;
use bytemuck::{bytes_of, Pod, Zeroable};
use log::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_response::RpcVoteAccountInfo;
use solana_gossip::crds::Crds;
use solana_gossip::crds_data::CrdsData;
use solana_gossip::crds_value::{CrdsValue, CrdsValueLabel};
use solana_gossip::gossip_service::make_gossip_node;
use solana_metrics::{datapoint_error, datapoint_info};
use solana_sdk::signature::Signable;
use solana_sdk::{
    epoch_info::EpochInfo,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use solana_streamer::socket::SocketAddrSpace;
use stakenet_sdk::models::submit_stats::SubmitStats;
use stakenet_sdk::utils::transactions::submit_transactions;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLockReadGuard;
use std::{collections::HashMap, net::SocketAddr, str::FromStr, sync::Arc, time::Duration};
use tokio::time::sleep;
use validator_history::ValidatorHistory;
use validator_history::ValidatorHistoryEntry;

use super::keeper_operations::{check_flag, KeeperOperations};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const IP_ECHO_HEADER_LEN: usize = 4;
const IP_ADDR_OFFSET_V4: usize = 8;
const SHRED_VERSION_OFFSET: usize = IP_ECHO_HEADER_LEN + IP_ADDR_OFFSET_V4;
const IP_ECHO_REQUEST: &[u8] = &[0x00; 21]; // IP echo server expects 21 bytes
const IP_ECHO_RESPONSE_LEN: usize = 27; // IP echo server will always respond with 27 bytes

struct Ipv4EchoResponse {
    ip: IpAddr,
    shred_version: Option<u16>,
}

impl TryFrom<&[u8]> for Ipv4EchoResponse {
    type Error = Box<dyn std::error::Error>;

    fn try_from(data: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        if data.len() < IP_ECHO_RESPONSE_LEN {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "Expected at least {} bytes, got {} bytes",
                    IP_ECHO_RESPONSE_LEN,
                    data.len()
                ),
            )));
        }
        let octets = &data[IP_ADDR_OFFSET_V4..IP_ADDR_OFFSET_V4 + 4];
        let shred_version_bytes = &data[SHRED_VERSION_OFFSET..SHRED_VERSION_OFFSET + 3];
        let ip = IpAddr::V4(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]));
        // Skip the option flag byte
        let shred_version = if data[SHRED_VERSION_OFFSET..SHRED_VERSION_OFFSET + 1] == [0] {
            None
        } else {
            Some(u16::from_le_bytes([
                shred_version_bytes[1],
                shred_version_bytes[2],
            ]))
        };
        Ok(Ipv4EchoResponse { ip, shred_version })
    }
}

struct Ipv4EchoClient {
    gossip_entrypoint: String,
}

impl Ipv4EchoClient {
    pub fn new<S: AsRef<str>>(gossip_entrypoint: S) -> Self {
        Self {
            gossip_entrypoint: gossip_entrypoint.as_ref().to_string(),
        }
    }

    pub async fn fetch_ip_and_shred_version(
        &mut self,
    ) -> Result<Ipv4EchoResponse, Box<dyn std::error::Error>> {
        let mut tcp_stream = TcpStream::connect(&self.gossip_entrypoint)
            .await
            .map_err(|e| format!("Failed to connect to {}: {}", self.gossip_entrypoint, e))?;
        tcp_stream
            .write_all(IP_ECHO_REQUEST)
            .await
            .map_err(|e| format!("Failed to write to {}: {}", self.gossip_entrypoint, e))?;
        tcp_stream.flush().await.map_err(|e| {
            format!(
                "Failed to flush TCP stream to {}: {}",
                self.gossip_entrypoint, e
            )
        })?;
        let mut buffer = vec![0u8; IP_ECHO_RESPONSE_LEN];
        let response_bytes = tcp_stream.read(&mut buffer).await.map_err(|e| {
            format!(
                "Failed to read response from {}: {}",
                self.gossip_entrypoint, e
            )
        })?;
        if response_bytes != IP_ECHO_RESPONSE_LEN {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "Expected {} bytes, got {} bytes from {}",
                    IP_ECHO_RESPONSE_LEN, response_bytes, self.gossip_entrypoint
                ),
            )));
        }
        Ipv4EchoResponse::try_from(&buffer[..response_bytes])
    }
}

fn _get_operation() -> KeeperOperations {
    KeeperOperations::GossipUpload
}

fn _should_run(epoch_info: &EpochInfo, runs_for_epoch: u64) -> bool {
    // Run at 0%, 50% and 90% completion of epoch
    runs_for_epoch < 1
        || (epoch_info.slot_index > epoch_info.slots_in_epoch / 2 && runs_for_epoch < 2)
        || (epoch_info.slot_index > epoch_info.slots_in_epoch * 9 / 10 && runs_for_epoch < 3)
}

#[allow(clippy::too_many_arguments)]
async fn _process(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    priority_fee_in_microlamports: u64,
    entrypoint: &SocketAddr,
    keeper_state: &KeeperState,
    retry_count: u16,
    confirmation_time: u64,
    cluster_name: &str,
) -> Result<SubmitStats, Box<dyn std::error::Error>> {
    upload_gossip_values(
        client,
        keypair,
        program_id,
        priority_fee_in_microlamports,
        entrypoint,
        keeper_state,
        retry_count,
        confirmation_time,
        cluster_name,
    )
    .await
}

pub async fn fire(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64, u64) {
    let client = &keeper_config.client;
    let keypair = &keeper_config.keypair;
    let program_id = &keeper_config.validator_history_program_id;
    let entrypoint = &keeper_config
        .gossip_entrypoint
        .expect("Entry point not set");

    let priority_fee_in_microlamports = keeper_config.priority_fee_in_microlamports;
    let retry_count = keeper_config.tx_retry_count;
    let confirmation_time = keeper_config.tx_confirmation_seconds;

    let operation = _get_operation();
    let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) =
        keeper_state.copy_runs_errors_and_txs_for_epoch(operation);

    let should_run = _should_run(&keeper_state.epoch_info, runs_for_epoch)
        && check_flag(keeper_config.run_flags, operation);

    if should_run {
        match _process(
            client,
            keypair,
            program_id,
            priority_fee_in_microlamports,
            entrypoint,
            keeper_state,
            retry_count,
            confirmation_time,
            &keeper_config.cluster_name,
        )
        .await
        {
            Ok(stats) => {
                for message in stats.results.iter().chain(stats.results.iter()) {
                    if let Err(e) = message {
                        datapoint_error!("gossip-upload-error", ("error", e.to_string(), String),);
                    } else {
                        txs_for_epoch += 1;
                    }
                }
                if stats.errors == 0 {
                    runs_for_epoch += 1;
                }
            }
            Err(e) => {
                datapoint_error!("gossip-upload-error", ("error", e.to_string(), String),);
                errors_for_epoch += 1;
            }
        }
    }

    (operation, runs_for_epoch, errors_for_epoch, txs_for_epoch)
}

// ----------------- OPERATION SPECIFIC FUNCTIONS -----------------

fn check_entry_valid(
    entry: &CrdsValue,
    validator_history: &ValidatorHistory,
    validator_identity: Pubkey,
) -> bool {
    // Filters out invalid gossip entries that would fail transaction submission. Checks for:
    // 0. Entry belongs to one of the expected types
    // 1. Entry timestamp is not too old
    // 2. Entry is for the correct validator
    match &entry.data {
        CrdsData::ContactInfo(contact_info) => {
            if contact_info.wallclock() < validator_history.last_ip_timestamp
                || contact_info.wallclock() < validator_history.last_version_timestamp
            {
                return false;
            }
        }
        _ => {
            return false;
        }
    };

    let signer = entry.pubkey();

    if signer != validator_identity {
        error!(
            "Invalid gossip value retrieved for validator {}",
            validator_identity
        );
        return false;
    }
    true
}

fn build_gossip_entry(
    vote_account: &RpcVoteAccountInfo,
    validator_history: &ValidatorHistory,
    crds: &RwLockReadGuard<'_, Crds>,
    program_id: Pubkey,
    keypair: &Arc<Keypair>,
) -> Option<Vec<GossipEntry>> {
    let validator_identity = Pubkey::from_str(&vote_account.node_pubkey).ok()?;
    let validator_vote_pubkey = Pubkey::from_str(&vote_account.vote_pubkey).ok()?;

    let contact_info_key: CrdsValueLabel = CrdsValueLabel::ContactInfo(validator_identity);

    // ContactInfo is the only gossip message we are interested in. Legacy* and Version
    // are fully deprecated and will not be transmitted on the gossip network.
    if let Some(entry) = crds.get::<&CrdsValue>(&contact_info_key) {
        if !check_entry_valid(entry, validator_history, validator_identity) {
            println!("Invalid entry for validator {}", validator_vote_pubkey);
            return None;
        }
        return Some(vec![GossipEntry::new(
            &validator_vote_pubkey,
            &entry.get_signature(),
            &entry.signable_data(),
            &program_id,
            &entry.pubkey(),
            &keypair.pubkey(),
        )]);
    }
    None
}

#[allow(clippy::too_many_arguments)]
pub async fn upload_gossip_values(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    priority_fee_in_microlamports: u64,
    entrypoint: &SocketAddr,
    keeper_state: &KeeperState,
    retry_count: u16,
    confirmation_time: u64,
    cluster_name: &str,
) -> Result<SubmitStats, Box<dyn std::error::Error>> {
    let vote_accounts = keeper_state.vote_account_map.values().collect::<Vec<_>>();
    let validator_history_map = &keeper_state.validator_history_map;

    // Modified from solana-gossip::main::process_spy and discover
    let exit: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    let mut ip_echo_client = Ipv4EchoClient::new(entrypoint.to_string());
    let ip_echo_response = ip_echo_client
        .fetch_ip_and_shred_version()
        .await
        .map_err(|_| "Failed to fetch IP and shred version from gossip entrypoint")?;

    let gossip_ip = ip_echo_response.ip;
    let cluster_shred_version = ip_echo_response.shred_version.unwrap_or(0);

    let gossip_addr = SocketAddr::new(
        gossip_ip,
        solana_net_utils::find_available_port_in_range(IpAddr::V4(Ipv4Addr::UNSPECIFIED), (0, 1))
            .expect("unable to find an available gossip port"),
    );

    let (_gossip_service, _ip_echo, cluster_info) = make_gossip_node(
        Keypair::from_base58_string(keypair.to_base58_string().as_str()),
        Some(entrypoint),
        exit.clone(),
        Some(&gossip_addr),
        cluster_shred_version,
        true,
        SocketAddrSpace::Global,
    );

    info!(
        "Gossip service started on {} with entrypoint {}. Waiting for validators to be discovered...",
        gossip_addr,
        entrypoint
    );
    // Wait for all active validators to be received
    sleep(Duration::from_secs(150)).await;

    let gossip_entries = {
        let crds = cluster_info
            .gossip
            .crds
            .read()
            .map_err(|e: std::sync::PoisonError<RwLockReadGuard<Crds>>| e.to_string())?;

        vote_accounts
            .iter()
            .filter_map(|vote_account| {
                let vote_account_pubkey = Pubkey::from_str(&vote_account.vote_pubkey).ok()?;
                let validator_history_account = validator_history_map.get(&vote_account_pubkey)?;

                build_gossip_entry(
                    vote_account,
                    validator_history_account,
                    &crds,
                    *program_id,
                    keypair,
                )
            })
            .flatten()
            .collect::<Vec<_>>()
    };

    datapoint_info!(
        "gossip-upload-info",
        ("validator_gossip_nodes", gossip_entries.len(), i64),
        "cluster" => cluster_name,
    );

    exit.store(true, Ordering::Relaxed);

    let update_transactions = gossip_entries
        .iter()
        .map(|entry| entry.build_update_tx(priority_fee_in_microlamports))
        .collect::<Vec<_>>();

    let submit_result = submit_transactions(
        client,
        update_transactions,
        keypair,
        retry_count,
        confirmation_time,
    )
    .await;

    submit_result.map_err(|e| e.into())
}

fn _gossip_data_uploaded(
    validator_history_map: &HashMap<Pubkey, ValidatorHistory>,
    vote_account: Pubkey,
    epoch: u64,
) -> bool {
    if let Some(validator_history) = validator_history_map.get(&vote_account) {
        if let Some(latest_entry) = validator_history.history.last() {
            return latest_entry.epoch == epoch as u16
                && latest_entry.ip != ValidatorHistoryEntry::default().ip
                && latest_entry.version.major != ValidatorHistoryEntry::default().version.major
                && latest_entry.client_type != ValidatorHistoryEntry::default().client_type;
        }
    }
    false
}

// CODE BELOW SLIGHTLY MODIFIED FROM
// solana_sdk/src/ed25519_instruction.rs

pub const PUBKEY_SERIALIZED_SIZE: usize = 32;
pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;
pub const SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 14;
// bytemuck requires structures to be aligned
pub const SIGNATURE_OFFSETS_START: usize = 2;
pub const DATA_START: usize = SIGNATURE_OFFSETS_SERIALIZED_SIZE + SIGNATURE_OFFSETS_START;

#[derive(Default, Debug, Copy, Clone, Zeroable, Pod, Eq, PartialEq)]
#[repr(C)]
pub struct Ed25519SignatureOffsets {
    signature_offset: u16,             // offset to ed25519 signature of 64 bytes
    signature_instruction_index: u16,  // instruction index to find signature
    public_key_offset: u16,            // offset to public key of 32 bytes
    public_key_instruction_index: u16, // instruction index to find public key
    message_data_offset: u16,          // offset to start of message data
    message_data_size: u16,            // size of message data
    message_instruction_index: u16,    // index of instruction data to get message data
}

// This code is modified from solana_sdk/src/ed25519_instruction.rs
// due to that function requiring a keypair, and generating the signature within the function.
// In our case we don't have the keypair, we just have the signature and pubkey.
pub fn build_verify_signature_ix(
    signature: &[u8],
    pubkey: [u8; 32],
    message: &[u8],
) -> Instruction {
    assert_eq!(pubkey.len(), PUBKEY_SERIALIZED_SIZE);
    assert_eq!(signature.len(), SIGNATURE_SERIALIZED_SIZE);

    let mut instruction_data = Vec::with_capacity(
        DATA_START
            .saturating_add(SIGNATURE_SERIALIZED_SIZE)
            .saturating_add(PUBKEY_SERIALIZED_SIZE)
            .saturating_add(message.len()),
    );

    let num_signatures: u8 = 1;
    let public_key_offset = DATA_START;
    let signature_offset = public_key_offset.saturating_add(PUBKEY_SERIALIZED_SIZE);
    let message_data_offset = signature_offset.saturating_add(SIGNATURE_SERIALIZED_SIZE);

    // add padding byte so that offset structure is aligned
    instruction_data.extend_from_slice(bytes_of(&[num_signatures, 0]));

    let offsets = Ed25519SignatureOffsets {
        signature_offset: signature_offset as u16,
        signature_instruction_index: u16::MAX,
        public_key_offset: public_key_offset as u16,
        public_key_instruction_index: u16::MAX,
        message_data_offset: message_data_offset as u16,
        message_data_size: message.len() as u16,
        message_instruction_index: u16::MAX,
    };

    instruction_data.extend_from_slice(bytes_of(&offsets));

    debug_assert_eq!(instruction_data.len(), public_key_offset);

    instruction_data.extend_from_slice(&pubkey);

    debug_assert_eq!(instruction_data.len(), signature_offset);

    instruction_data.extend_from_slice(signature);

    debug_assert_eq!(instruction_data.len(), message_data_offset);

    instruction_data.extend_from_slice(message);

    Instruction {
        program_id: solana_program::ed25519_program::id(),
        accounts: vec![],
        data: instruction_data,
    }
}
