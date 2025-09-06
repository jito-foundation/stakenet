use crate::utils::{get_validator_list, state_checks};
use crate::{maybe_transition, Config, StewardStateAccount, StewardStateAccountV2, StewardStateEnum};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct ComputeDelegations<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccountV2>,

    /// CHECK: Account owner checked, account type checked in get_validator_stake_info_at_index
    #[account(address = get_validator_list(&config)?)]
    pub validator_list: AccountInfo<'info>,
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

    state_checks(
        &clock,
        &config,
        &state_account,
        &ctx.accounts.validator_list,
        Some(StewardStateEnum::ComputeDelegations),
    )?;

    state_account
        .state
        .compute_delegations(clock.epoch, &config)?;

    if let Some(event) = maybe_transition(
        &mut state_account.state,
        &clock,
        &config.parameters,
        &epoch_schedule,
    )? {
        emit!(event);
    }

    Ok(())
}
