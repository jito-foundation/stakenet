use crate::{
    errors::StewardError,
    events::IndexMismatchInterruptEvent,
    utils::{
        deserialize_stake_pool, deserialize_validator_list, get_stake_pool_address,
        get_validator_list_length,
    },
    Config, StewardStateAccount,
};
use anchor_lang::prelude::*;
use spl_stake_pool::state::StakeStatus;

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
pub fn handler(ctx: Context<IndexMismatchInterrupt>) -> Result<()> {
    let mut state_account = ctx.accounts.state_account.load_mut()?;
    let validator_list = deserialize_validator_list(&ctx.accounts.validator_list)?;

    // {
    //     // Need there to be a mismatch
    //     require!(
    //         state_account.state.num_pool_validators as usize
    //             + state_account.state.validators_added as usize
    //             != validator_list.validators.len(),
    //         StewardError::ListStateMismatch
    //     );

    //     require!(
    //         state_account.state.validators_to_remove.count() > 0,
    //         StewardError::NoValidatorsToRemove
    //     )
    // }

    // How do we know if a validator has been removed?

    //TODO index passed in has to be lowest marked for removal
    // Iterate through the list of validators and check status
    // Use diff amount to find correct index to remove
    //TODO take out validator index to remove - we always remove the lowest index

    let mut validator_index_to_remove = validator_list.validators.len();

    // for i in smallest_index..validator_list.validators.len() {
    //     let validator = &validator_list.validators[i];
    //     let stake_status = StakeStatus::try_from(validator.status).unwrap();

    //     match stake_status {
    //         StakeStatus::Active => {
    //             // If what's coming up is active this means the validator has been removed and
    //             // shifted
    //             validator_index_to_remove = i;
    //         }
    //         StakeStatus::DeactivatingTransient
    //         | StakeStatus::ReadyForRemoval
    //         | StakeStatus::DeactivatingValidator
    //         | StakeStatus::DeactivatingAll => {
    //             // Index has not yet been removed
    //         }
    //     }

    //     if validator_index_to_remove != validator_list.validators.len() {
    //         break;
    //     }
    // }

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
