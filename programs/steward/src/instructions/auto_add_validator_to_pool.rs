use crate::constants::{MAX_VALIDATORS, STAKE_POOL_WITHDRAW_SEED};
use crate::errors::StewardError;
use crate::events::AutoAddValidatorEvent;
use crate::state::{Config, StewardStateAccount, StewardStateAccountV2};
use crate::{
    stake_pool_utils::deserialize_stake_pool,
    utils::{add_validator_check, get_stake_pool_address, get_validator_list_length},
};
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke_signed, stake, sysvar, vote};
use spl_stake_pool::find_stake_program_address;
use validator_history::state::ValidatorHistory;
use validator_history::ValidatorHistoryEntry;

#[derive(Accounts)]
pub struct AutoAddValidator<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub steward_state: AccountLoader<'info, StewardStateAccountV2>,

    // Only adding validators where this exists
    #[account(
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        seeds::program = validator_history::ID,
        bump
    )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        mut,
        address = get_stake_pool_address(&config)?
    )]
    pub stake_pool: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut, address = deserialize_stake_pool(&stake_pool)?.reserve_stake)]
    pub reserve_stake: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        seeds = [
            stake_pool.key().as_ref(),
            STAKE_POOL_WITHDRAW_SEED
        ],
        seeds::program = spl_stake_pool::ID,
        bump = deserialize_stake_pool(&stake_pool)?.stake_withdraw_bump_seed
    )]
    pub withdraw_authority: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut, address = deserialize_stake_pool(&stake_pool)?.validator_list)]
    pub validator_list: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        mut,
        address = find_stake_program_address(
            &spl_stake_pool::id(),
            &vote_account.key(),
            &stake_pool.key(),
            None,
        ).0,
    )]
    pub stake_account: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(owner = vote::program::ID)]
    pub vote_account: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = sysvar::stake_history::ID)]
    pub stake_history: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = stake::config::ID)]
    pub stake_config: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = stake::program::ID)]
    pub stake_program: AccountInfo<'info>,

    /// CHECK: CPI address
    #[account(
        address = spl_stake_pool::ID
    )]
    pub stake_pool_program: AccountInfo<'info>,

    pub system_program: Program<'info, System>,

    pub rent: Sysvar<'info, Rent>,

    pub clock: Sysvar<'info, Clock>,
}

/*
`AutoAddValidatorToPool` adds validators to the pool, to ensure that the stake pool validator list contains
all the validators we want to be eligible for delegation, as well as to accept stake deposits from out-of-pool validators.
Performs some eligibility checks in order to not fill up the validator list with offline or malicious validators.
*/
pub fn handler(ctx: Context<AutoAddValidator>) -> Result<()> {
    {
        let mut state_account = ctx.accounts.steward_state.load_mut()?;
        let config = ctx.accounts.config.load()?;
        let validator_history = ctx.accounts.validator_history_account.load()?;
        let validator_list = &ctx.accounts.validator_list;
        let clock = Clock::get()?;
        let epoch = clock.epoch;

        add_validator_check(&clock, &config, &state_account, validator_list)?;

        let validator_list_len = get_validator_list_length(&ctx.accounts.validator_list)?;
        if validator_list_len.checked_add(1).unwrap() > MAX_VALIDATORS {
            return Err(StewardError::MaxValidatorsReached.into());
        }

        let start_epoch =
            epoch.saturating_sub(config.parameters.minimum_voting_epochs.saturating_sub(1));
        if let Some(entry) = validator_history.history.last() {
            // Steward requires that validators have been active for last minimum_voting_epochs epochs
            if validator_history
                .history
                .epoch_credits_range(start_epoch as u16, epoch as u16)
                .iter()
                .any(|entry| entry.is_none())
            {
                return Err(StewardError::ValidatorBelowLivenessMinimum.into());
            }
            if entry.activated_stake_lamports
                == ValidatorHistoryEntry::default().activated_stake_lamports
            {
                return Err(StewardError::StakeHistoryNotRecentEnough.into());
            }
            if entry.activated_stake_lamports < config.parameters.minimum_stake_lamports {
                return Err(StewardError::ValidatorBelowStakeMinimum.into());
            }
        } else {
            return Err(StewardError::ValidatorBelowLivenessMinimum.into());
        }

        state_account.state.increment_validator_to_add()?;

        emit!(AutoAddValidatorEvent {
            vote_account: ctx.accounts.vote_account.key(),
            validator_list_index: validator_list_len as u64
        });
    }

    invoke_signed(
        &spl_stake_pool::instruction::add_validator_to_pool(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.steward_state.key(),
            &ctx.accounts.reserve_stake.key(),
            &ctx.accounts.withdraw_authority.key(),
            &ctx.accounts.validator_list.key(),
            &ctx.accounts.stake_account.key(),
            &ctx.accounts.vote_account.key(),
            None,
        ),
        &[
            ctx.accounts.stake_pool.to_account_info(),
            ctx.accounts.steward_state.to_account_info(),
            ctx.accounts.reserve_stake.to_owned(),
            ctx.accounts.withdraw_authority.to_owned(),
            ctx.accounts.validator_list.to_account_info(),
            ctx.accounts.stake_account.to_owned(),
            ctx.accounts.vote_account.to_account_info(),
            ctx.accounts.rent.to_account_info(),
            ctx.accounts.clock.to_account_info(),
            ctx.accounts.stake_history.to_account_info(),
            ctx.accounts.stake_config.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
            ctx.accounts.stake_program.to_account_info(),
        ],
        &[&[
            StewardStateAccount::SEED,
            &ctx.accounts.config.key().to_bytes(),
            &[ctx.bumps.steward_state],
        ]],
    )?;

    Ok(())
}
