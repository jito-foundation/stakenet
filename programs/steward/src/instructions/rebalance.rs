use std::num::NonZeroU32;

use anchor_lang::{
    idl::*,
    prelude::*,
    solana_program::{
        program::invoke_signed,
        stake::{self, tools::get_minimum_delegation},
        system_program, sysvar, vote,
    },
};
use borsh::BorshDeserialize;
use spl_pod::solana_program::{borsh1::try_from_slice_unchecked, stake::state::StakeStateV2};
use spl_stake_pool::{
    find_stake_program_address, find_transient_stake_program_address, minimum_delegation,
    state::ValidatorListHeader,
};
use validator_history::ValidatorHistory;

use crate::{
    constants::STAKE_POOL_WITHDRAW_SEED,
    delegation::RebalanceType,
    errors::StewardError,
    events::{DecreaseComponents, RebalanceEvent, RebalanceTypeTag},
    maybe_transition,
    utils::{
        deserialize_stake_pool, get_stake_pool_address, get_validator_stake_info_at_index,
        state_checks,
    },
    Config, StewardStateAccount, StewardStateEnum,
};

#[derive(Accounts)]
#[instruction(validator_list_index: u64)]
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

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(address = get_stake_pool_address(&config)?)]
    pub stake_pool: AccountInfo<'info>,

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
    #[account(
        mut,
        address = deserialize_stake_pool(&stake_pool)?.validator_list
    )]
    pub validator_list: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    #[account(
        mut,
        address = deserialize_stake_pool(&stake_pool)?.reserve_stake
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

    /// CHECK: We check the owning program in the handler
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
}

pub fn handler(ctx: Context<Rebalance>, validator_list_index: usize) -> Result<()> {
    let validator_history = ctx.accounts.validator_history.load()?;
    let validator_list = &ctx.accounts.validator_list;
    let clock = Clock::get()?;
    let epoch_schedule = EpochSchedule::get()?;
    let config = ctx.accounts.config.load()?;

    let rebalance_type: RebalanceType;
    let transient_seed: u64;

    {
        let mut state_account = ctx.accounts.state_account.load_mut()?;

        // Transitions to Idle before doing rebalance if RESET_TO_IDLE is set
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
            Some(StewardStateEnum::Rebalance),
        )?;

        let validator_stake_info =
            get_validator_stake_info_at_index(validator_list, validator_list_index)?;
        require!(
            validator_stake_info.vote_account_address == validator_history.vote_account,
            StewardError::ValidatorNotInList
        );

        if ctx.accounts.vote_account.owner != &vote::program::ID
            && !state_account
                .state
                .validators_to_remove
                .get(validator_list_index)?
        {
            return Err(StewardError::ValidatorNeedsToBeMarkedForRemoval.into());
        }

        transient_seed = u64::from(validator_stake_info.transient_seed_suffix);

        let stake_account_data = &mut ctx.accounts.stake_account.data.borrow();
        let stake_state = try_from_slice_unchecked::<StakeStateV2>(stake_account_data)?;
        let stake_account_active_lamports = match stake_state {
            StakeStateV2::Stake(_meta, stake, _stake_flags) => stake.delegation.stake,
            _ => return Err(StewardError::StakeStateIsNotStake.into()),
        };

        let minimum_delegation = minimum_delegation(get_minimum_delegation()?);
        let stake_rent = Rent::get()?.minimum_balance(StakeStateV2::size_of());

        rebalance_type = {
            let validator_list_data = &mut ctx.accounts.validator_list.try_borrow_mut_data()?;
            let (_, validator_list) = ValidatorListHeader::deserialize_vec(validator_list_data)?;

            let stake_pool_lamports_with_fixed_cost =
                deserialize_stake_pool(&ctx.accounts.stake_pool)?.total_lamports;
            let reserve_lamports_with_rent = ctx.accounts.reserve_stake.lamports();

            state_account.state.rebalance(
                clock.epoch,
                validator_list_index,
                &validator_list,
                stake_pool_lamports_with_fixed_cost,
                reserve_lamports_with_rent,
                stake_account_active_lamports,
                minimum_delegation,
                stake_rent,
                &config.parameters,
            )?
        };
    }

    match rebalance_type.clone() {
        RebalanceType::Decrease(decrease_components) => {
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
                    decrease_components.total_unstake_lamports,
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
        }
        RebalanceType::Increase(lamports) => {
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
        }
        RebalanceType::None => {}
    }

    {
        let mut state_account = ctx.accounts.state_account.load_mut()?;

        emit!(rebalance_to_event(
            ctx.accounts.vote_account.key(),
            clock.epoch as u16,
            rebalance_type
        ));

        if let Some(event) = maybe_transition(
            &mut state_account.state,
            &clock,
            &config.parameters,
            &epoch_schedule,
        )? {
            emit!(event);
        }
    }

    Ok(())
}

fn rebalance_to_event(
    vote_account: Pubkey,
    epoch: u16,
    rebalance_type: RebalanceType,
) -> RebalanceEvent {
    match rebalance_type {
        RebalanceType::None => RebalanceEvent {
            vote_account,
            epoch,
            rebalance_type_tag: RebalanceTypeTag::None,
            increase_lamports: 0,
            decrease_components: DecreaseComponents::default(),
        },
        RebalanceType::Increase(lamports) => RebalanceEvent {
            vote_account,
            epoch,
            rebalance_type_tag: RebalanceTypeTag::Increase,
            increase_lamports: lamports,
            decrease_components: DecreaseComponents::default(),
        },
        RebalanceType::Decrease(decrease_components) => RebalanceEvent {
            vote_account,
            epoch,
            rebalance_type_tag: RebalanceTypeTag::Decrease,
            increase_lamports: 0,
            decrease_components,
        },
    }
}
