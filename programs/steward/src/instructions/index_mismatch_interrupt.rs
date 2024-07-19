use crate::{
    errors::StewardError,
    events::IndexMismatchInterruptEvent,
    utils::{deserialize_stake_pool, get_stake_pool_address, get_validator_list_length},
    Config, StewardStateAccount,
};
use anchor_lang::prelude::*;
use spl_stake_pool::state::ValidatorListHeader;

#[derive(Accounts)]
pub struct IndexMismatchInterrupt<'info> {
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

/// Runs maintenance tasks at the start of each epoch, needs to be run multiple times
/// Routines:
/// - Remove delinquent validators
pub fn handler(
    ctx: Context<IndexMismatchInterrupt>,
    validator_index_to_remove: usize,
) -> Result<()> {
    let mut state_account = ctx.accounts.state_account.load_mut()?;

    // How do we know if a validator has been removed?

    {
        require!(
            state_account
                .state
                .validators_to_remove
                .get(validator_index_to_remove)?,
            StewardError::ValidatorNotMarkedForRemoval
        );
    }

    state_account
        .state
        .remove_validator(validator_index_to_remove)?;

    emit!(IndexMismatchInterruptEvent {
        validator_index_to_remove: validator_index_to_remove as u64,
        validator_list_length: get_validator_list_length(&ctx.accounts.validator_list)? as u64,
        num_pool_validators: state_account.state.num_pool_validators,
        validators_to_remove: state_account.state.validators_to_remove.count() as u64,
        validators_to_add: state_account.state.validators_added as u64,
    });

    Ok(())
}
