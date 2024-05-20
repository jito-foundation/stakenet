use anchor_lang::prelude::*;
use spl_stake_pool::state::ValidatorListHeader;

use crate::{
    errors::StewardError, maybe_transition_and_emit, utils::get_validator_stake_info_at_index,
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

    #[account(owner = spl_stake_pool::id())]
    pub validator_list: AccountInfo<'info>,

    #[account(
        seeds = [ClusterHistory::SEED],
        seeds::program = validator_history::id(),
        bump
    )]
    pub cluster_history: AccountLoader<'info, ClusterHistory>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handler(ctx: Context<ComputeScore>, validator_list_index: usize) -> Result<()> {
    let config = ctx.accounts.config.load()?;
    let mut state_account = ctx.accounts.state_account.load_mut()?;
    let validator_history = ctx.accounts.validator_history.load()?;
    let cluster_history = ctx.accounts.cluster_history.load()?;
    let validator_list = &ctx.accounts.validator_list;
    let clock: Clock = Clock::get()?;
    let epoch_schedule = EpochSchedule::get()?;

    let validator_stake_info =
        get_validator_stake_info_at_index(validator_list, validator_list_index)?;
    require!(
        validator_stake_info.vote_account_address == validator_history.vote_account,
        StewardError::ValidatorNotInList
    );

    let num_pool_validators = {
        let mut validator_list_data = validator_list.try_borrow_mut_data()?;
        let (_, validator_list) = ValidatorListHeader::deserialize_vec(&mut validator_list_data)?;
        validator_list.len() as usize
    };

    if config.is_paused() {
        return Err(StewardError::StateMachinePaused.into());
    }

    // May need to force an extra transition here in case cranking got stuck in any previous state
    // and it's now the start of a new scoring cycle
    if !matches!(
        state_account.state.state_tag,
        StewardStateEnum::ComputeScores
    ) {
        maybe_transition_and_emit(
            &mut state_account.state,
            &clock,
            &config.parameters,
            &epoch_schedule,
        )?;
    }
    require!(
        matches!(
            state_account.state.state_tag,
            StewardStateEnum::ComputeScores
        ),
        StewardError::InvalidState
    );

    state_account.state.compute_score(
        &clock,
        &epoch_schedule,
        &validator_history,
        validator_list_index,
        &cluster_history,
        &config,
        num_pool_validators,
    )?;

    maybe_transition_and_emit(
        &mut state_account.state,
        &clock,
        &config.parameters,
        &epoch_schedule,
    )?;

    Ok(())
}
