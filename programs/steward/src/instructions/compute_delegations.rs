use crate::errors::StewardError;
use crate::utils::{deserialize_stake_pool, get_stake_pool_address, get_validator_list_length};
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
`compute_delegations` takes in the results from scoring and any other accounts that may affect a validator's delegation
It computes a share of the pool for each validator.
*/
pub fn handler(ctx: Context<ComputeDelegations>) -> Result<()> {
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
            matches!(
                state_account.state.state_tag,
                StewardStateEnum::ComputeDelegations
            ),
            StewardError::InvalidState
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
