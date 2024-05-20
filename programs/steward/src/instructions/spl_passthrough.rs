// All spl-stake-pool instructions which need to be signed by the staker keypair.
// Nearly all accounts are passed through to a spl-stake-pool instruction, which does its own
// checks on the validity of each account. All that's important for these instructions to check
// is that the config, stake pool address, staker, signer, and sometimes state account match up.
// Otherwise these instructions are intented to be minimally restrictive.

use crate::errors::StewardError;
use crate::state::{Config, Staker};
use crate::utils::{get_config_authority, get_stake_pool, StakePool, ValidatorList};
use crate::StewardStateAccount;
use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke_signed, stake, sysvar, vote};
use spl_stake_pool::instruction::PreferredValidatorType;
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
    /// CHECK: CPI program
    #[account(
        address = spl_stake_pool::ID
    )]
    pub stake_pool_program: AccountInfo<'info>,
    #[account(
        address = get_stake_pool(&config)?
    )]
    pub stake_pool: Account<'info, StakePool>,
    #[account(
        seeds = [Staker::SEED, config.key().as_ref()],
        bump = staker.bump
    )]
    pub staker: Account<'info, Staker>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub reserve_stake: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub withdraw_authority: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub validator_list: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
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
    #[account(mut, address = get_config_authority(&config)?)]
    pub signer: Signer<'info>,
}

pub fn add_validator_to_pool_handler(
    ctx: Context<AddValidatorToPool>,
    validator_seed: Option<u32>,
) -> Result<()> {
    invoke_signed(
        &spl_stake_pool::instruction::add_validator_to_pool(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.staker.key(),
            &ctx.accounts.reserve_stake.key(),
            &ctx.accounts.withdraw_authority.key(),
            &ctx.accounts.validator_list.key(),
            &ctx.accounts.stake_account.key(),
            &ctx.accounts.vote_account.key(),
            NonZeroU32::new(validator_seed.unwrap_or_default()),
        ),
        &[
            ctx.accounts.stake_pool.to_account_info(),
            ctx.accounts.staker.to_account_info(),
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
            Staker::SEED,
            &ctx.accounts.config.key().to_bytes(),
            &[ctx.accounts.staker.bump],
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
    pub steward_state: AccountLoader<'info, StewardStateAccount>,

    /// CHECK: CPI program
    #[account(
        address = spl_stake_pool::ID
    )]
    pub stake_pool_program: AccountInfo<'info>,
    #[account(
        address = get_stake_pool(&config)?
    )]
    pub stake_pool: Account<'info, StakePool>,
    #[account(
        seeds = [Staker::SEED, config.key().as_ref()],
        bump = staker.bump
    )]
    pub staker: Account<'info, Staker>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub withdraw_authority: AccountInfo<'info>,
    pub validator_list: Account<'info, ValidatorList>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub stake_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub transient_stake_account: AccountInfo<'info>,
    pub clock: Sysvar<'info, Clock>,
    pub system_program: Program<'info, System>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = stake::program::ID)]
    pub stake_program: AccountInfo<'info>,
    #[account(mut, address = get_config_authority(&config)?)]
    pub signer: Signer<'info>,
}

pub fn remove_validator_from_pool_handler(
    ctx: Context<RemoveValidatorFromPool>,
    validator_list_index: usize,
) -> Result<()> {
    let mut state_account = ctx.accounts.steward_state.load_mut()?;

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
            ctx.accounts.withdraw_authority.to_owned(),
            ctx.accounts.validator_list.to_account_info(),
            ctx.accounts.stake_account.to_account_info(),
            ctx.accounts.transient_stake_account.to_account_info(),
            ctx.accounts.clock.to_account_info(),
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

#[derive(Accounts)]
pub struct SetPreferredValidator<'info> {
    pub config: AccountLoader<'info, Config>,
    /// CHECK: CPI program
    #[account(
        address = spl_stake_pool::ID
    )]
    pub stake_pool_program: AccountInfo<'info>,
    #[account(
        address = get_stake_pool(&config)?
    )]
    pub stake_pool: Account<'info, StakePool>,
    #[account(
        seeds = [Staker::SEED, config.key().as_ref()],
        bump = staker.bump
    )]
    pub staker: Account<'info, Staker>,
    #[account(address = stake_pool.validator_list)]
    pub validator_list: Account<'info, ValidatorList>,
    #[account(mut, address = get_config_authority(&config)?)]
    pub signer: Signer<'info>,
}

