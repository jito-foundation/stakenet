// All spl-stake-pool instructions which need to be signed by the staker keypair.
// Nearly all accounts are passed through to a spl-stake-pool instruction, which does its own
// checks on the validity of each account. All that's important for these instructions to check
// is that the config, stake pool address, staker, signer, and sometimes state account match up.
// Otherwise these instructions are intented to be minimally restrictive.

use crate::constants::{LAMPORT_BALANCE_DEFAULT, MAX_VALIDATORS};
use crate::errors::StewardError;
use crate::state::Config;
use crate::utils::{
    deserialize_stake_pool, get_config_admin, get_stake_pool_address,
    get_validator_stake_info_at_index, stake_is_inactive_without_history, stake_is_usable_by_pool,
};
use crate::StewardStateAccount;
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke_signed, stake, sysvar, vote};
use spl_pod::solana_program::borsh1::try_from_slice_unchecked;
use spl_pod::solana_program::stake::state::StakeStateV2;
use spl_stake_pool::find_stake_program_address;
use spl_stake_pool::instruction::PreferredValidatorType;
use spl_stake_pool::state::{StakeStatus, ValidatorListHeader};
use std::num::NonZeroU32;
use validator_history::ValidatorHistory;

#[derive(Accounts)]
pub struct AddValidatorToPool<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,
    /// CHECK: CPI program
    #[account(
        address = spl_stake_pool::ID
    )]
    pub stake_pool_program: AccountInfo<'info>,
    #[account(
        mut,
        address = get_stake_pool_address(&config)?
    )]
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub stake_pool: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub reserve_stake: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub withdraw_authority: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut, address = deserialize_stake_pool(&stake_pool)?.validator_list)]
    pub validator_list: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub stake_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(owner = vote::program::ID)]
    pub vote_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub rent: AccountInfo<'info>,
    pub clock: Sysvar<'info, Clock>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub stake_history: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = stake::config::ID)]
    pub stake_config: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = stake::program::ID)]
    pub stake_program: AccountInfo<'info>,
    #[account(mut, address = get_config_admin(&config)?)]
    pub admin: Signer<'info>,
}

pub fn add_validator_to_pool_handler(
    ctx: Context<AddValidatorToPool>,
    validator_seed: Option<u32>,
) -> Result<()> {
    {
        let mut state_account = ctx.accounts.state_account.load_mut()?;
        let epoch = Clock::get()?.epoch;

        // Should not be able to add a validator if update is not complete
        require!(
            epoch == state_account.state.current_epoch,
            StewardError::EpochMaintenanceNotComplete
        );

        {
            let validator_list_data = &mut ctx.accounts.validator_list.try_borrow_mut_data()?;
            let (_, validator_list) = ValidatorListHeader::deserialize_vec(validator_list_data)?;

            if validator_list.len().checked_add(1).unwrap() > MAX_VALIDATORS as u32 {
                return Err(StewardError::MaxValidatorsReached.into());
            }
        }

        state_account.state.increment_validator_to_add()?;
    }

    invoke_signed(
        &spl_stake_pool::instruction::add_validator_to_pool(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.state_account.key(),
            &ctx.accounts.reserve_stake.key(),
            &ctx.accounts.withdraw_authority.key(),
            &ctx.accounts.validator_list.key(),
            &ctx.accounts.stake_account.key(),
            &ctx.accounts.vote_account.key(),
            NonZeroU32::new(validator_seed.unwrap_or_default()),
        ),
        &[
            ctx.accounts.stake_pool.to_account_info(),
            ctx.accounts.state_account.to_account_info(),
            ctx.accounts.reserve_stake.to_owned(),
            ctx.accounts.withdraw_authority.to_owned(),
            ctx.accounts.validator_list.to_account_info(),
            ctx.accounts.stake_account.to_account_info(),
            ctx.accounts.vote_account.to_owned(),
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
            &[ctx.bumps.state_account],
        ]],
    )?;
    Ok(())
}

#[derive(Accounts)]
pub struct RemoveValidatorFromPool<'info> {
    pub config: AccountLoader<'info, Config>,
    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,

    /// CHECK: CPI program
    #[account(
        address = spl_stake_pool::ID
    )]
    pub stake_pool_program: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        mut,
        address = get_stake_pool_address(&config)?
    )]
    pub stake_pool: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    pub withdraw_authority: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut, address = deserialize_stake_pool(&stake_pool)?.validator_list)]
    pub validator_list: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub stake_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub transient_stake_account: AccountInfo<'info>,
    pub clock: Sysvar<'info, Clock>,
    pub system_program: Program<'info, System>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = stake::program::ID)]
    pub stake_program: AccountInfo<'info>,
    #[account(mut, address = get_config_admin(&config)?)]
    pub admin: Signer<'info>,
}

