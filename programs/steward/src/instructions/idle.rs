use anchor_lang::prelude::*;

use crate::{
    errors::StewardError,
    maybe_transition_and_emit,
    utils::{deserialize_stake_pool, get_stake_pool_address, get_validator_list_length},
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

    /// CHECK: Correct account guaranteed if address is correct
    #[account(address = deserialize_stake_pool(&stake_pool)?.validator_list)]
    pub validator_list: AccountInfo<'info>,

    /// CHECK: Correct account guaranteed if address is correct
    #[account(
        address = get_stake_pool_address(&config)?
    )]
    pub stake_pool: AccountInfo<'info>,
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
        // CHECKS
        require!(
            clock.epoch == state_account.state.current_epoch,
            StewardError::EpochMaintenanceNotComplete
        );

        let validators_in_list = get_validator_list_length(&ctx.accounts.validator_list)?;
        require!(
            state_account.state.num_pool_validators as usize
                + state_account.state.validators_added as usize
                == validators_in_list,
            StewardError::ListStateMismatch
        );

        if config.is_paused() {
            return Err(StewardError::StateMachinePaused.into());
        }

        require!(
            matches!(state_account.state.state_tag, StewardStateEnum::Idle),
            StewardError::InvalidState
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
