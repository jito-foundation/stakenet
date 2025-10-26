use std::num::NonZeroU32;

use anchor_lang::{
    prelude::*,
    solana_program::{
        borsh1::try_from_slice_unchecked,
        program::invoke_signed,
        stake::{self, state::StakeStateV2, tools::get_minimum_delegation},
        system_program, sysvar,
    },
};
use borsh::BorshDeserialize;
use spl_stake_pool::{
    find_stake_program_address, find_transient_stake_program_address, minimum_delegation,
};

use crate::{
    constants::STAKE_POOL_WITHDRAW_SEED,
    directed_delegation::{RebalanceType, UnstakeState},
    errors::StewardError,
    events::{DecreaseComponents, RebalanceEvent, RebalanceTypeTag},
    maybe_transition,
    stake_pool_utils::deserialize_stake_pool,
    state::directed_stake::DirectedStakeMeta,
    utils::{
        get_stake_pool_address, get_transient_stake_seed_at_index, get_validator_list_length,
        get_validator_stake_info_at_index, state_checks,
    },
    Config, StewardStateAccount, StewardStateEnum, REBALANCE_DIRECTED_COMPLETE,
};
#[derive(Accounts)]
#[instruction(validator_list_index: u64)]
pub struct RebalanceDirected<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccount>,

    #[account(
        mut,
        seeds = [DirectedStakeMeta::SEED, config.key().as_ref()],
        bump
    )]
    pub directed_stake_meta: AccountLoader<'info, DirectedStakeMeta>,

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

