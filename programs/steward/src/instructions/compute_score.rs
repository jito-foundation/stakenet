use anchor_lang::prelude::*;

use crate::{
    errors::StewardError,
    maybe_transition,
    utils::{
        get_validator_list, get_validator_list_length, get_validator_stake_info_at_index,
        state_checks,
    },
    Config, StewardStateAccount, StewardStateEnum,
};
use validator_history::{ClusterHistory, ValidatorHistory};

#[derive(Accounts)]
pub struct ComputeScore<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,

    pub validator_history: AccountLoader<'info, ValidatorHistory>,

    /// CHECK: Account owner checked, account type checked in get_validator_stake_info_at_index
    #[account(address = get_validator_list(&config)?)]
    pub validator_list: AccountInfo<'info>,

    #[account(
        seeds = [ClusterHistory::SEED],
        seeds::program = validator_history::id(),
        bump
    )]
    pub cluster_history: AccountLoader<'info, ClusterHistory>,
}

pub fn handler(ctx: Context<ComputeScore>, validator_list_index: usize) -> Result<()> {
    let config = ctx.accounts.config.load()?;
    let mut state_account = ctx.accounts.state_account.load_mut()?;
    let validator_history = ctx.accounts.validator_history.load()?;
    let cluster_history = ctx.accounts.cluster_history.load()?;
    let validator_list = &ctx.accounts.validator_list;
    let clock: Clock = Clock::get()?;
    let epoch_schedule = EpochSchedule::get()?;

    // We don't check the state here because we force it below
    state_checks(&clock, &config, &state_account, validator_list, None)?;

    let validator_stake_info =
        get_validator_stake_info_at_index(validator_list, validator_list_index)?;
    require!(
        validator_stake_info.vote_account_address == validator_history.vote_account,
        StewardError::ValidatorNotInList
    );

    // May need to force an extra transition here in case cranking got stuck in any previous state
    // and it's now the start of a new scoring cycle
    if !matches!(
        state_account.state.state_tag,
        StewardStateEnum::ComputeScores
    ) {
        msg!(
            "Attempting state transition to ComputeScores from {}",
            state_account.state.state_tag
        );
        if let Some(event) = maybe_transition(
            &mut state_account.state,
            &clock,
            &config.parameters,
            &epoch_schedule,
        )? {
            emit!(event);
        }
    }

    require!(
        matches!(
            state_account.state.state_tag,
            StewardStateEnum::ComputeScores
        ),
        StewardError::InvalidState
    );

    let num_pool_validators = get_validator_list_length(validator_list)?;

    if let Some(score) = state_account.state.compute_score(
        &clock,
        &epoch_schedule,
        &validator_history,
        validator_list_index,
        &cluster_history,
        &config,
        num_pool_validators as u64,
    )? {
        msg!(
            "Scored validator at index {} / {}",
            validator_list_index,
            num_pool_validators
        );
        emit!(score);
    }

    // msg! the state progress
    msg!(
        "Scoring progress is complete: {:?}",
        state_account
            .state
            .progress
            .is_complete(num_pool_validators as u64)
    );

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
