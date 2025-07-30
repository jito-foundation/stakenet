use anchor_lang::{prelude::*, solana_program::vote};

use crate::{
    errors::ValidatorHistoryError,
    state::{Config, ValidatorHistory},
    utils::cast_epoch,
    MerkleRootUploadAuthority,
};

use jito_priority_fee_distribution::state::PriorityFeeDistributionAccount;

#[derive(Accounts)]
#[instruction(epoch: u64)]
pub struct CopyPriorityFeeDistribution<'info> {
    #[account(
        mut,
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        bump,
        has_one = vote_account
    )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,

    /// CHECK: Safe because we check the vote program is the owner before deserialization.
    /// Used to read validator commission.
    #[account(owner = vote::program::ID.key())]
    pub vote_account: AccountInfo<'info>,

    #[account(
        seeds = [Config::SEED],
        bump = config.bump,
    )]
    pub config: Account<'info, Config>,

    /// CHECK: Avoiding struct deserialization here to avoid default Owner trait check.
    /// `owner = config.priority_fee_distribution_program.key()` here is sufficient.
    #[account(
        seeds = [
            PriorityFeeDistributionAccount::SEED,
            vote_account.key().as_ref(),
            epoch.to_le_bytes().as_ref(),
        ],
        bump,
        seeds::program = config.priority_fee_distribution_program.key(),
    )]
    pub distribution_account: UncheckedAccount<'info>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handle_copy_priority_fee_distribution_account(
    ctx: Context<CopyPriorityFeeDistribution>,
    epoch: u64,
) -> Result<()> {
    // cant set data in validator history for future epochs
    if epoch > Clock::get()?.epoch {
        return Err(ValidatorHistoryError::EpochOutOfRange.into());
    }

    // The PFDA cannot be updated outside of its own epoch - this guarantees the immutability of the data we copy
    if epoch == Clock::get()?.epoch {
        return Err(ValidatorHistoryError::PriorityFeeDistributionAccountNotFinalized.into());
    }

    let epoch = cast_epoch(epoch)?;
    let mut validator_history_account = ctx.accounts.validator_history_account.load_mut()?;

    let validator_history_entry_for_epoch = validator_history_account
        .history
        .arr_mut()
        .iter_mut()
        .find(|entry| entry.epoch == epoch);

    // This ensures there is no possibility to overwrite a validator history entry after the PFDA
    // rent has been reclaimed and introduce an erroneous unstake.
    if let Some(entry) = validator_history_entry_for_epoch {
        if entry.priority_fee_merkle_root_upload_authority != MerkleRootUploadAuthority::Unset {
            return Err(ValidatorHistoryError::PriorityFeeDistributionAccountAlreadyCopied.into());
        }
    }

    let mut pdfa_data: &[u8] = &ctx.accounts.distribution_account.try_borrow_data()?;

    let distribution_account =
        PriorityFeeDistributionAccount::try_deserialize(&mut pdfa_data).unwrap_or_default();
    // If the distribution account is not found, we set the default values of 0 for the commission and priority fees earned
    let commission_bps = distribution_account.validator_commission_bps;
    let priority_fees_earned = distribution_account.total_lamports_transferred;
    // If the distribution account is not found, we set '11111111111111111111111111111111' as the merkle root upload authority
    // passing this to MerkleRootUploadAuthority::from_pubkey resolve to a DNE authority
    let merkle_root_upload_authority = distribution_account.merkle_root_upload_authority;

    validator_history_account.set_priority_fees_earned_and_commission(
        epoch,
        commission_bps,
        priority_fees_earned,
        MerkleRootUploadAuthority::from_pubkey(&merkle_root_upload_authority),
    )?;

    Ok(())
}