pub fn remove_validator_from_pool_handler(
    ctx: Context<RemoveValidatorFromPool>,
    validator_list_index: usize,
) -> Result<()> {
    let epoch = Clock::get()?.epoch;
    {
        let state_account = ctx.accounts.state_account.load_mut()?;

        // Should not be able to remove a validator if update is not complete
        require!(
            epoch == state_account.state.current_epoch,
            StewardError::EpochMaintenanceNotComplete
        );

        if validator_list_index < state_account.state.num_pool_validators as usize {
            let validator_list_stake_info = get_validator_stake_info_at_index(
                &ctx.accounts.validator_list.to_account_info(),
                validator_list_index,
            )?;

            let (validator_list_stake_account, _) = find_stake_program_address(
                &ctx.accounts.stake_pool_program.key(),
                &validator_list_stake_info.vote_account_address,
                &ctx.accounts.stake_pool.key(),
                NonZeroU32::new(u32::from(validator_list_stake_info.validator_seed_suffix)),
            );

            if validator_list_stake_account != ctx.accounts.stake_account.key() {
                return Err(StewardError::ValidatorNotInList.into());
            }
        }
    }

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
            ctx.accounts.withdraw_authority.to_owned(),
            ctx.accounts.validator_list.to_account_info(),
            ctx.accounts.stake_account.to_account_info(),
            ctx.accounts.transient_stake_account.to_account_info(),
            ctx.accounts.clock.to_account_info(),
            ctx.accounts.stake_program.to_account_info(),
        ],
        &[&[
            StewardStateAccount::SEED,
            &ctx.accounts.config.key().to_bytes(),
            &[ctx.bumps.state_account],
        ]],
    )?;

    {
        // Read the state account again
        let mut state_account = ctx.accounts.state_account.load_mut()?;
        let validator_list = &ctx.accounts.validator_list;
        let validator_stake_info =
            get_validator_stake_info_at_index(validator_list, validator_list_index)?;

        let stake_status = StakeStatus::try_from(validator_stake_info.status)?;

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
                } else {
                    state_account
                        .state
                        .mark_validator_for_removal(validator_list_index)?;
                }
            }
            StakeStatus::ReadyForRemoval => {
                state_account
                    .state
                    .mark_validator_for_immediate_removal(validator_list_index)?;
            }
            StakeStatus::DeactivatingAll | StakeStatus::DeactivatingTransient => {
                state_account
                    .state
                    .mark_validator_for_removal(validator_list_index)?;
            }
        }
    }
    Ok(())
}

#[derive(Accounts)]
pub struct SetPreferredValidator<'info> {
    pub config: AccountLoader<'info, Config>,
    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,
    /// CHECK: CPI program
    #[account(
        address = spl_stake_pool::ID
    )]
    pub stake_pool_program: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        mut,
        address = get_stake_pool_address(&config)?
    )]
    pub stake_pool: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = deserialize_stake_pool(&stake_pool)?.validator_list)]
    pub validator_list: AccountInfo<'info>,
    #[account(mut, address = get_config_admin(&config)?)]
    pub admin: Signer<'info>,
}

pub fn set_preferred_validator_handler(
    ctx: Context<SetPreferredValidator>,
    validator_type: &PreferredValidatorType,
    validator: Option<Pubkey>,
) -> Result<()> {
    invoke_signed(
        &spl_stake_pool::instruction::set_preferred_validator(
            ctx.accounts.stake_pool_program.key,
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.state_account.key(),
            &ctx.accounts.validator_list.key(),
            validator_type.clone(),
            validator,
        ),
        &[
            ctx.accounts.stake_pool.to_account_info(),
            ctx.accounts.state_account.to_account_info(),
            ctx.accounts.validator_list.to_account_info(),
        ],
        &[&[
            StewardStateAccount::SEED,
            &ctx.accounts.config.key().to_bytes(),
            &[ctx.bumps.state_account],
        ]],
    )?;
    Ok(())
}

