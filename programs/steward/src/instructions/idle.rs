use anchor_lang::prelude::*;

use crate::{
    maybe_transition_and_emit,
    utils::{get_validator_list, state_checks},
    Config, StewardStateAccount, StewardStateEnum,
};

#[derive(Accounts)]
pub struct Idle<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,

    /// CHECK: account type checked in state_checks and address set in config
    #[account(address = get_validator_list(&config)?)]
    pub validator_list: AccountInfo<'info>,
}

/*
Nothing to do at this state, just transition to the next state
*/
pub fn handler(ctx: Context<Idle>) -> Result<()> {
    let config = ctx.accounts.config.load()?;
    let mut state_account = ctx.accounts.state_account.load_mut()?;
    let clock = Clock::get()?;
    let epoch_schedule = EpochSchedule::get()?;

    state_checks(
        &clock,
        &config,
        &state_account,
        &ctx.accounts.validator_list,
        Some(StewardStateEnum::Idle),
    )?;

    maybe_transition_and_emit(
        &mut state_account.state,
        &clock,
        &config.parameters,
        &epoch_schedule,
    )?;

    Ok(())
}
