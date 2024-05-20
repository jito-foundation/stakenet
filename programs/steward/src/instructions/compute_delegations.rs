use crate::errors::StewardError;
use crate::{maybe_transition_and_emit, Config, StewardStateAccount};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct ComputeDelegations<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

/*
`compute_delegations` takes in the results from scoring and any other accounts that may affect a validator's delegation
It computes a share of the pool for each validator.
*/
pub fn handler(ctx: Context<ComputeDelegations>) -> Result<()> {
    let config = ctx.accounts.config.load()?;
    let mut state_account = ctx.accounts.state_account.load_mut()?;

    let clock = Clock::get()?;
    let epoch_schedule = EpochSchedule::get()?;

    if config.is_paused() {
        return Err(StewardError::StateMachinePaused.into());
    }

    state_account
        .state
        .compute_delegations(clock.epoch, &config)?;

    maybe_transition_and_emit(
        &mut state_account.state,
        &clock,
        &config.parameters,
        &epoch_schedule,
    )?;

    Ok(())
}
