use crate::{
    errors::StewardError,
    utils::{get_stake_pool, get_validator_list_length, StakePool},
    Config, StewardStateAccount,
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

    #[account(mut, address = stake_pool.validator_list)]
    pub validator_list: AccountInfo<'info>,

    #[account(
        address = get_stake_pool(&config)?
    )]
    pub stake_pool: Account<'info, StakePool>,
}

/// Runs maintenance tasks at the start of each epoch, needs to be run multiple times
/// Routines:
/// - Remove delinquent validators
pub fn handler(
    ctx: Context<EpochMaintenance>,
    validator_index_to_remove: Option<usize>,
) -> Result<()> {
    let stake_pool = &ctx.accounts.stake_pool;
    let mut state_account = ctx.accounts.state_account.load_mut()?;

    let clock = Clock::get()?;

    require!(
        clock.epoch == stake_pool.last_update_epoch,
        StewardError::StakePoolNotUpdated
    );

    {
        // Routine - Remove marked validators
        // We still want these checks to run even if we don't specify a validator to remove
        let validators_in_list = get_validator_list_length(&ctx.accounts.validator_list)?;
        let validators_to_remove = state_account.state.validators_to_remove.count();

        // Ensure the validators have been removed from the list
        require!(
            state_account.state.num_pool_validators + state_account.state.validators_added as usize
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
        let okay_to_update = state_account.state.validators_to_remove.is_empty();
        if okay_to_update {
            state_account.state.current_epoch = clock.epoch;
        }
    }

    Ok(())
}