pub fn handler(
    ctx: Context<RebalanceDirected>,
    directed_stake_meta_index: usize,
    validator_list_index: usize,
) -> Result<()> {
    let mut directed_stake_meta = ctx.accounts.directed_stake_meta.load_mut()?;
    let validator_list = &ctx.accounts.validator_list;
    let clock = Clock::get()?;
    let epoch_schedule = EpochSchedule::get()?;
    let config = ctx.accounts.config.load()?;

    // Vote pubkeys from directed stake meta entry and validator list must match
    // if the directed stake meta has valid entries
    let vote_pubkey_from_directed_stake_meta =
        directed_stake_meta.targets[directed_stake_meta_index].vote_pubkey;
    let vote_pubkey_from_validator_list =
        get_validator_stake_info_at_index(validator_list, validator_list_index as usize)?;

    // An empty meta means there are no directed stake targets, automatically transition to Idle
    if directed_stake_meta.total_stake_targets > 0 {
        msg!(
            "vote_pubkey_from_directed_stake_meta: {}",
            vote_pubkey_from_directed_stake_meta
        );
        msg!(
            "vote_pubkey_from_validator_list: {}",
            vote_pubkey_from_validator_list.vote_account_address
        );
        require!(
            vote_pubkey_from_directed_stake_meta
                == vote_pubkey_from_validator_list.vote_account_address,
            StewardError::DirectedStakeVoteAccountMismatch
        );
    }

    let rebalance_type: RebalanceType;
    let transient_seed: u64 =
        get_transient_stake_seed_at_index(&validator_list, validator_list_index as usize)?;
    {
        let mut state_account = ctx.accounts.state_account.load_mut()?;

        let current_epoch = clock.epoch;
        if (current_epoch > state_account.state.current_epoch
            || state_account.state.num_pool_validators == 0)
            && !state_account.state.has_flag(REBALANCE_DIRECTED_COMPLETE)
        {
            state_account.state.reset_state_for_new_cycle(
                clock.epoch,
                clock.slot,
                config.parameters.num_epochs_between_scoring,
            )?;

            let num_pool_validators = get_validator_list_length(validator_list)?;
            require!(
                num_pool_validators as usize
                    == state_account.state.num_pool_validators as usize
                        + state_account.state.validators_added as usize,
                StewardError::ListStateMismatch
            );
            state_account.state.num_pool_validators = num_pool_validators as u64;
            state_account.state.validators_added = 0;
            msg!("Setting num pool validators: {}", num_pool_validators);
        }

        // If there are no more targets to rebalance, set the flag to REBALANCE_DIRECTED_COMPLETE
        // This will cause the state to transition to Idle
        if directed_stake_meta.all_targets_rebalanced_for_epoch(clock.epoch) {
            state_account.state.set_flag(REBALANCE_DIRECTED_COMPLETE);
        }

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
            Some(StewardStateEnum::RebalanceDirected),
        )?;

        let minimum_delegation = minimum_delegation(get_minimum_delegation()?);
        let stake_rent = Rent::get()?.minimum_balance(StakeStateV2::size_of());

        rebalance_type = {
            let stake_pool_lamports_with_fixed_cost =
                deserialize_stake_pool(&ctx.accounts.stake_pool)?.total_lamports;
            let reserve_lamports_with_rent = ctx.accounts.reserve_stake.lamports();

            msg!("reserve_lamports_with_rent: {}", reserve_lamports_with_rent);

            // Use directed delegation logic instead of regular rebalance
            use crate::directed_delegation::{
                decrease_stake_calculation, increase_stake_calculation,
            };

            let unstake_state = UnstakeState {
                directed_unstake_total: state_account.state.directed_unstake_total,
            };

            let directed_unstake_cap_lamports = stake_pool_lamports_with_fixed_cost
                .saturating_mul(config.parameters.directed_stake_unstake_cap_bps as u64)
                .saturating_div(10_000);

            let undirected_tvl_lamports = stake_pool_lamports_with_fixed_cost
                .saturating_sub(directed_stake_meta.total_staked_lamports());

            let undirected_floor_cap =
                undirected_tvl_lamports < config.parameters.undirected_stake_floor_lamports();

            msg!(
                "config parameters undirected stake floor lamports {}",
                &config
                    .parameters
                    .undirected_stake_floor_lamports()
                    .to_string()
            );
            msg!(
                "directed_unstake_cap_bps: {}",
                config.parameters.directed_stake_unstake_cap_bps
            );
            msg!(
                "directed_unstake_cap_lamports: {}",
                directed_unstake_cap_lamports
            );
            msg!("undirected_tvl_lamports: {}", undirected_tvl_lamports);
            msg!("undirected_floor_cap: {}", undirected_floor_cap);

            // Hmm this could be better
            let staked_lamports_at_stake_meta_index = directed_stake_meta
                .get_total_staked_lamports(&vote_pubkey_from_directed_stake_meta)
                .unwrap_or(0);

            // Try decrease first, then increase (if undirected floor cap does not apply)
            let decrease_result = decrease_stake_calculation(
                &state_account.state,
                &directed_stake_meta,
                directed_stake_meta_index,
                unstake_state,
                staked_lamports_at_stake_meta_index,
                stake_pool_lamports_with_fixed_cost,
                minimum_delegation,
                stake_rent,
                directed_unstake_cap_lamports,
            );

            match decrease_result {
                Ok(RebalanceType::Decrease(_)) => decrease_result,
                _ => increase_stake_calculation(
                    &state_account.state,
                    &directed_stake_meta,
                    directed_stake_meta_index,
                    staked_lamports_at_stake_meta_index,
                    stake_pool_lamports_with_fixed_cost,
                    reserve_lamports_with_rent,
                    minimum_delegation,
                    stake_rent,
                    undirected_floor_cap,
                ),
            }?
        };
    }

    msg!("rebalance_type: {:?}", rebalance_type);

    match rebalance_type.clone() {
        RebalanceType::Decrease(decrease_components) => {
            if decrease_components.directed_unstake_lamports > 0 {
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
                        decrease_components.directed_unstake_lamports,
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
                msg!("decrease_validator_stake_with_reserve successful");
                directed_stake_meta.subtract_from_total_staked_lamports(
                    &ctx.accounts.vote_account.key(),
                    decrease_components.directed_unstake_lamports,
                    clock.epoch,
                );
            } else {
                msg!("Decrease component is zero.");
            }
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
            msg!("increase_validator_stake successful");
            directed_stake_meta.add_to_total_staked_lamports(
                &ctx.accounts.vote_account.key(),
                lamports,
                clock.epoch,
            );
        }
        RebalanceType::None => {
            msg!("RebalanceType::None");
            directed_stake_meta
                .update_staked_last_updated_epoch(&ctx.accounts.vote_account.key(), clock.epoch);
        }
    }

    let mut state_account = ctx.accounts.state_account.load_mut()?;

    if let RebalanceType::Decrease(decrease_components) = &rebalance_type {
        state_account.state.directed_unstake_total = state_account
            .state
            .directed_unstake_total
            .saturating_add(decrease_components.directed_unstake_lamports);
    }

    if directed_stake_meta.all_targets_rebalanced_for_epoch(clock.epoch) {
        state_account.state.set_flag(REBALANCE_DIRECTED_COMPLETE);
    }

    {
        emit!(rebalance_to_event(
            ctx.accounts.vote_account.key(),
            clock.epoch as u16,
            rebalance_type
        ));
    }

    if let Some(event) = maybe_transition(
        &mut state_account.state,
        &clock,
        &config.parameters,
        &epoch_schedule,
    )? {
        emit!(event);
        return Ok(());
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