#[derive(Accounts)]
pub struct IncreaseValidatorStake<'info> {
    pub config: AccountLoader<'info, Config>,
    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,
    #[account(
        mut,
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        seeds::program = validator_history::ID,
        bump
    )]
    pub validator_history: AccountLoader<'info, ValidatorHistory>,
    /// CHECK: CPI program
    #[account(
        address = spl_stake_pool::ID
    )]
    pub stake_pool_program: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        mut,
        address = get_stake_pool_address(&config)?
    )]
    pub stake_pool: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    pub withdraw_authority: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut, address = deserialize_stake_pool(&stake_pool)?.validator_list)]
    pub validator_list: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        mut,
        address = deserialize_stake_pool(&stake_pool)?.reserve_stake
    )]
    pub reserve_stake: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub transient_stake_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub stake_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(owner = vote::program::ID)]
    pub vote_account: AccountInfo<'info>,
    pub clock: Sysvar<'info, Clock>,
    pub rent: Sysvar<'info, Rent>,
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
    #[account(mut, address = get_config_admin(&config)?)]
    pub admin: Signer<'info>,
}
pub fn increase_validator_stake_handler(
    ctx: Context<IncreaseValidatorStake>,
    lamports: u64,
    transient_seed: u64,
) -> Result<()> {
    {
        let validator_history = ctx.accounts.validator_history.load()?;
        let mut state_account = ctx.accounts.state_account.load_mut()?;
        // Get the balance
        let balance = state_account
            .state
            .validator_lamport_balances
            .get_mut(validator_history.index as usize)
            .ok_or(StewardError::ValidatorIndexOutOfBounds)?;

        // Set the balance
        if *balance != LAMPORT_BALANCE_DEFAULT {
            *balance = balance
                .checked_add(lamports)
                .ok_or(StewardError::ArithmeticError)?;
        }
    }

    invoke_signed(
        &spl_stake_pool::instruction::increase_validator_stake(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.state_account.key(),
            &ctx.accounts.withdraw_authority.key(),
            &ctx.accounts.validator_list.key(),
            &ctx.accounts.reserve_stake.key(),
            &ctx.accounts.transient_stake_account.key(),
            &ctx.accounts.stake_account.key(),
            &ctx.accounts.vote_account.key(),
            lamports,
            transient_seed,
        ),
        &[
            ctx.accounts.stake_pool.to_account_info(),
            ctx.accounts.state_account.to_account_info(),
            ctx.accounts.withdraw_authority.to_owned(),
            ctx.accounts.validator_list.to_account_info(),
            ctx.accounts.reserve_stake.to_account_info(),
            ctx.accounts.transient_stake_account.to_account_info(),
            ctx.accounts.stake_account.to_account_info(),
            ctx.accounts.vote_account.to_owned(),
            ctx.accounts.clock.to_account_info(),
            ctx.accounts.rent.to_account_info(),
            ctx.accounts.stake_history.to_account_info(),
            ctx.accounts.stake_config.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
            ctx.accounts.stake_program.to_account_info(),
        ],
        &[&[
            StewardStateAccount::SEED,
            &ctx.accounts.config.key().to_bytes(),
            &[ctx.bumps.state_account],
        ]],
    )?;
    Ok(())
}

#[derive(Accounts)]
pub struct DecreaseValidatorStake<'info> {
    pub config: AccountLoader<'info, Config>,
    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,
    #[account(
        mut,
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        seeds::program = validator_history::ID,
        bump
    )]
    pub validator_history: AccountLoader<'info, ValidatorHistory>,
    /// CHECK: CPI program
    #[account(
        address = spl_stake_pool::ID
    )]
    pub stake_pool_program: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        mut,
        address = get_stake_pool_address(&config)?
    )]
    pub stake_pool: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    pub withdraw_authority: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut, address = deserialize_stake_pool(&stake_pool)?.validator_list)]
    pub validator_list: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        mut,
        address = deserialize_stake_pool(&stake_pool)?.reserve_stake
    )]
    pub reserve_stake: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub stake_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub transient_stake_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(owner = vote::program::ID)]
    pub vote_account: AccountInfo<'info>,
    pub clock: Sysvar<'info, Clock>,
    pub rent: Sysvar<'info, Rent>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = sysvar::stake_history::ID)]
    pub stake_history: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = stake::program::ID)]
    pub stake_program: AccountInfo<'info>,
    #[account(mut, address = get_config_admin(&config)?)]
    pub admin: Signer<'info>,
}

