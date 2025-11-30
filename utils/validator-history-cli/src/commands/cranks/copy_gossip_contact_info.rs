use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        {Arc, RwLockReadGuard},
    },
    time::Duration,
};

use anchor_lang::{InstructionData, ToAccountMetas};
use bytemuck::{bytes_of, Pod, Zeroable};
use clap::Parser;
use log::info;
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_response::RpcVoteAccountInfo};
use solana_gossip::gossip_service::make_gossip_node;
use solana_gossip::{
    crds::Crds,
    crds_data::CrdsData,
    crds_value::{CrdsValue, CrdsValueLabel},
};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::Signature,
    signature::{read_keypair_file, Keypair, Signable, Signer},
};
use solana_streamer::socket::SocketAddrSpace;
use stakenet_sdk::{
    models::entries::Address,
    utils::{
        accounts::{
            get_all_validator_history_accounts, get_validator_history_address,
            get_validator_history_config_address,
        },
        transactions::{get_vote_accounts_with_retry, submit_transactions},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use validator_history::{ValidatorHistory, ValidatorHistoryEntry};

#[derive(Clone, Debug)]
pub struct GossipEntry {
    pub vote_account: Pubkey,
    pub validator_history_account: Pubkey,
    pub config: Pubkey,
    pub signature: Signature,
    pub message: Vec<u8>,
    pub program_id: Pubkey,
    pub identity: Pubkey,
    pub signer: Pubkey,
}

impl GossipEntry {
    pub fn new(
        vote_account: &Pubkey,
        signature: &Signature,
        message: &[u8],
        program_id: &Pubkey,
        identity: &Pubkey,
        signer: &Pubkey,
    ) -> Self {
        let validator_history_account = get_validator_history_address(vote_account, program_id);
        let config = get_validator_history_config_address(program_id);
        Self {
            vote_account: *vote_account,
            validator_history_account,
            config,
            signature: *signature,
            message: message.to_vec(),
            program_id: *program_id,
            identity: *identity,
            signer: *signer,
        }
    }
}

impl Address for GossipEntry {
    fn address(&self) -> Pubkey {
        self.validator_history_account
    }
}

impl GossipEntry {
    pub fn build_update_tx(&self, priority_fee: u64) -> Vec<Instruction> {
        let mut ixs = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(100_000),
            ComputeBudgetInstruction::set_compute_unit_price(priority_fee),
            build_verify_signature_ix(
                self.signature.as_ref(),
                self.identity.to_bytes(),
                &self.message,
            ),
        ];

        // info!("Ed25519 instruction data length: {}", ixs[0].data.len());

        ixs.push(Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::CopyGossipContactInfo {
                validator_history_account: self.validator_history_account,
                vote_account: self.vote_account,
                instructions: solana_program::sysvar::instructions::id(),
                config: self.config,
                oracle_authority: self.signer,
            }
            .to_account_metas(None),
            data: validator_history::instruction::CopyGossipContactInfo {}.data(),
        });
        ixs
    }
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

// Constants for the IP echo server protocol
// https://github.com/anza-xyz/agave/blob/master/net-utils/src/ip_echo_server.rs
// https://github.com/anza-xyz/agave/blob/master/net-utils/src/lib.rs
const IP_ECHO_HEADER_LEN: usize = 4;
const IP_ECHO_RESPONSE_LEN: usize = 27; // IP echo server will always respond with 27 bytes
const IP_ADDR_OFFSET_V4: usize = 8;
const SHRED_VERSION_OFFSET: usize = IP_ECHO_HEADER_LEN + IP_ADDR_OFFSET_V4;
// When joining the Gossip network, IpEchoServerMessage is set it its default value
// https://github.com/anza-xyz/agave/blob/master/net-utils/src/lib.rs#L60
const IP_ECHO_REQUEST: &[u8] = &[0x00; 21]; // IP echo server expects 21 bytes

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
        // error!(
        //     "Invalid gossip value retrieved for validator {}",
        //     validator_identity
        // );
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

#[derive(Parser)]
#[command(about = "Copy gossip contact info")]
pub struct CrankCopyGossipContactInfo {
    /// Path to oracle authority keypair
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Path to oracle authority keypair
    #[arg(short, long, env)]
    entrypoint: String,

    /// Path to oracle source CSV file
    #[arg(
        short,
        long,
        env,
        default_value = "/data/validator-age/oracle/data.csv"
    )]
    oracle_source: PathBuf,
}

