use crate::{
    errors::StewardError,
    events::EpochMaintenanceEvent,
    utils::{deserialize_stake_pool, get_stake_pool_address, get_validator_list_length},
    Config, StewardStateAccount, CHECKED_VALIDATORS_REMOVED_FROM_LIST, COMPUTE_INSTANT_UNSTAKES,
    EPOCH_MAINTENANCE, POST_LOOP_IDLE, PRE_LOOP_IDLE, REBALANCE, RESET_TO_IDLE,
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct EpochMaintenance<'info> {
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

/// Resets Status Flags and updates the epoch
/// only runs once per epoch before anything else
pub fn handler(ctx: Context<EpochMaintenance>) -> Result<()> {
    let stake_pool = deserialize_stake_pool(&ctx.accounts.stake_pool)?;
    let mut state_account = ctx.accounts.state_account.load_mut()?;

    let clock = Clock::get()?;

    {
        // CHECKS
        require!(
            clock.epoch != state_account.state.current_epoch,
            StewardError::EpochMaintenanceAlreadyCompleted
        );

        require!(
            clock.epoch == stake_pool.last_update_epoch,
            StewardError::StakePoolNotUpdated
        );

        let validators_in_list = get_validator_list_length(&ctx.accounts.validator_list)?;
        require!(
            state_account.state.num_pool_validators as usize
                + state_account.state.validators_added as usize
                == validators_in_list,
            StewardError::ListStateMismatch
        );
    }

    {
        // UPDATES
        state_account.state.current_epoch = clock.epoch;

        // We keep Compute Scores and Compute Delegations to be unset on next epoch cycle
        state_account.state.unset_flag(
            CHECKED_VALIDATORS_REMOVED_FROM_LIST
                | PRE_LOOP_IDLE
                | COMPUTE_INSTANT_UNSTAKES
                | REBALANCE
                | POST_LOOP_IDLE,
        );
        state_account
            .state
            .set_flag(RESET_TO_IDLE | EPOCH_MAINTENANCE);

        emit!(EpochMaintenanceEvent {
            maintenance_complete: true,
        });
    }

    Ok(())
}
