use anchor_lang::{prelude::*, solana_program::vote};

use crate::{
    errors::ValidatorHistoryError,
    state::{Config, ValidatorHistory},
    utils::{cast_epoch, fixed_point_sol},
    ValidatorHistoryEntry,
};

use jito_priority_fee_distribution::{
    state::PriorityFeeDistributionAccount, ID as PRIORITY_FEE_DIST_PROGRAM_ID,
};

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
    /// `owner = PRIORITY_FEE_DIST_PROGRAM_ID` here is sufficient.
    #[account(
        seeds = [
            PriorityFeeDistributionAccount::SEED,
            vote_account.key().as_ref(),
            epoch.to_le_bytes().as_ref(),
        ],
        bump,
        seeds::program = PRIORITY_FEE_DIST_PROGRAM_ID,
        owner = PRIORITY_FEE_DIST_PROGRAM_ID
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
    let epoch = cast_epoch(epoch)?;
    let mut validator_history_account = ctx.accounts.validator_history_account.load_mut()?;

    let mut tda_data: &[u8] = &ctx.accounts.distribution_account.try_borrow_data()?;

    let distribution_account = PriorityFeeDistributionAccount::try_deserialize(&mut tda_data)?;
    let commission_bps = distribution_account.validator_commission_bps;

    // if the merkle_root has been uploaded pull the mev_earned for the epoch
    let priority_fees_earned = if let Some(merkle_root) = distribution_account.merkle_root {
        fixed_point_sol(merkle_root.max_total_claim)
    } else {
        ValidatorHistoryEntry::default().priority_fees_earned
    };

    validator_history_account.set_priority_fee_commission(epoch, commission_bps, priority_fees_earned)?;

    Ok(())
}
