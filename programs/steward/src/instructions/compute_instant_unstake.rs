use crate::{
    errors::StewardError,
    maybe_transition_and_emit,
    utils::{get_validator_list, get_validator_list_length, get_validator_stake_info_at_index},
    Config, StewardStateAccount, StewardStateEnum,
};
use anchor_lang::prelude::*;
use validator_history::{ClusterHistory, ValidatorHistory};

#[derive(Accounts)]
pub struct ComputeInstantUnstake<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,

    pub validator_history: AccountLoader<'info, ValidatorHistory>,

    #[account(address = get_validator_list(&config)?)]
    /// CHECK: We check against the Config
    pub validator_list: AccountInfo<'info>,

    #[account(
        seeds = [ClusterHistory::SEED],
        seeds::program = validator_history::id(),
        bump
    )]
    pub cluster_history: AccountLoader<'info, ClusterHistory>,
}

pub fn handler(ctx: Context<ComputeInstantUnstake>, validator_list_index: usize) -> Result<()> {
    let config = ctx.accounts.config.load()?;
    let mut state_account = ctx.accounts.state_account.load_mut()?;
    let validator_history = ctx.accounts.validator_history.load()?;
    let cluster = ctx.accounts.cluster_history.load()?;
    let validator_list = &ctx.accounts.validator_list;
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
                StewardStateEnum::ComputeInstantUnstake
            ),
            StewardError::InvalidState
        );

        let validator_stake_info =
            get_validator_stake_info_at_index(validator_list, validator_list_index)?;
        require!(
            validator_stake_info.vote_account_address == validator_history.vote_account,
            StewardError::ValidatorNotInList
        );
    }

    if let Some(instant_unstake) = state_account.state.compute_instant_unstake(
        &clock,
        &epoch_schedule,
        &validator_history,
        validator_list_index,
        &cluster,
        &config,
    )? {
        emit!(instant_unstake);
    }

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