pub fn decrease_validator_stake_handler(
    ctx: Context<DecreaseValidatorStake>,
    lamports: u64,
    transient_seed: u64,
) -> Result<()> {
    {
        let validator_history = ctx.accounts.validator_history.load()?;
        let mut state_account = ctx.accounts.state_account.load_mut()?;
        // Get the balance
        let balance = state_account
            .state
            .validator_lamport_balances
            .get_mut(validator_history.index as usize)
            .ok_or(StewardError::ValidatorIndexOutOfBounds)?;

        // Set the balance
        if *balance != LAMPORT_BALANCE_DEFAULT {
            *balance = balance
                .checked_sub(lamports)
                .ok_or(StewardError::ArithmeticError)?;
        }
    }

    invoke_signed(
        &spl_stake_pool::instruction::decrease_validator_stake_with_reserve(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.state_account.key(),
            &ctx.accounts.withdraw_authority.key(),
            &ctx.accounts.validator_list.key(),
            &ctx.accounts.reserve_stake.key(),
            &ctx.accounts.stake_account.key(),
            &ctx.accounts.transient_stake_account.key(),
            lamports,
            transient_seed,
        ),
        &[
            ctx.accounts.stake_pool.to_account_info(),
            ctx.accounts.state_account.to_account_info(),
            ctx.accounts.withdraw_authority.to_owned(),
            ctx.accounts.validator_list.to_account_info(),
            ctx.accounts.reserve_stake.to_account_info(),
            ctx.accounts.stake_account.to_account_info(),
            ctx.accounts.transient_stake_account.to_account_info(),
            ctx.accounts.clock.to_account_info(),
            ctx.accounts.rent.to_account_info(),
            ctx.accounts.stake_history.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
            ctx.accounts.stake_program.to_account_info(),
        ],
        &[&[
            StewardStateAccount::SEED,
            &ctx.accounts.config.key().to_bytes(),
            &[ctx.bumps.state_account],
        ]],
    )?;
    Ok(())
}

#[derive(Accounts)]
pub struct IncreaseAdditionalValidatorStake<'info> {
    pub config: AccountLoader<'info, Config>,
    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,
    #[account(
        mut,
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        seeds::program = validator_history::ID,
        bump
    )]
    pub validator_history: AccountLoader<'info, ValidatorHistory>,
    /// CHECK: CPI program
    #[account(
        address = spl_stake_pool::ID
    )]
    pub stake_pool_program: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        address = get_stake_pool_address(&config)?
    )]
    pub stake_pool: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    pub withdraw_authority: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut, address = deserialize_stake_pool(&stake_pool)?.validator_list)]
    pub validator_list: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub reserve_stake: AccountInfo<'info>,
    // Uninitialized
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub ephemeral_stake_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub transient_stake_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub stake_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(owner = vote::program::ID)]
    pub vote_account: AccountInfo<'info>,
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
    #[account(mut, address = get_config_admin(&config)?)]
    pub admin: Signer<'info>,
}

pub fn increase_additional_validator_stake_handler(
    ctx: Context<IncreaseAdditionalValidatorStake>,
    lamports: u64,
    transient_seed: u64,
    ephemeral_seed: u64,
) -> Result<()> {
    {
        let validator_history = ctx.accounts.validator_history.load()?;
        let mut state_account = ctx.accounts.state_account.load_mut()?;
        // Get the balance
        let balance = state_account
            .state
            .validator_lamport_balances
            .get_mut(validator_history.index as usize)
            .ok_or(StewardError::ValidatorIndexOutOfBounds)?;

        // Set the balance
        if *balance != LAMPORT_BALANCE_DEFAULT {
            *balance = balance
                .checked_add(lamports)
                .ok_or(StewardError::ArithmeticError)?;
        }
    }

    invoke_signed(
        &spl_stake_pool::instruction::increase_additional_validator_stake(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.state_account.key(),
            &ctx.accounts.withdraw_authority.key(),
            &ctx.accounts.validator_list.key(),
            &ctx.accounts.reserve_stake.key(),
            &ctx.accounts.ephemeral_stake_account.key(),
            &ctx.accounts.transient_stake_account.key(),
            &ctx.accounts.stake_account.key(),
            &ctx.accounts.vote_account.key(),
            lamports,
            transient_seed,
            ephemeral_seed,
        ),
        &[
            ctx.accounts.stake_pool.to_account_info(),
            ctx.accounts.state_account.to_account_info(),
            ctx.accounts.withdraw_authority.to_owned(),
            ctx.accounts.validator_list.to_account_info(),
            ctx.accounts.reserve_stake.to_account_info(),
            ctx.accounts.ephemeral_stake_account.to_account_info(),
            ctx.accounts.transient_stake_account.to_account_info(),
            ctx.accounts.stake_account.to_account_info(),
            ctx.accounts.vote_account.to_owned(),
            ctx.accounts.clock.to_account_info(),
            ctx.accounts.stake_history.to_account_info(),
            ctx.accounts.stake_config.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
            ctx.accounts.stake_program.to_account_info(),
        ],
        &[&[
            StewardStateAccount::SEED,
            &ctx.accounts.config.key().to_bytes(),
            &[ctx.bumps.state_account],
        ]],
    )?;
    Ok(())
}

