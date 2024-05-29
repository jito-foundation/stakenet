use std::num::NonZeroU32;

use crate::constants::STAKE_POOL_WITHDRAW_SEED;
use crate::errors::StewardError;
use crate::state::{Config, Staker};
use crate::utils::{get_stake_pool, get_validator_stake_info_at_index, StakePool};
use crate::StewardStateAccount;
use anchor_lang::solana_program::{program::invoke_signed, stake, sysvar, vote};
use anchor_lang::{prelude::*, system_program};
use spl_pod::solana_program::borsh1::try_from_slice_unchecked;
use spl_pod::solana_program::stake::state::StakeStateV2;
use spl_stake_pool::{find_stake_program_address, find_transient_stake_program_address};
use validator_history::state::ValidatorHistory;

#[derive(Accounts)]
#[instruction(validator_list_index: usize)]
pub struct AutoRemoveValidator<'info> {
    #[account(
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        seeds::program = validator_history::ID,
        bump
    )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,

    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,

    /// CHECK: CPI address
    #[account(
        address = spl_stake_pool::ID
    )]
    pub stake_pool_program: AccountInfo<'info>,

    #[account(
        mut,
        address = get_stake_pool(&config)?
    )]
    pub stake_pool: Account<'info, StakePool>,

    #[account(
        seeds = [Staker::SEED, config.key().as_ref()],
        bump = staker.bump
    )]
    pub staker: Account<'info, Staker>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut, address = stake_pool.reserve_stake)]
    pub reserve_stake: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        seeds = [
            stake_pool.key().as_ref(),
            STAKE_POOL_WITHDRAW_SEED
        ],
        seeds::program = spl_stake_pool::ID,
        bump = stake_pool.stake_withdraw_bump_seed
    )]
    pub withdraw_authority: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut, address = stake_pool.validator_list)]
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
                    get_validator_stake_info_at_index(&validator_list, validator_list_index)?
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
            get_validator_stake_info_at_index(&validator_list, validator_list_index)?.transient_seed_suffix.into()
        ).0
    )]
    pub transient_stake_account: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(constraint = (vote_account.owner == &vote::program::ID ||  vote_account.owner == &system_program::ID))]
    pub vote_account: AccountInfo<'info>,

    pub rent: Sysvar<'info, Rent>,

    pub clock: Sysvar<'info, Clock>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = sysvar::stake_history::ID)]
    pub stake_history: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = stake::config::ID)]
    pub stake_config: AccountInfo<'info>,

    pub system_program: Program<'info, System>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = stake::program::ID)]
    pub stake_program: AccountInfo<'info>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

/*

*/
pub fn handler(ctx: Context<AutoRemoveValidator>, validator_list_index: usize) -> Result<()> {
    let mut state_account = ctx.accounts.state_account.load_mut()?;
    let validator_list = &ctx.accounts.validator_list;
    let epoch = Clock::get()?.epoch;

    let validator_stake_info =
        get_validator_stake_info_at_index(validator_list, validator_list_index)?;
    require!(
        validator_stake_info.vote_account_address == ctx.accounts.vote_account.key(),
        StewardError::ValidatorNotInList
    );

    // Checks state for deactivate delinquent status, preventing pool from merging stake with activating
    let stake_account_deactivated = {
        let stake_account_data = &mut ctx.accounts.stake_account.data.borrow_mut();
        let stake_state: StakeStateV2 =
            try_from_slice_unchecked::<StakeStateV2>(stake_account_data)?;

        let deactivation_epoch = match stake_state {
            StakeStateV2::Stake(_meta, stake, _stake_flags) => stake.delegation.deactivation_epoch,
            _ => return Err(StewardError::InvalidState.into()), // TODO fix
        };
        deactivation_epoch < epoch
    };

    // Check if vote account closed
    let vote_account_closed = ctx.accounts.vote_account.owner == &system_program::ID;

    require!(
        stake_account_deactivated || vote_account_closed,
        StewardError::ValidatorNotRemovable
    );

    state_account.state.remove_validator(validator_list_index)?;

    invoke_signed(
        &spl_stake_pool::instruction::remove_validator_from_pool(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.staker.key(),
            &ctx.accounts.withdraw_authority.key(),
            &ctx.accounts.validator_list.key(),
            &ctx.accounts.stake_account.key(),
            &ctx.accounts.transient_stake_account.key(),
        ),
        &[
            ctx.accounts.stake_pool.to_account_info(),
            ctx.accounts.staker.to_account_info(),
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
            Staker::SEED,
            &ctx.accounts.config.key().to_bytes(),
            &[ctx.accounts.staker.bump],
        ]],
    )?;

    Ok(())
}
