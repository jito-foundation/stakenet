use anchor_lang::{
    prelude::*,
    solana_program::{self, clock::Clock, pubkey::Pubkey, sysvar},
};

use crate::{
    crds_value::CrdsData, errors::ValidatorHistoryError, state::ValidatorHistory, utils::cast_epoch,
};
use validator_history_vote_state::VoteStateVersions;

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
    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handler(ctx: Context<CopyGossipContactInfo>) -> Result<()> {
    let mut validator_history_account = ctx.accounts.validator_history_account.load_mut()?;
    let instructions = ctx.accounts.instructions.to_account_info();
    let clock = Clock::get()?;
    let epoch = cast_epoch(clock.epoch);

    let verify_instruction = sysvar::instructions::get_instruction_relative(-1, &instructions)?;

    // Check that the instruction is a ed25519 instruction
    if verify_instruction.program_id != solana_program::ed25519_program::ID {
        return Err(ValidatorHistoryError::NotSigVerified.into());
    }

    let message_signer = Pubkey::try_from(&verify_instruction.data[16..48])
        .map_err(|_| ValidatorHistoryError::GossipDataInvalid)?;
    let message_data = &verify_instruction.data[112..];

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
