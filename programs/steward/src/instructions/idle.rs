use anchor_lang::prelude::*;

use crate::{
    errors::StewardError, maybe_transition_and_emit, Config, StewardStateAccount, StewardStateEnum,
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
}

/*
Nothing to do at this state, just transition to the next state
*/
pub fn handler(ctx: Context<Idle>) -> Result<()> {
    let config = ctx.accounts.config.load()?;
    let mut state_account = ctx.accounts.state_account.load_mut()?;
    let clock = Clock::get()?;
    let epoch_schedule = EpochSchedule::get()?;

    {
        if config.is_paused() {
            return Err(StewardError::StateMachinePaused.into());
        }

        require!(
            matches!(state_account.state.state_tag, StewardStateEnum::Idle),
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

    maybe_transition_and_emit(
        &mut state_account.state,
        &clock,
        &config.parameters,
        &epoch_schedule,
    )?;

    Ok(())
}
