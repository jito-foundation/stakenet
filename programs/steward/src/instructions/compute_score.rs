use std::str::FromStr;

use anchor_lang::prelude::*;
use spl_pod::solana_program::{clock::Epoch, feature::Feature};

use crate::{
    constants::{TVC_FEATURE_PUBKEY, TVC_MAINNET_ACTIVATION_EPOCH},
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

    #[account(
        address = Pubkey::from_str(TVC_FEATURE_PUBKEY).unwrap()
    )]
    pub maybe_tvc_feature_account: Option<AccountInfo<'info>>,
}

pub fn handler(ctx: Context<ComputeScore>, validator_list_index: usize) -> Result<()> {
    let config = ctx.accounts.config.load()?;
    let mut state_account = ctx.accounts.state_account.load_mut()?;
    let validator_history = ctx.accounts.validator_history.load()?;
    let cluster_history = ctx.accounts.cluster_history.load()?;
    let validator_list = &ctx.accounts.validator_list;
    let clock: Clock = Clock::get()?;
    let epoch_schedule = EpochSchedule::get()?;

    let tvc_activation_epoch = {
        if let Some(tvc_feature_account) = ctx.accounts.maybe_tvc_feature_account.as_ref() {
            let activation_slot = Feature::from_account_info(tvc_feature_account)?.activated_at;
            if let Some(activation_slot) = activation_slot {
                epoch_schedule.get_epoch(activation_slot)
            } else {
                TVC_MAINNET_ACTIVATION_EPOCH
            }
        } else {
            TVC_MAINNET_ACTIVATION_EPOCH
        }
    };

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
        tvc_activation_epoch,
    )? {
        emit!(score);
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
