use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLockReadGuard,
    },
    time::Duration,
};

use anchor_lang::{InstructionData, ToAccountMetas};
use bytemuck::{bytes_of, Pod, Zeroable};
use keeper_core::{
    get_multiple_accounts_batched, get_vote_accounts_with_retry, submit_transactions, Address,
    CreateTransaction, CreateUpdateStats,
};
use log::error;
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_response::RpcVoteAccountInfo};
use solana_gossip::{
    crds::Crds,
    crds_value::{CrdsData, CrdsValue, CrdsValueLabel},
};
use solana_metrics::datapoint_info;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signable, Signature},
    signer::Signer,
};
use tokio::time::sleep;
use validator_history::{
    self,
    constants::{MAX_ALLOC_BYTES, MIN_VOTE_EPOCHS},
    Config, ValidatorHistory, ValidatorHistoryEntry,
};

use crate::{get_validator_history_accounts_with_retry, start_spy_server};

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
        let (validator_history_account, _) = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, &vote_account.to_bytes()],
            program_id,
        );
        let (config, _) = Pubkey::find_program_address(&[Config::SEED], program_id);
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

impl CreateTransaction for GossipEntry {
    fn create_transaction(&self) -> Vec<Instruction> {
        let mut ixs = vec![Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::InitializeValidatorHistoryAccount {
                validator_history_account: self.validator_history_account,
                vote_account: self.vote_account,
                system_program: solana_program::system_program::id(),
                signer: self.signer,
            }
            .to_account_metas(None),
            data: validator_history::instruction::InitializeValidatorHistoryAccount {}.data(),
        }];
        let num_reallocs = (ValidatorHistory::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
        ixs.extend(vec![
            Instruction {
                program_id: self.program_id,
                accounts: validator_history::accounts::ReallocValidatorHistoryAccount {
                    validator_history_account: self.validator_history_account,
                    vote_account: self.vote_account,
                    config: self.config,
                    system_program: solana_program::system_program::id(),
                    signer: self.signer,
                }
                .to_account_metas(None),
                data: validator_history::instruction::ReallocValidatorHistoryAccount {}.data(),
            };
            num_reallocs
        ]);
        ixs
    }
}

impl GossipEntry {
    pub fn build_update_tx(&self) -> Vec<Instruction> {
        let mut ixs = vec![build_verify_signature_ix(
            self.signature.as_ref(),
            self.identity.to_bytes(),
            &self.message,
        )];

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

pub fn emit_gossip_datapoint(stats: CreateUpdateStats, runs_for_epoch: i64) {
    datapoint_info!(
        "gossip-upload-stats",
        ("num_creates_success", stats.creates.successes, i64),
        ("num_creates_error", stats.creates.errors, i64),
        ("num_updates_success", stats.updates.successes, i64),
        ("num_updates_error", stats.updates.errors, i64),
        ("runs_for_epoch", runs_for_epoch, i64),
    );
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
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    entrypoint: SocketAddr,
    program_id: &Pubkey,
) -> Result<CreateUpdateStats, Box<dyn std::error::Error>> {
    let gossip_port = 0;

    let spy_socket_addr = SocketAddr::new(
        IpAddr::from_str("0.0.0.0").expect("Invalid IP"),
        gossip_port,
    );
    let exit: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let (_gossip_service, cluster_info) =
        start_spy_server(entrypoint, gossip_port, spy_socket_addr, &keypair, &exit);

    let vote_accounts = get_vote_accounts_with_retry(&client, MIN_VOTE_EPOCHS, None).await?;
    let validator_history_accounts =
        get_validator_history_accounts_with_retry(&client, *program_id).await?;

    let validator_history_map = HashMap::from_iter(validator_history_accounts.iter().map(|vh| {
        (
            Pubkey::find_program_address(
                &[ValidatorHistory::SEED, &vh.vote_account.to_bytes()],
                program_id,
            )
            .0,
            vh,
        )
    }));

    // Wait for all active validators to be received
    sleep(Duration::from_secs(150)).await;

    let gossip_entries = {
        let crds = cluster_info.gossip.crds.read().map_err(|e| e.to_string())?;

        vote_accounts
            .iter()
            .filter_map(|vote_account| {
                let vote_account_pubkey = Pubkey::from_str(&vote_account.vote_pubkey).ok()?;
                let validator_history_account = validator_history_accounts
                    .iter()
                    .find(|account| account.vote_account == vote_account_pubkey)?;

                build_gossip_entry(
                    vote_account,
                    validator_history_account,
                    &crds,
                    *program_id,
                    &keypair,
                )
            })
            .flatten()
            .collect::<Vec<_>>()
    };

    exit.store(true, Ordering::Relaxed);

    let epoch = client.get_epoch_info().await?.epoch;

    let addresses = gossip_entries
        .iter()
        .filter_map(|a| {
            if gossip_data_uploaded(&validator_history_map, a.address(), epoch) {
                None
            } else {
                Some(a.address())
            }
        })
        .collect::<Vec<Pubkey>>();

    let existing_accounts_response = get_multiple_accounts_batched(&addresses, &client).await?;

    let create_transactions = existing_accounts_response
        .iter()
        .zip(gossip_entries.iter())
        .filter_map(|(existing_account, entry)| {
            if existing_account.is_none() {
                Some(entry.create_transaction())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let update_transactions = gossip_entries
        .iter()
        .map(|entry| entry.build_update_tx())
        .collect::<Vec<_>>();

    Ok(CreateUpdateStats {
        creates: submit_transactions(&client, create_transactions, &keypair).await?,
        updates: submit_transactions(&client, update_transactions, &keypair).await?,
    })
}

fn gossip_data_uploaded(
    validator_history_map: &HashMap<Pubkey, &ValidatorHistory>,
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