pub fn set_preferred_validator_handler(
    ctx: Context<SetPreferredValidator>,
    validator_type: PreferredValidatorType,
    validator: Option<Pubkey>,
) -> Result<()> {
    invoke_signed(
        &spl_stake_pool::instruction::set_preferred_validator(
            ctx.accounts.stake_pool_program.key,
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.staker.key(),
            &ctx.accounts.validator_list.key(),
            validator_type,
            validator,
        ),
        &[
            ctx.accounts.stake_pool.to_account_info(),
            ctx.accounts.staker.to_account_info(),
            ctx.accounts.validator_list.to_account_info(),
        ],
        &[&[
            Staker::SEED,
            &ctx.accounts.config.key().to_bytes(),
            &[ctx.accounts.staker.bump],
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
    pub steward_state: AccountLoader<'info, StewardStateAccount>,
    #[account(
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        bump
    )]
    pub validator_history: AccountLoader<'info, ValidatorHistory>,
    /// CHECK: CPI program
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
    pub withdraw_authority: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub validator_list: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
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
    #[account(mut, address = get_config_authority(&config)?)]
    pub signer: Signer<'info>,
}

pub fn increase_validator_stake_handler(
    ctx: Context<IncreaseValidatorStake>,
    lamports: u64,
    transient_seed: u64,
) -> Result<()> {
    let validator_history = ctx.accounts.validator_history.load()?;
    let mut state_account = ctx.accounts.steward_state.load_mut()?;
    state_account
        .state
        .validator_lamport_balances
        .get_mut(validator_history.index as usize)
        .ok_or(StewardError::ValidatorIndexOutOfBounds)?
        .checked_add(lamports)
        .ok_or(StewardError::ArithmeticError)?;
    invoke_signed(
        &spl_stake_pool::instruction::increase_validator_stake(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.staker.key(),
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
            ctx.accounts.staker.to_account_info(),
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
            Staker::SEED,
            &ctx.accounts.config.key().to_bytes(),
            &[ctx.accounts.staker.bump],
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
    pub steward_state: AccountLoader<'info, StewardStateAccount>,
    #[account(
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        bump
    )]
    pub validator_history: AccountLoader<'info, ValidatorHistory>,
    /// CHECK: Used to derive validator history seed
    #[account(
        owner = vote::program::ID
    )]
    pub vote_account: AccountInfo<'info>,
    /// CHECK: CPI program
    #[account(
        address = spl_stake_pool::ID
    )]
    pub stake_pool_program: AccountInfo<'info>,
    #[account(
        address = get_stake_pool(&config)?
    )]
    pub stake_pool: Account<'info, StakePool>,
    #[account(
        mut,
        seeds = [Staker::SEED, config.key().as_ref()],
        bump = staker.bump
    )]
    pub staker: Account<'info, Staker>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub withdraw_authority: AccountInfo<'info>,
    #[account(mut, address = stake_pool.validator_list)]
    pub validator_list: Account<'info, ValidatorList>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut, address = stake_pool.reserve_stake)]
    pub reserve_stake: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub stake_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(mut)]
    pub transient_stake_account: AccountInfo<'info>,
    pub clock: Sysvar<'info, Clock>,
    pub rent: Sysvar<'info, Rent>,
    #[account(address = sysvar::stake_history::ID)]
    pub system_program: Program<'info, System>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = stake::program::ID)]
    pub stake_program: AccountInfo<'info>,
    #[account(mut, address = get_config_authority(&config)?)]
    pub signer: Signer<'info>,
}

pub fn decrease_validator_stake_handler(
    ctx: Context<DecreaseValidatorStake>,
    lamports: u64,
    transient_seed: u64,
) -> Result<()> {
    let validator_history = ctx.accounts.validator_history.load()?;
    let mut state_account = ctx.accounts.steward_state.load_mut()?;
    state_account
        .state
        .validator_lamport_balances
        .get_mut(validator_history.index as usize)
        .ok_or(StewardError::ValidatorIndexOutOfBounds)?
        .checked_sub(lamports)
        .ok_or(StewardError::ArithmeticError)?;

    invoke_signed(
        &spl_stake_pool::instruction::decrease_validator_stake_with_reserve(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.staker.key(),
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
            ctx.accounts.staker.to_account_info(),
            ctx.accounts.withdraw_authority.to_owned(),
            ctx.accounts.validator_list.to_account_info(),
            ctx.accounts.stake_account.to_account_info(),
            ctx.accounts.transient_stake_account.to_account_info(),
            ctx.accounts.clock.to_account_info(),
            ctx.accounts.rent.to_account_info(),
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

#[derive(Accounts)]
pub struct IncreaseAdditionalValidatorStake<'info> {
    pub config: AccountLoader<'info, Config>,
    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub steward_state: AccountLoader<'info, StewardStateAccount>,
    #[account(
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        bump
    )]
    pub validator_history: AccountLoader<'info, ValidatorHistory>,
    /// CHECK: CPI program
    #[account(
        address = spl_stake_pool::ID
    )]
    pub stake_pool_program: AccountInfo<'info>,
    #[account(
        address = get_stake_pool(&config)?
    )]
    pub stake_pool: Account<'info, StakePool>,
    #[account(
        seeds = [Staker::SEED, config.key().as_ref()],
        bump = staker.bump
    )]
    pub staker: Account<'info, Staker>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub withdraw_authority: AccountInfo<'info>,
    #[account(address = stake_pool.validator_list)]
    pub validator_list: Account<'info, ValidatorList>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub reserve_stake: AccountInfo<'info>,
    // Uninitialized
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub ephemeral_stake_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
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
    #[account(mut, address = get_config_authority(&config)?)]
    pub signer: Signer<'info>,
}

