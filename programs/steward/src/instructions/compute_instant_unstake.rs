use crate::{
    errors::StewardError,
    maybe_transition,
    utils::{get_validator_list, get_validator_stake_info_at_index, state_checks},
    Config, StewardStateAccountV2, StewardStateEnum,
};
use anchor_lang::prelude::*;
use validator_history::{ClusterHistory, ValidatorHistory};

#[derive(Accounts)]
pub struct ComputeInstantUnstake<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccountV2::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccountV2>,

    /// CHECK: We check it is the correct vote account in the handler
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

    // Transitions to Idle before doing compute_instant_unstake if RESET_TO_IDLE is set
    if let Some(event) = maybe_transition(
        &mut state_account.state,
        &clock,
        &config.parameters,
        &epoch_schedule,
    )? {
        emit!(event);
        return Ok(());
    }

    state_checks(
        &clock,
        &config,
        &state_account,
        &ctx.accounts.validator_list,
        Some(StewardStateEnum::ComputeInstantUnstake),
    )?;

    let validator_stake_info =
        get_validator_stake_info_at_index(validator_list, validator_list_index)?;
    require!(
        validator_stake_info.vote_account_address == validator_history.vote_account,
        StewardError::ValidatorNotInList
    );

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

    if let Some(event) = maybe_transition(
        &mut state_account.state,
        &clock,
        &config.parameters,
        &epoch_schedule,
    )? {
        emit!(event);
    }

    Ok(())
}
