use crate::entries::gossip_entry::GossipEntry;
/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/
use crate::start_spy_server;
use crate::state::keeper_config::KeeperConfig;
use crate::state::keeper_state::KeeperState;
use bytemuck::{bytes_of, Pod, Zeroable};
use keeper_core::{submit_transactions, SubmitStats};
use log::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_response::RpcVoteAccountInfo;
use solana_gossip::crds::Crds;
use solana_gossip::crds_value::{CrdsData, CrdsValue, CrdsValueLabel};
use solana_metrics::datapoint_error;
use solana_sdk::signature::Signable;
use solana_sdk::{
    epoch_info::EpochInfo,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLockReadGuard;
use std::{collections::HashMap, net::SocketAddr, str::FromStr, sync::Arc, time::Duration};
use tokio::time::sleep;
use validator_history::ValidatorHistory;
use validator_history::ValidatorHistoryEntry;

use super::keeper_operations::KeeperOperations;

fn _get_operation() -> KeeperOperations {
    KeeperOperations::GossipUpload
}

fn _should_run(epoch_info: &EpochInfo, runs_for_epoch: u64) -> bool {
    // Run at 0%, 50% and 90% completion of epoch
    runs_for_epoch < 1
        || (epoch_info.slot_index > epoch_info.slots_in_epoch / 2 && runs_for_epoch < 2)
        || (epoch_info.slot_index > epoch_info.slots_in_epoch * 9 / 10 && runs_for_epoch < 3)
}

async fn _process(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    priority_fee_in_microlamports: u64,
    entrypoint: &SocketAddr,
    keeper_state: &KeeperState,
) -> Result<SubmitStats, Box<dyn std::error::Error>> {
    upload_gossip_values(
        client,
        keypair,
        program_id,
        priority_fee_in_microlamports,
        entrypoint,
        keeper_state,
    )
    .await
}

pub async fn fire(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64, u64) {
    let client = &keeper_config.client;
    let keypair = &keeper_config.keypair;
    let program_id = &keeper_config.program_id;
    let entrypoint = &keeper_config
        .gossip_entrypoint
        .expect("Entry point not set");

    let priority_fee_in_microlamports = keeper_config.priority_fee_in_microlamports;

    let operation = _get_operation();
    let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) =
        keeper_state.copy_runs_errors_and_txs_for_epoch(operation.clone());

    let should_run = _should_run(&keeper_state.epoch_info, runs_for_epoch);

    if should_run {
        match _process(
            client,
            keypair,
            program_id,
            priority_fee_in_microlamports,
            entrypoint,
            keeper_state,
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
        CrdsData::LegacyContactInfo(legacy_contact_info) => {
            if legacy_contact_info.wallclock() < validator_history.last_ip_timestamp {
                return false;
            }
        }
        CrdsData::LegacyVersion(legacy_version) => {
            if legacy_version.wallclock < validator_history.last_version_timestamp {
                return false;
            }
        }
        CrdsData::Version(version) => {
            if version.wallclock < validator_history.last_version_timestamp {
                return false;
            }
        }
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
    let legacy_contact_info_key: CrdsValueLabel =
        CrdsValueLabel::LegacyContactInfo(validator_identity);
    let version_key: CrdsValueLabel = CrdsValueLabel::Version(validator_identity);
    let legacy_version_key: CrdsValueLabel = CrdsValueLabel::LegacyVersion(validator_identity);

    // Current ContactInfo has both IP and Version, but LegacyContactInfo has only IP.
    // So if there is not ContactInfo, we need to submit tx for LegacyContactInfo + one of (Version, LegacyVersion)
    if let Some(entry) = crds.get::<&CrdsValue>(&contact_info_key) {
        if !check_entry_valid(entry, validator_history, validator_identity) {
            return None;
        }
        Some(vec![GossipEntry::new(
            &validator_vote_pubkey,
            &entry.get_signature(),
            &entry.signable_data(),
            &program_id,
            &entry.pubkey(),
            &keypair.pubkey(),
        )])
    } else {
        let mut entries = vec![];
        if let Some(entry) = crds.get::<&CrdsValue>(&legacy_contact_info_key) {
            if !check_entry_valid(entry, validator_history, validator_identity) {
                return None;
            }
            entries.push(GossipEntry::new(
                &validator_vote_pubkey,
                &entry.get_signature(),
                &entry.signable_data(),
                &program_id,
                &entry.pubkey(),
                &keypair.pubkey(),
            ))
        }

        if let Some(entry) = crds.get::<&CrdsValue>(&version_key) {
            if !check_entry_valid(entry, validator_history, validator_identity) {
                return None;
            }
            entries.push(GossipEntry::new(
                &validator_vote_pubkey,
                &entry.get_signature(),
                &entry.signable_data(),
                &program_id,
                &entry.pubkey(),
                &keypair.pubkey(),
            ))
        } else if let Some(entry) = crds.get::<&CrdsValue>(&legacy_version_key) {
            if !check_entry_valid(entry, validator_history, validator_identity) {
                return None;
            }
            entries.push(GossipEntry::new(
                &validator_vote_pubkey,
                &entry.get_signature(),
                &entry.signable_data(),
                &program_id,
                &entry.pubkey(),
                &keypair.pubkey(),
            ))
        }
        Some(entries)
    }
}

pub async fn upload_gossip_values(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    priority_fee_in_microlamports: u64,
    entrypoint: &SocketAddr,
    keeper_state: &KeeperState,
) -> Result<SubmitStats, Box<dyn std::error::Error>> {
    let vote_accounts = keeper_state.vote_account_map.values().collect::<Vec<_>>();
    let validator_history_map = &keeper_state.validator_history_map;

    let gossip_port = 0;

    let spy_socket_addr = SocketAddr::new(
        IpAddr::from_str("0.0.0.0").expect("Invalid IP"),
        gossip_port,
    );
    let exit: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let (_gossip_service, cluster_info) = start_spy_server(
        *entrypoint,
        gossip_port,
        spy_socket_addr,
        keypair,
        exit.clone(),
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

    exit.store(true, Ordering::Relaxed);

    let update_transactions = gossip_entries
        .iter()
        .map(|entry| entry.build_update_tx(priority_fee_in_microlamports))
        .collect::<Vec<_>>();

    let submit_result = submit_transactions(client, update_transactions, keypair).await;

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
