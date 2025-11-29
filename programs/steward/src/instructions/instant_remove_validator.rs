use crate::{
    errors::StewardError,
    stake_pool_utils::deserialize_stake_pool,
    utils::{
        get_stake_pool_address, get_validator_list, get_validator_list_length, tally_stake_status,
    },
    Config, DirectedStakeMeta, StewardStateAccount, StewardStateAccountV2,
};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct InstantRemoveValidator<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccountV2>,

    /// CHECK: Correct account guaranteed if address is correct
    #[account(address = get_validator_list(&config)?)]
    pub validator_list: AccountInfo<'info>,

    /// CHECK: Correct account guaranteed if address is correct
    #[account(
        address = get_stake_pool_address(&config)?
    )]
    pub stake_pool: AccountInfo<'info>,

    #[account(
        mut,
        seeds = [DirectedStakeMeta::SEED, config.key().as_ref()],
        bump
    )]
    pub directed_stake_meta: AccountLoader<'info, DirectedStakeMeta>,
}

/// Removes validators from the pool that have been marked for immediate removal
pub fn handler(
    ctx: Context<InstantRemoveValidator>,
    validator_index_to_remove: usize,
) -> Result<()> {
    let stake_pool = deserialize_stake_pool(&ctx.accounts.stake_pool)?;
    let mut state_account = ctx.accounts.state_account.load_mut()?;
    let mut directed_stake_meta = ctx.accounts.directed_stake_meta.load_mut()?;

    let clock = Clock::get()?;
    let validators_for_immediate_removal =
        state_account.state.validators_for_immediate_removal.count();
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

    let stake_status_tally = tally_stake_status(&ctx.accounts.validator_list)?;

    let total_deactivating = stake_status_tally.deactivating_all
        + stake_status_tally.deactivating_transient
        + stake_status_tally.deactivating_validator
        + stake_status_tally.ready_for_removal;

    require!(
        total_deactivating == state_account.state.validators_to_remove.count() as u64,
        StewardError::ValidatorsHaveNotBeenRemoved
    );

    require!(
        stake_status_tally.ready_for_removal == 0,
        StewardError::ValidatorsNeedToBeRemoved
    );

    require!(
        state_account.state.num_pool_validators as usize
            + state_account.state.validators_added as usize
            - validators_for_immediate_removal
            == validators_in_list,
        StewardError::ListStateMismatch
    );

    state_account
        .state
        .remove_validator(validator_index_to_remove, &mut directed_stake_meta)?;

    Ok(())
}
