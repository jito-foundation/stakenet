use std::num::NonZeroU32;

use anchor_lang::{
    prelude::*,
    solana_program::{
        program::invoke_signed,
        stake::{self, tools::get_minimum_delegation},
        system_program, sysvar, vote,
    },
};
use spl_pod::solana_program::stake::state::StakeStateV2;
use spl_stake_pool::{
    find_stake_program_address, find_transient_stake_program_address, minimum_delegation,
    state::ValidatorListHeader,
};
use validator_history::ValidatorHistory;

use crate::{
    constants::STAKE_POOL_WITHDRAW_SEED,
    delegation::RebalanceType,
    errors::StewardError,
    maybe_transition_and_emit,
    utils::{get_stake_pool, get_validator_stake_info_at_index, StakePool},
    Config, Staker, StewardStateAccount,
};

#[derive(Accounts)]
#[instruction(validator_list_index: usize)]
pub struct Rebalance<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,

    #[account(
        seeds = [ValidatorHistory::SEED, vote_account.key().as_ref()],
        seeds::program = validator_history::id(),
        bump
    )]
    pub validator_history: AccountLoader<'info, ValidatorHistory>,

    /// CHECK: CPI program
    #[account(address = spl_stake_pool::ID)]
    pub stake_pool_program: AccountInfo<'info>,

    #[account(address = get_stake_pool(&config)?)]
    pub stake_pool: Account<'info, StakePool>,

    #[account(
        mut,
        seeds = [Staker::SEED, config.key().as_ref()],
        bump = staker.bump
    )]
    pub staker: Account<'info, Staker>,

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
    #[account(
        mut,
        address = stake_pool.validator_list
    )]
    pub validator_list: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        mut,
        address = stake_pool.reserve_stake
    )]
    pub reserve_stake: AccountInfo<'info>,

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
    #[account(owner = vote::program::ID)]
    pub vote_account: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = sysvar::clock::ID)]
    pub clock: AccountInfo<'info>,

    #[account(address = sysvar::rent::ID)]
    pub rent: Sysvar<'info, Rent>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = sysvar::stake_history::ID)]
    pub stake_history: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = stake::config::ID)]
    pub stake_config: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = system_program::ID)]
    pub system_program: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = stake::program::ID)]
    pub stake_program: AccountInfo<'info>,

    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handler(ctx: Context<Rebalance>, validator_list_index: usize) -> Result<()> {
    let config = ctx.accounts.config.load()?;
    let mut state_account = ctx.accounts.state_account.load_mut()?;
    let validator_history = ctx.accounts.validator_history.load()?;
    let validator_list = &ctx.accounts.validator_list;
    let clock = Clock::get()?;
    let epoch_schedule = EpochSchedule::get()?;

    let validator_stake_info =
        get_validator_stake_info_at_index(validator_list, validator_list_index)?;
    require!(
        validator_stake_info.vote_account_address == validator_history.vote_account,
        StewardError::ValidatorNotInList
    );
    let transient_seed = u64::from(validator_stake_info.transient_seed_suffix);

    if config.is_paused() {
        return Err(StewardError::StateMachinePaused.into());
    }

    let minimum_delegation = minimum_delegation(get_minimum_delegation()?);
    let stake_rent = Rent::get()?.minimum_balance(StakeStateV2::size_of());

    let result = {
        let validator_list_data = &mut ctx.accounts.validator_list.try_borrow_mut_data()?;
        let (_, validator_list) = ValidatorListHeader::deserialize_vec(validator_list_data)?;

        let stake_pool_lamports_with_fixed_cost = ctx.accounts.stake_pool.total_lamports;
        let reserve_lamports_with_rent = ctx.accounts.reserve_stake.lamports();

        state_account.state.rebalance(
            clock.epoch,
            validator_list_index,
            &validator_list,
            stake_pool_lamports_with_fixed_cost,
            reserve_lamports_with_rent,
            minimum_delegation,
            stake_rent,
            &config.parameters,
        )?
    };

    match result {
        RebalanceType::Decrease(decrease_components) => {
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
                    decrease_components.total_unstake_lamports,
                    transient_seed,
                ),
                &[
                    ctx.accounts.stake_pool.to_account_info(),
                    ctx.accounts.staker.to_account_info(),
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
                    Staker::SEED,
                    &ctx.accounts.config.key().to_bytes(),
                    &[ctx.accounts.staker.bump],
                ]],
            )?;
        }
        RebalanceType::Increase(lamports) => {
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
        }
        RebalanceType::None => {}
    }

    maybe_transition_and_emit(
        &mut state_account.state,
        &clock,
        &config.parameters,
        &epoch_schedule,
    )?;

    Ok(())
}
