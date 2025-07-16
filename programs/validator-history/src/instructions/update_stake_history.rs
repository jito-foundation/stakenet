use crate::{
    errors::ValidatorHistoryError,
    state::{Config, ValidatorHistory},
    utils::cast_epoch,
    ValidatorStakeBuffer,
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

    #[account(
        mut,
        seeds = [ValidatorStakeBuffer::SEED],
        bump
    )]
    pub validator_stake_buffer_account: AccountLoader<'info, ValidatorStakeBuffer>,

    /// CHECK: fine since we are not deserializing account
    #[account(owner = vote::program::ID.key())]
    pub vote_account: AccountInfo<'info>,

    #[account(
        seeds = [Config::SEED],
        bump = config.bump,
    )]
    pub config: Account<'info, Config>,
}

// TODO: backfilling?
pub fn handle_update_stake_history(ctx: Context<UpdateStakeHistory>) -> Result<()> {
    // Load accounts
    let mut validator_history_account: std::cell::RefMut<'_, ValidatorHistory> =
        ctx.accounts.validator_history_account.load_mut()?;
    let validator_stake_buffer_account = ctx.accounts.validator_stake_buffer_account.load()?;

    // Assert that the stake buffer is finalized for the current epoch
    let epoch = Clock::get()?.epoch;
    if validator_stake_buffer_account
        .last_observed_epoch()
        .ne(&epoch)
    {
        return Err(ValidatorHistoryError::EpochOutOfRange.into());
    }
    if !validator_stake_buffer_account.is_finalized() {
        return Err(ValidatorHistoryError::StakeBufferNotFinalized.into());
    }

    // Get validator rank
    let validator_id = validator_history_account.index;
    let (lamports, rank, is_superminority) =
        validator_stake_buffer_account.get_by_id(validator_id)?;

    // Persist
    let epoch = cast_epoch(epoch)?;
    validator_history_account.set_stake(epoch, lamports, rank, is_superminority)?;

    Ok(())
}
