use crate::{
    errors::ValidatorHistoryError,
    state::{Config, ValidatorHistory},
    utils::cast_epoch,
};
use anchor_lang::{prelude::*, solana_program::vote};

#[derive(Accounts)]
pub struct UpdateStakeHistory<'info> {
    #[account(
        mut,
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        bump
    )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,

    /// CHECK: fine since we are not deserializing account
    #[account(owner = vote::program::ID.key())]
    pub vote_account: AccountInfo<'info>,

    #[account(
        seeds = [Config::SEED],
        bump = config.bump,
        has_one = oracle_authority
    )]
    pub config: Account<'info, Config>,

    #[account(mut)]
    pub oracle_authority: Signer<'info>,
}

// NOTE: If using this instruction to backfill a new validator history account, you must ensure that epochs are added in ascending order.
// This is because new entries cannot be inserted for an epoch that is lower than the last entry's if missed.
pub fn handle_update_stake_history(
    ctx: Context<UpdateStakeHistory>,
    epoch: u64,
    lamports: u64,
    rank: u32,
    is_superminority: bool,
) -> Result<()> {
    let mut validator_history_account: std::cell::RefMut<'_, ValidatorHistory> =
        ctx.accounts.validator_history_account.load_mut()?;

    // Cannot set stake for future epochs
    if epoch > Clock::get()?.epoch {
        return Err(ValidatorHistoryError::EpochOutOfRange.into());
    }
    let epoch = cast_epoch(epoch)?;

    validator_history_account.set_stake(epoch, lamports, rank, is_superminority)?;

    Ok(())
}
