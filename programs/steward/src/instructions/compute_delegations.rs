use crate::errors::StewardError;
use crate::{maybe_transition_and_emit, Config, StewardStateAccount, StewardStateEnum};
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

    {
        if config.is_paused() {
            return Err(StewardError::StateMachinePaused.into());
        }

        require!(
            matches!(
                state_account.state.state_tag,
                StewardStateEnum::ComputeDelegations
            ),
            StewardError::InvalidState
        );

        require!(
            clock.epoch == state_account.state.current_epoch,
            StewardError::EpochMaintenanceNotComplete
        );

        require!(
            state_account.state.validators_for_immediate_removal.count() == 0,
            StewardError::ValidatorsNeedToBeRemoved
        );
    }

    state_account
        .state
        .compute_delegations(clock.epoch, &config)?;

    if let Some(event) = maybe_transition_and_emit(
        &mut state_account.state,
        &clock,
        &config.parameters,
        &epoch_schedule,
    )? {
        emit!(event);
    }

    Ok(())
}