pub fn increase_additional_validator_stake_handler(
    ctx: Context<IncreaseAdditionalValidatorStake>,
    lamports: u64,
    transient_seed: u64,
    ephemeral_seed: u64,
) -> Result<()> {
    let validator_history = ctx.accounts.validator_history.load()?;
    let mut state_account = ctx.accounts.steward_state.load_mut()?;
    state_account
        .state
        .validator_lamport_balances
        .get_mut(validator_history.index as usize)
        .ok_or(StewardError::ValidatorIndexOutOfBounds)?
        .checked_add(lamports)
        .ok_or(StewardError::ArithmeticError)?;

    invoke_signed(
        &spl_stake_pool::instruction::increase_additional_validator_stake(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.staker.key(),
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
            ctx.accounts.staker.to_account_info(),
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
            Staker::SEED,
            &ctx.accounts.config.key().to_bytes(),
            &[ctx.accounts.staker.bump],
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
    pub steward_state: AccountLoader<'info, StewardStateAccount>,
    #[account(
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
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
    #[account(
        address = get_stake_pool(&config)?
    )]
    pub stake_pool: Account<'info, StakePool>,
    #[account(
        seeds = [Staker::SEED, config.key().as_ref()],
        bump = staker.bump
    )]
    pub staker: Account<'info, Staker>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub withdraw_authority: AccountInfo<'info>,
    #[account(address = stake_pool.validator_list)]
    pub validator_list: Account<'info, ValidatorList>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub reserve_stake: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub stake_account: AccountInfo<'info>,
    // Uninitialized
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub ephemeral_stake_account: AccountInfo<'info>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub transient_stake_account: AccountInfo<'info>,
    pub clock: Sysvar<'info, Clock>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = sysvar::stake_history::ID)]
    pub stake_history: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = stake::program::ID)]
    pub stake_program: AccountInfo<'info>,
    #[account(mut, address = get_config_authority(&config)?)]
    pub signer: Signer<'info>,
}

pub fn decrease_additional_validator_stake_handler(
    ctx: Context<DecreaseAdditionalValidatorStake>,
    lamports: u64,
    transient_seed: u64,
    ephemeral_seed: u64,
) -> Result<()> {
    let validator_history = ctx.accounts.validator_history.load()?;
    let mut state_account = ctx.accounts.steward_state.load_mut()?;
    state_account
        .state
        .validator_lamport_balances
        .get_mut(validator_history.index as usize)
        .ok_or(StewardError::ValidatorIndexOutOfBounds)?
        .checked_sub(lamports)
        .ok_or(StewardError::ArithmeticError)?;

    invoke_signed(
        &spl_stake_pool::instruction::decrease_additional_validator_stake(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.staker.key(),
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
            ctx.accounts.staker.to_account_info(),
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
            Staker::SEED,
            &ctx.accounts.config.key().to_bytes(),
            &[ctx.accounts.staker.bump],
        ]],
    )?;
    Ok(())
}

#[derive(Accounts)]
pub struct SetStaker<'info> {
    pub config: AccountLoader<'info, Config>,
    /// CHECK: CPI program
    #[account(
        address = spl_stake_pool::ID
    )]
    pub stake_pool_program: AccountInfo<'info>,
    #[account(
        address = get_stake_pool(&config)?
    )]
    pub stake_pool: Account<'info, StakePool>,
    #[account(
        seeds = [Staker::SEED, config.key().as_ref()],
        bump = staker.bump
    )]
    pub staker: Account<'info, Staker>,
    /// CHECK: passing through, checks are done by spl-stake-pool
    pub new_staker: AccountInfo<'info>,
    #[account(mut, address = get_config_authority(&config)?)]
    pub signer: Signer<'info>,
}

pub fn set_staker_handler(ctx: Context<SetStaker>) -> Result<()> {
    invoke_signed(
        &spl_stake_pool::instruction::set_staker(
            &ctx.accounts.stake_pool_program.key(),
            &ctx.accounts.stake_pool.key(),
            &ctx.accounts.staker.key(),
            &ctx.accounts.new_staker.key(),
        ),
        &[
            ctx.accounts.stake_pool.to_account_info(),
            ctx.accounts.staker.to_account_info(),
            ctx.accounts.new_staker.to_account_info(),
        ],
        &[&[Staker::SEED, &ctx.accounts.config.key().to_bytes()]],
    )?;
    Ok(())
}
