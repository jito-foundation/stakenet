use anchor_lang::{
    prelude::*,
    solana_program::{self, clock::Clock, pubkey::Pubkey, sysvar},
};
use bytemuck::{from_bytes, Pod, Zeroable};

use crate::{
    crds_value::CrdsData, errors::ValidatorHistoryError, state::ValidatorHistory,
    utils::cast_epoch, Config,
};
use validator_history_vote_state::VoteStateVersions;

// Structs and constants copied from solana_sdk::ed25519_instruction. Copied in order to make fields public. Compilation issues hit when importing solana_sdk
#[derive(Default, Debug, Copy, Clone, Zeroable, Pod, Eq, PartialEq)]
#[repr(C)]
pub struct Ed25519SignatureOffsets {
    pub signature_offset: u16, // offset to ed25519 signature of 64 bytes
    pub signature_instruction_index: u16, // instruction index to find signature
    pub public_key_offset: u16, // offset to public key of 32 bytes
    pub public_key_instruction_index: u16, // instruction index to find public key
    pub message_data_offset: u16, // offset to start of message data
    pub message_data_size: u16, // size of message data
    pub message_instruction_index: u16, // index of instruction data to get message data
}

pub const PUBKEY_SERIALIZED_SIZE: usize = 32;
pub const SIGNATURE_SERIALIZED_SIZE: usize = 64;
pub const SIGNATURE_OFFSETS_SERIALIZED_SIZE: usize = 14;
// bytemuck requires structures to be aligned
pub const SIGNATURE_OFFSETS_START: usize = 2;
pub const DATA_START: usize = SIGNATURE_OFFSETS_SERIALIZED_SIZE + SIGNATURE_OFFSETS_START;

#[derive(Accounts)]
pub struct CopyGossipContactInfo<'info> {
    #[account(
        mut,
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        bump,
    )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,
    /// CHECK: Safe because we check the vote program is the owner.
    #[account(owner = solana_program::vote::program::ID.key())]
    pub vote_account: AccountInfo<'info>,
    /// CHECK: Safe because it's a sysvar account
    #[account(address = sysvar::instructions::ID)]
    pub instructions: UncheckedAccount<'info>,
    #[account(
        seeds = [Config::SEED],
        bump = config.bump,
        has_one = oracle_authority
    )]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub oracle_authority: Signer<'info>,
}

pub fn handle_copy_gossip_contact_info(ctx: Context<CopyGossipContactInfo>) -> Result<()> {
    let mut validator_history_account = ctx.accounts.validator_history_account.load_mut()?;
    let instructions = ctx.accounts.instructions.to_account_info();
    let clock = Clock::get()?;
    let epoch = cast_epoch(clock.epoch)?;

    let verify_instruction = sysvar::instructions::get_instruction_relative(-1, &instructions)?;

    // Check that the instruction is a ed25519 instruction
    if verify_instruction.program_id != solana_program::ed25519_program::ID {
        return Err(ValidatorHistoryError::NotSigVerified.into());
    }

    let ed25519_offsets = from_bytes::<Ed25519SignatureOffsets>(
        &verify_instruction.data
            [SIGNATURE_OFFSETS_START..SIGNATURE_OFFSETS_START + SIGNATURE_OFFSETS_SERIALIZED_SIZE],
    );

    // Check offsets and indices are correct so an attacker cannot submit invalid data
    if ed25519_offsets.signature_instruction_index != ed25519_offsets.public_key_instruction_index
        || ed25519_offsets.signature_instruction_index != ed25519_offsets.message_instruction_index
        || ed25519_offsets.public_key_offset
            != (SIGNATURE_OFFSETS_START + SIGNATURE_OFFSETS_SERIALIZED_SIZE) as u16
        || ed25519_offsets.signature_offset
            != ed25519_offsets.public_key_offset + PUBKEY_SERIALIZED_SIZE as u16
        || ed25519_offsets.message_data_offset
            != ed25519_offsets.signature_offset + SIGNATURE_SERIALIZED_SIZE as u16
    {
        return Err(ValidatorHistoryError::GossipDataInvalid.into());
    }

    let message_signer = Pubkey::try_from(
        &verify_instruction.data[ed25519_offsets.public_key_offset as usize
            ..ed25519_offsets.public_key_offset as usize + PUBKEY_SERIALIZED_SIZE],
    )
    .map_err(|_| ValidatorHistoryError::GossipDataInvalid)?;
    let message_data = &verify_instruction.data[ed25519_offsets.message_data_offset as usize..];

    let crds_data: CrdsData =
        bincode::deserialize(message_data).map_err(|_| ValidatorHistoryError::GossipDataInvalid)?;

    let (crds_data_pubkey, last_signed_ts) = match &crds_data {
        CrdsData::LegacyContactInfo(contact_info) => {
            (*contact_info.pubkey(), contact_info.wallclock())
        }
        CrdsData::ContactInfo(contact_info) => (*contact_info.pubkey(), contact_info.wallclock()),
        CrdsData::Version(version) => (version.from, version.wallclock),
        CrdsData::LegacyVersion(version) => (version.from, version.wallclock),
        _ => {
            return Err(ValidatorHistoryError::GossipDataInvalid.into());
        }
    };

    let node_pubkey = VoteStateVersions::deserialize_node_pubkey(&ctx.accounts.vote_account)?;
    // The gossip signature signer, the ContactInfo struct, and the vote account identity
    // must all reference the same address
    if crds_data_pubkey != node_pubkey || message_signer != node_pubkey {
        return Err(ValidatorHistoryError::GossipDataInvalid.into());
    }

    // Timestamp can't be too far in the future or this upload will be stuck. Allows 10 minutes of buffer.
    // last_signed_ts is in ms, clock.unix_timestamp is in seconds
    if last_signed_ts / 1000
        > clock
            .unix_timestamp
            .checked_add(600)
            .ok_or(ValidatorHistoryError::ArithmeticError)? as u64
    {
        return Err(ValidatorHistoryError::GossipDataInFuture.into());
    }

    // Set gossip values
    match crds_data {
        CrdsData::LegacyContactInfo(legacy_contact_info) => {
            validator_history_account.set_legacy_contact_info(
                epoch,
                &legacy_contact_info,
                last_signed_ts,
            )?;
        }
        CrdsData::ContactInfo(contact_info) => {
            validator_history_account.set_contact_info(epoch, &contact_info, last_signed_ts)?;
        }
        CrdsData::Version(version) => {
            validator_history_account.set_version(epoch, &version, last_signed_ts)?;
        }
        CrdsData::LegacyVersion(legacy_version) => {
            validator_history_account.set_legacy_version(epoch, &legacy_version, last_signed_ts)?;
        }
        _ => {
            return Err(ValidatorHistoryError::GossipDataInvalid.into());
        }
    }

    Ok(())
}
