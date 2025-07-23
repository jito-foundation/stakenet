use anchor_lang::prelude::*;

use crate::{
    errors::ValidatorHistoryError, utils::cast_epoch, Config, ValidatorHistory,
    ValidatorStakeBuffer,
};

// TODO: If we maintain the permissioned verion alongside this one (no oracle), anyone can
// overwrite the oracle
#[derive(Accounts)]
pub struct CopyStakeInfo<'info> {
    #[account(
        mut,
        owner = crate::id()
    )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,

    #[account(
        seeds = [Config::SEED],
        bump = config.bump,
    )]
    pub config: Account<'info, Config>,

    #[account(
        seeds = [ValidatorStakeBuffer::SEED],
        bump
    )]
    pub validator_stake_buffer_account: AccountLoader<'info, ValidatorStakeBuffer>,
}

pub fn handle_copy_stake_info(ctx: Context<CopyStakeInfo>) -> Result<()> {
    // Read an arbitrary validator history account
    //
    // No further validations required as we are simply reading an account that has already been
    // created and allocated by this program
    let mut validator_history_account = ctx.accounts.validator_history_account.load_mut()?;

    // Assert that we are observing vaidator stake buffer in current epoch
    let epoch = Clock::get()?.epoch;
    let validator_stake_buffer_account = ctx.accounts.validator_stake_buffer_account.load()?;
    if epoch != validator_stake_buffer_account.last_observed_epoch() {
        return Err(ValidatorHistoryError::EpochOutOfRange.into());
    }

    // Assert stake buffer is finalized
    if !validator_stake_buffer_account.is_finalized() {
        return Err(ValidatorHistoryError::StakeBufferNotFinalized.into());
    }

    // Look up stake info in buffer
    let (stake, rank, is_superminority) =
        validator_stake_buffer_account.get_by_id(validator_history_account.index)?;

    // Insert and persit stake info in validator history account
    let epoch = cast_epoch(epoch)?;
    validator_history_account.set_stake(epoch, stake, rank, is_superminority)?;

    Ok(())
}
