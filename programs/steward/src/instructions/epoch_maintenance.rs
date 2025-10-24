use crate::{
    errors::StewardError,
    events::EpochMaintenanceEvent,
    stake_pool_utils::deserialize_stake_pool,
    utils::{
        check_validator_list_has_stake_status_other_than, get_stake_pool_address,
        get_validator_list, get_validator_list_length,
    },
    Config, StewardStateAccount, COMPUTE_INSTANT_UNSTAKES, EPOCH_MAINTENANCE, POST_LOOP_IDLE,
    PRE_LOOP_IDLE, REBALANCE, REBALANCE_DIRECTED_COMPLETE, RESET_TO_IDLE,
};
use anchor_lang::prelude::*;
use spl_stake_pool::state::StakeStatus;

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
    #[account(address = get_validator_list(&config)?)]
    pub validator_list: AccountInfo<'info>,

    /// CHECK: Correct account guaranteed if address is correct
    #[account(
        address = get_stake_pool_address(&config)?
    )]
    pub stake_pool: AccountInfo<'info>,
}

/// Runs maintenance tasks at the start of each epoch, needs to be run multiple times
/// Routines:
/// - Remove delinquent validators
pub fn handler(
    ctx: Context<EpochMaintenance>,
    validator_index_to_remove: Option<usize>,
) -> Result<()> {
    let stake_pool = deserialize_stake_pool(&ctx.accounts.stake_pool)?;
    let mut state_account = ctx.accounts.state_account.load_mut()?;

    let clock = Clock::get()?;

    require!(
        clock.epoch == stake_pool.last_update_epoch,
        StewardError::StakePoolNotUpdated
    );

    require!(
        state_account.state.current_epoch < clock.epoch,
        StewardError::EpochMaintenanceAlreadyComplete
    );

    // Ensure there are no validators in the list that have not been removed, that should be
    require!(
        !check_validator_list_has_stake_status_other_than(
            &ctx.accounts.validator_list,
            &[StakeStatus::Active]
        )?,
        StewardError::ValidatorsHaveNotBeenRemoved
    );

    state_account.state.unset_flag(EPOCH_MAINTENANCE);

    {
        // Routine - Remove marked validators
        // We still want these checks to run even if we don't specify a validator to remove
        let validators_in_list = get_validator_list_length(&ctx.accounts.validator_list)?;
        let validators_to_remove = state_account.state.validators_to_remove.count()
            + state_account.state.validators_for_immediate_removal.count();

        // Ensure we have a 1-1 mapping between the number of validators in the list and the number of validators in the state
        // If we don't have this mapping, everything needs to be removed
        require!(
            state_account.state.num_pool_validators as usize
                + state_account.state.validators_added as usize
                - validators_to_remove
                == validators_in_list,
            StewardError::ListStateMismatch
        );
        if let Some(validator_index_to_remove) = validator_index_to_remove {
            state_account
                .state
                .remove_validator(validator_index_to_remove)?;
        }
    }

    {
        // Routine - Update state
        let okay_to_update = state_account.state.validators_to_remove.is_empty()
            && state_account
                .state
                .validators_for_immediate_removal
                .is_empty();

        if okay_to_update {
            state_account.state.current_epoch = clock.epoch;

            // We keep Compute Scores and Compute Delegations to be unset on next epoch cycle
            state_account.state.unset_flag(
                PRE_LOOP_IDLE
                    | COMPUTE_INSTANT_UNSTAKES
                    | REBALANCE
                    | POST_LOOP_IDLE
                    | REBALANCE_DIRECTED_COMPLETE,
            );
            state_account
                .state
                .set_flag(RESET_TO_IDLE | EPOCH_MAINTENANCE);
        }
        emit!(EpochMaintenanceEvent {
            validator_index_to_remove: validator_index_to_remove.map(|x| x as u64),
            validator_list_length: get_validator_list_length(&ctx.accounts.validator_list)? as u64,
            num_pool_validators: state_account.state.num_pool_validators,
            validators_to_remove: state_account.state.validators_to_remove.count() as u64,
            validators_to_add: state_account.state.validators_added as u64,
            maintenance_complete: okay_to_update,
        });
    }

    Ok(())
}