pub async fn run(args: CrankCopyGossipContactInfo, client: Arc<RpcClient>) {
    // let vote_accounts = keeper_state.vote_account_map.values().collect::<Vec<_>>();
    let program_id = validator_history::id();
    let vote_accounts = get_vote_accounts_with_retry(&client, 5, None)
        .await
        .expect("msg");
    let entrypoint = solana_net_utils::parse_host_port(&args.entrypoint).unwrap_or_else(|err| {
        panic!(
            "Failed to parse gossip entrypoint '{}': {}",
            args.entrypoint, err
        )
    });
    let keypair = read_keypair_file(args.keypair_path).expect("Failed reading keypair file");
    let keypair = Arc::new(keypair);
    let validator_histories = get_all_validator_history_accounts(&client, program_id)
        .await
        .expect("Failed to read validator histories");
    let validator_history_map: HashMap<Pubkey, ValidatorHistory> = HashMap::from_iter(
        validator_histories
            .iter()
            .map(|vote_history| (vote_history.vote_account, *vote_history)),
    );

    // for entrypoint in entrypoints {
    // Modified from solana-gossip::main::process_spy and discover
    let exit: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let mut ip_echo_client = Ipv4EchoClient::new(entrypoint.to_string());

    let ip_echo_response = match ip_echo_client.fetch_ip_and_shred_version().await {
        Ok(res) => res,
        Err(e) => {
            panic!("Failed to fetch IP and shred version from gossip entrypoint: {entrypoint}, Error: {e}");
            // continue;
        }
    };

    let gossip_ip = ip_echo_response.ip;
    let cluster_shred_version = ip_echo_response.shred_version.unwrap_or(0);

    let gossip_addr = SocketAddr::new(
        gossip_ip,
        solana_net_utils::find_available_port_in_range(IpAddr::V4(Ipv4Addr::UNSPECIFIED), (0, 1))
            .expect("unable to find an available gossip port"),
    );

    let (_gossip_service, _ip_echo, cluster_info) = make_gossip_node(
        Keypair::from_base58_string(keypair.to_base58_string().as_str()),
        Some(&entrypoint),
        exit.clone(),
        Some(&gossip_addr),
        cluster_shred_version,
        true,
        SocketAddrSpace::Global,
    );

    info!("Gossip service started on {gossip_addr} with entrypoint {entrypoint}. Waiting for validators to be discovered...");

    // Wait for all active validators to be received
    tokio::time::sleep(Duration::from_secs(150)).await;

    let gossip_entries = {
        let crds = cluster_info
            .gossip
            .crds
            .read()
            .map_err(|e: std::sync::PoisonError<RwLockReadGuard<Crds>>| e.to_string())
            .expect("msg");

        vote_accounts
            .iter()
            .filter_map(|vote_account| {
                let vote_account_pubkey = Pubkey::from_str(&vote_account.vote_pubkey).ok()?;
                let validator_history_account = validator_history_map.get(&vote_account_pubkey)?;

                build_gossip_entry(
                    vote_account,
                    validator_history_account,
                    &crds,
                    program_id,
                    &keypair,
                )
            })
            .flatten()
            .collect::<Vec<_>>()
    };

    exit.store(true, Ordering::Relaxed);

    if gossip_entries.is_empty() {
        // continue;
    }

    let update_transactions = gossip_entries
        .iter()
        .map(|entry| entry.build_update_tx(0))
        .collect::<Vec<_>>();

    submit_transactions(&client, update_transactions, &keypair, 5, 5)
        .await
        .expect("msg");

    // return submit_result.map_err(|e| e.into());
    // }

    // Err("Failed to discover gossip entries from any of the provided entrypoints".into())
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

// pub const PUBKEY_SERIALIZED_SIZE: usize = 32;
// pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;
// pub const SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 14;
// // bytemuck requires structures to be aligned
// pub const SIGNATURE_OFFSETS_START: usize = 2;
// pub const DATA_START: usize = SIGNATURE_OFFSETS_SERIALIZED_SIZE + SIGNATURE_OFFSETS_START;
//
// #[derive(Default, Debug, Copy, Clone, Zeroable, Pod, Eq, PartialEq)]
// #[repr(C)]
// pub struct Ed25519SignatureOffsets {
//     signature_offset: u16,             // offset to ed25519 signature of 64 bytes
//     signature_instruction_index: u16,  // instruction index to find signature
//     public_key_offset: u16,            // offset to public key of 32 bytes
//     public_key_instruction_index: u16, // instruction index to find public key
//     message_data_offset: u16,          // offset to start of message data
//     message_data_size: u16,            // size of message data
//     message_instruction_index: u16,    // index of instruction data to get message data
// }
//
// // This code is modified from solana_sdk/src/ed25519_instruction.rs
// // due to that function requiring a keypair, and generating the signature within the function.
// // In our case we don't have the keypair, we just have the signature and pubkey.
// pub fn build_verify_signature_ix(
//     signature: &[u8],
//     pubkey: [u8; 32],
//     message: &[u8],
// ) -> Instruction {
//     assert_eq!(pubkey.len(), PUBKEY_SERIALIZED_SIZE);
//     assert_eq!(signature.len(), SIGNATURE_SERIALIZED_SIZE);
//
//     let mut instruction_data = Vec::with_capacity(
//         DATA_START
//             .saturating_add(SIGNATURE_SERIALIZED_SIZE)
//             .saturating_add(PUBKEY_SERIALIZED_SIZE)
//             .saturating_add(message.len()),
//     );
//
//     let num_signatures: u8 = 1;
//     let public_key_offset = DATA_START;
//     let signature_offset = public_key_offset.saturating_add(PUBKEY_SERIALIZED_SIZE);
//     let message_data_offset = signature_offset.saturating_add(SIGNATURE_SERIALIZED_SIZE);
//
//     // add padding byte so that offset structure is aligned
//     instruction_data.extend_from_slice(bytes_of(&[num_signatures, 0]));
//
//     let offsets = Ed25519SignatureOffsets {
//         signature_offset: signature_offset as u16,
//         signature_instruction_index: u16::MAX,
//         public_key_offset: public_key_offset as u16,
//         public_key_instruction_index: u16::MAX,
//         message_data_offset: message_data_offset as u16,
//         message_data_size: message.len() as u16,
//         message_instruction_index: u16::MAX,
//     };
//
//     instruction_data.extend_from_slice(bytes_of(&offsets));
//
//     debug_assert_eq!(instruction_data.len(), public_key_offset);
//
//     instruction_data.extend_from_slice(&pubkey);
//
//     debug_assert_eq!(instruction_data.len(), signature_offset);
//
//     instruction_data.extend_from_slice(signature);
//
//     debug_assert_eq!(instruction_data.len(), message_data_offset);
//
//     instruction_data.extend_from_slice(message);
//
//     Instruction {
//         program_id: solana_program::ed25519_program::id(),
//         accounts: vec![],
//         data: instruction_data,
//     }
// }
//
