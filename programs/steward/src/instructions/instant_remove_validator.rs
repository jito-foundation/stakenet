use crate::{
    errors::StewardError,
    utils::{
        check_validator_list_has_stake_status_other_than, deserialize_stake_pool,
        get_stake_pool_address, get_validator_list, get_validator_list_length,
    },
    Config, StewardStateAccount,
};
use anchor_lang::prelude::*;
use spl_stake_pool::state::StakeStatus;

#[derive(Accounts)]
pub struct InstantRemoveValidator<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,

    /// CHECK: Correct account guaranteed if address is correct
    #[account(address = get_validator_list(&config)?)]
    pub validator_list: AccountInfo<'info>,

    /// CHECK: Correct account guaranteed if address is correct
    #[account(
        address = get_stake_pool_address(&config)?
    )]
    pub stake_pool: AccountInfo<'info>,
}

/// Removes validators from the pool that have been marked for immediate removal
pub fn handler(
    ctx: Context<InstantRemoveValidator>,
    validator_index_to_remove: usize,
) -> Result<()> {
    let stake_pool = deserialize_stake_pool(&ctx.accounts.stake_pool)?;
    let mut state_account = ctx.accounts.state_account.load_mut()?;

    let clock = Clock::get()?;
    let validators_to_remove = state_account.state.validators_for_immediate_removal.count();
    let validators_in_list = get_validator_list_length(&ctx.accounts.validator_list)?;

    require!(
        state_account.state.current_epoch == clock.epoch,
        StewardError::EpochMaintenanceNotComplete
    );

    require!(
        clock.epoch == stake_pool.last_update_epoch,
        StewardError::StakePoolNotUpdated
    );

    require!(
        state_account
            .state
            .validators_for_immediate_removal
            .get(validator_index_to_remove)?,
        StewardError::ValidatorNotInList
    );

    require!(
        state_account.state.num_pool_validators as usize
            + state_account.state.validators_added as usize
            - validators_to_remove
            == validators_in_list,
        StewardError::ListStateMismatch
    );

    // Ensure there are no validators in the list that have not been removed, that should be
    require!(
        !check_validator_list_has_stake_status_other_than(
            &ctx.accounts.validator_list,
            &vec![
                StakeStatus::Active,
                StakeStatus::DeactivatingAll,
                StakeStatus::DeactivatingTransient
            ]
        )?,
        StewardError::ValidatorsHaveNotBeenRemoved
    );

    state_account
        .state
        .remove_validator(validator_index_to_remove)?;

    Ok(())
}
