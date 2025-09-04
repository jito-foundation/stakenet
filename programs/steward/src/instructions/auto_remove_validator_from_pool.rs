use std::num::NonZeroU32;

use crate::constants::STAKE_POOL_WITHDRAW_SEED;
use crate::errors::StewardError;
use crate::events::AutoRemoveValidatorEvent;
use crate::state::Config;
use crate::StewardStateAccountV2;
use crate::{
    stake_pool_utils::deserialize_stake_pool,
    utils::{
        get_stake_pool_address, get_validator_stake_info_at_index, remove_validator_check,
        stake_is_inactive_without_history, stake_is_usable_by_pool,
    },
};
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    borsh1::try_from_slice_unchecked,
    program::invoke_signed,
    stake::{self, state::StakeStateV2},
    sysvar, vote,
};
use spl_stake_pool::state::StakeStatus;
use spl_stake_pool::{find_stake_program_address, find_transient_stake_program_address};
use validator_history::state::ValidatorHistory;

#[derive(Accounts)]
#[instruction(validator_list_index: u64)]
pub struct AutoRemoveValidator<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        seeds::program = validator_history::ID,
        bump
    )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,

    #[account(
        mut,
        seeds = [StewardStateAccountV2::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccountV2>,

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
            NonZeroU32::new(
                u32::from(
                    get_validator_stake_info_at_index(&validator_list, validator_list_index as usize)?
                        .validator_seed_suffix
                )
            )
        ).0,
        owner = stake::program::ID
    )]
    pub stake_account: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    /// Account may not exist yet so no owner check done
    #[account(
        mut,
        address = find_transient_stake_program_address(
            &spl_stake_pool::id(),
            &vote_account.key(),
            &stake_pool.key(),
            get_validator_stake_info_at_index(&validator_list, validator_list_index as usize)?.transient_seed_suffix.into()
        ).0
    )]
    pub transient_stake_account: AccountInfo<'info>,

    /// CHECK: Owner check done in handler
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

pub fn handler(ctx: Context<AutoRemoveValidator>, validator_list_index: usize) -> Result<()> {
    let stake_account_deactivated;
    let vote_account_closed;
    let clock = Clock::get()?;
    let epoch = clock.epoch;

    {
        let config = ctx.accounts.config.load()?;
        let state_account = ctx.accounts.state_account.load()?;
        let validator_list = &ctx.accounts.validator_list;

        remove_validator_check(&clock, &config, &state_account, validator_list)?;

        let validator_stake_info =
            get_validator_stake_info_at_index(validator_list, validator_list_index)?;
        require!(
            validator_stake_info.vote_account_address == ctx.accounts.vote_account.key(),
            StewardError::ValidatorNotInList
        );

        // Checks state for deactivate delinquent status, preventing pool from merging stake with activating
        stake_account_deactivated = {
            let stake_account_data = &mut ctx.accounts.stake_account.data.borrow_mut();
            let stake_state: StakeStateV2 =
                try_from_slice_unchecked::<StakeStateV2>(stake_account_data)?;

            let deactivation_epoch = match stake_state {
                StakeStateV2::Stake(_meta, stake, _stake_flags) => {
                    stake.delegation.deactivation_epoch
                }
                _ => return Err(StewardError::StakeStateIsNotStake.into()),
            };
            deactivation_epoch < epoch
        };

        // Check if vote account closed
        vote_account_closed = *ctx.accounts.vote_account.owner != vote::program::ID;

        require!(
            stake_account_deactivated || vote_account_closed,
            StewardError::ValidatorNotRemovable
        );
    }

    {
        invoke_signed(
            &spl_stake_pool::instruction::remove_validator_from_pool(
                &ctx.accounts.stake_pool_program.key(),
                &ctx.accounts.stake_pool.key(),
                &ctx.accounts.state_account.key(),
                &ctx.accounts.withdraw_authority.key(),
                &ctx.accounts.validator_list.key(),
                &ctx.accounts.stake_account.key(),
                &ctx.accounts.transient_stake_account.key(),
            ),
            &[
                ctx.accounts.stake_pool.to_account_info(),
                ctx.accounts.state_account.to_account_info(),
                ctx.accounts.reserve_stake.to_owned(),
                ctx.accounts.withdraw_authority.to_owned(),
                ctx.accounts.validator_list.to_account_info(),
                ctx.accounts.stake_account.to_account_info(),
                ctx.accounts.transient_stake_account.to_account_info(),
                ctx.accounts.vote_account.to_account_info(),
                ctx.accounts.rent.to_account_info(),
                ctx.accounts.clock.to_account_info(),
                ctx.accounts.stake_history.to_account_info(),
                ctx.accounts.stake_config.to_account_info(),
                ctx.accounts.system_program.to_account_info(),
                ctx.accounts.stake_program.to_account_info(),
            ],
            &[&[
                StewardStateAccountV2::SEED,
                &ctx.accounts.config.key().to_bytes(),
                &[ctx.bumps.state_account],
            ]],
        )?;
    }

    {
        // Read the state account again
        let mut state_account = ctx.accounts.state_account.load_mut()?;
        let validator_list = &ctx.accounts.validator_list;
        let validator_stake_info =
            get_validator_stake_info_at_index(validator_list, validator_list_index)?;

        let stake_status = StakeStatus::try_from(validator_stake_info.status)?;
        let marked_for_immediate_removal: bool;

        let stake_pool = deserialize_stake_pool(&ctx.accounts.stake_pool)?;

        match stake_status {
            StakeStatus::Active => {
                // Should never happen
                return Err(StewardError::ValidatorMarkedActive.into());
            }
            StakeStatus::DeactivatingValidator => {
                let stake_account_data = &mut ctx.accounts.stake_account.data.borrow_mut();
                let (meta, stake) =
                    match try_from_slice_unchecked::<StakeStateV2>(stake_account_data)? {
                        StakeStateV2::Stake(meta, stake, _stake_flags) => (meta, stake),
                        _ => return Err(StewardError::StakeStateIsNotStake.into()),
                    };

                if stake_is_usable_by_pool(
                    &meta,
                    ctx.accounts.withdraw_authority.key,
                    &stake_pool.lockup,
                ) && stake_is_inactive_without_history(&stake, epoch)
                {
                    state_account
                        .state
                        .mark_validator_for_immediate_removal(validator_list_index)?;
                    marked_for_immediate_removal = true;
                } else {
                    state_account
                        .state
                        .mark_validator_for_removal(validator_list_index)?;
                    marked_for_immediate_removal = false;
                }
            }
            StakeStatus::ReadyForRemoval => {
                // Should never happen but this is logical action
                marked_for_immediate_removal = true;
                state_account
                    .state
                    .mark_validator_for_immediate_removal(validator_list_index)?;
            }
            StakeStatus::DeactivatingAll | StakeStatus::DeactivatingTransient => {
                // DeactivatingTransient should not be possible but this is the logical action
                marked_for_immediate_removal = false;
                state_account
                    .state
                    .mark_validator_for_removal(validator_list_index)?;
            }
        }

        emit!(AutoRemoveValidatorEvent {
            vote_account: ctx.accounts.vote_account.key(),
            validator_list_index: validator_list_index as u64,
            stake_account_deactivated,
            vote_account_closed,
            marked_for_immediate_removal,
        });
    }

    Ok(())
}