#[derive(Accounts)]
pub struct DecreaseAdditionalValidatorStake<'info> {
    pub config: AccountLoader<'info, Config>,
    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,
    #[account(
        mut,
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        seeds::program = validator_history::ID,
        bump
    )]
    pub validator_history: AccountLoader<'info, ValidatorHistory>,
    /// CHECK: Used to derive validator history seed
    #[account(
        owner = vote::program::ID
    )]
    pub vote_account: AccountInfo<'info>,
    #[account(
        address = spl_stake_pool::ID
    )]
    /// CHECK: CPI program
    pub stake_pool_program: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        address = get_stake_pool_address(&config)?
    )]
    pub stake_pool: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    pub withdraw_authority: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut, address = deserialize_stake_pool(&stake_pool)?.validator_list)]
    pub validator_list: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub reserve_stake: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub stake_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub ephemeral_stake_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub transient_stake_account: AccountInfo<'info>,
    pub clock: Sysvar<'info, Clock>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = sysvar::stake_history::ID)]
    pub stake_history: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = stake::program::ID)]
    pub stake_program: AccountInfo<'info>,
    #[account(mut, address = get_config_admin(&config)?)]
    pub admin: Signer<'info>,
}

pub fn decrease_additional_validator_stake_handler(
    ctx: Context<DecreaseAdditionalValidatorStake>,
    lamports: u64,
    transient_seed: u64,
    ephemeral_seed: u64,
) -> Result<()> {
    {
        let validator_history = ctx.accounts.validator_history.load()?;
        let mut state_account = ctx.accounts.state_account.load_mut()?;
        // Get the balance
        let balance = state_account
            .state
            .validator_lamport_balances
            .get_mut(validator_history.index as usize)
            .ok_or(StewardError::ValidatorIndexOutOfBounds)?;

        // Set the balance
        if *balance != LAMPORT_BALANCE_DEFAULT {
            *balance = balance
                .checked_sub(lamports)
                .ok_or(StewardError::ArithmeticError)?;
        }
    }

    invoke_signed(
        &spl_stake_pool::instruction::decrease_additional_validator_stake(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.state_account.key(),
            &ctx.accounts.withdraw_authority.key(),
            &ctx.accounts.validator_list.key(),
            &ctx.accounts.reserve_stake.key(),
            &ctx.accounts.stake_account.key(),
            &ctx.accounts.ephemeral_stake_account.key(),
            &ctx.accounts.transient_stake_account.key(),
            lamports,
            transient_seed,
            ephemeral_seed,
        ),
        &[
            ctx.accounts.stake_pool.to_account_info(),
            ctx.accounts.state_account.to_account_info(),
            ctx.accounts.withdraw_authority.to_owned(),
            ctx.accounts.validator_list.to_account_info(),
            ctx.accounts.reserve_stake.to_account_info(),
            ctx.accounts.stake_account.to_account_info(),
            ctx.accounts.ephemeral_stake_account.to_account_info(),
            ctx.accounts.transient_stake_account.to_account_info(),
            ctx.accounts.clock.to_account_info(),
            ctx.accounts.stake_history.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
            ctx.accounts.stake_program.to_account_info(),
        ],
        &[&[
            StewardStateAccount::SEED,
            &ctx.accounts.config.key().to_bytes(),
            &[ctx.bumps.state_account],
        ]],
    )?;
    Ok(())
}

#[derive(Accounts)]
pub struct SetStaker<'info> {
    pub config: AccountLoader<'info, Config>,
    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,
    /// CHECK: CPI program
    #[account(
        address = spl_stake_pool::ID
    )]
    pub stake_pool_program: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        mut, address = get_stake_pool_address(&config)?
    )]
    pub stake_pool: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    pub new_staker: AccountInfo<'info>,
    #[account(mut, address = get_config_admin(&config)?)]
    pub admin: Signer<'info>,
}

/// Note this function can only be called once by the Steward, as it will lose it's authority
/// to the new staker. This can be undone by calling `spl_stake_pool::instruction::set_staker`
/// manually with the manager or new staker as a signer.
pub fn set_staker_handler(ctx: Context<SetStaker>) -> Result<()> {
    invoke_signed(
        &spl_stake_pool::instruction::set_staker(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.state_account.key(),
            &ctx.accounts.new_staker.key(),
        ),
        &[
            ctx.accounts.stake_pool.to_account_info(),
            ctx.accounts.state_account.to_account_info(),
            ctx.accounts.new_staker.to_account_info(),
        ],
        &[&[
            StewardStateAccount::SEED,
            &ctx.accounts.config.key().to_bytes(),
            &[ctx.bumps.state_account],
        ]],
    )?;
    Ok(())
}
