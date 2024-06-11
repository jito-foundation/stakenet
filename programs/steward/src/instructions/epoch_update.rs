use crate::{Config, StewardStateAccount};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct EpochUpdate<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,
}

// Removes validator from blacklist. Validator will be eligible to receive delegation again when scores are recomputed.
// Index is the index of the validator from ValidatorHistory.
pub fn handler(ctx: Context<EpochUpdate>) -> Result<()> {
    let mut state_account = ctx.accounts.state_account.load_mut()?;

    let clock = Clock::get()?;
    state_account.state.current_epoch = clock.epoch;

    //TODO go through and remove delinquent validators
    // state_account.state.remove_validator(validator_list_index)?;

    Ok(())
}
