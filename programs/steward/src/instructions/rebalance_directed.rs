use crate::directed_delegation::{decrease_stake_calculation, increase_stake_calculation};
use crate::{
    constants::{LAMPORT_BALANCE_DEFAULT, STAKE_POOL_WITHDRAW_SEED},
    directed_delegation::{RebalanceType, UnstakeState},
    errors::StewardError,
    events::{DirectedRebalanceEvent, RebalanceTypeTag},
    maybe_transition,
    stake_pool_utils::deserialize_stake_pool,
    state::directed_stake::DirectedStakeMeta,
    utils::{
        get_stake_pool_address, get_transient_stake_seed_at_index_from_big_vec,
        stake_lamports_at_validator_list_index, state_checks, vote_pubkey_at_validator_list_index,
    },
    Config, StewardStateAccount, StewardStateAccountV2, StewardStateEnum,
    REBALANCE_DIRECTED_COMPLETE,
};
use anchor_lang::{
    prelude::*,
    solana_program::{
        program::invoke_signed,
        stake::{self, state::StakeStateV2, tools::get_minimum_delegation},
        system_program, sysvar,
    },
};
use spl_stake_pool::{minimum_delegation, state::ValidatorListHeader};
#[derive(Accounts)]
pub struct RebalanceDirected<'info> {
    pub config: AccountLoader<'info, Config>,

    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump
    )]
    pub state_account: AccountLoader<'info, StewardStateAccountV2>,

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
    /// Account may not exist yet so no owner check done
    /// TODO: PDA Check in handler
    #[account(mut)]
    pub stake_account: AccountInfo<'info>,

    /// CHECK: passing through, checks are done by spl-stake-pool
    /// Account may not exist yet so no owner check done
    /// TODO: PDA Check in handler
    #[account(mut)]
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

pub fn adjust_directed_stake_for_deposits_and_withdrawals(
    target_total_staked_lamports: u64,
    validator_list_index: usize,
    directed_stake_meta_index: usize,
    directed_stake_meta: &mut DirectedStakeMeta,
    state_account: &mut StewardStateAccountV2,
) -> Result<()> {
    let steward_state_total_lamports =
        state_account.state.validator_lamport_balances[validator_list_index];
    let directed_stake_target_lamports =
        directed_stake_meta.targets[directed_stake_meta_index].total_target_lamports;
    let directed_stake_applied_lamports =
        directed_stake_meta.targets[directed_stake_meta_index].total_staked_lamports;
    if target_total_staked_lamports < steward_state_total_lamports {
        let withdrawal_lamports =
            steward_state_total_lamports.saturating_sub(target_total_staked_lamports);
        // If the withdrawal lamports is greated than the applied directed stake, then we need to roll-over the remainder
        // to the undirected stake
        if withdrawal_lamports > directed_stake_applied_lamports {
            let remainder = withdrawal_lamports.saturating_sub(directed_stake_applied_lamports);
            // Subtract from directed stake
            directed_stake_meta.targets[directed_stake_meta_index].total_staked_lamports = 0;
            directed_stake_meta.directed_stake_lamports[validator_list_index] = 0;
            // Implicitly subtract from undirected stake by subtracting from steward state total lamports
            state_account.state.validator_lamport_balances[validator_list_index] =
                state_account.state.validator_lamport_balances[validator_list_index]
                    .saturating_sub(remainder);
        } else {
            directed_stake_meta.subtract_from_total_staked_lamports(
                directed_stake_meta_index,
                withdrawal_lamports,
            );
            directed_stake_meta.directed_stake_lamports[validator_list_index] = directed_stake_meta
                .directed_stake_lamports[validator_list_index]
                .saturating_sub(withdrawal_lamports);
        }
    } else if target_total_staked_lamports > steward_state_total_lamports
        && (directed_stake_applied_lamports < directed_stake_target_lamports)
    {
        let directed_deficit_lamports =
            directed_stake_target_lamports.saturating_sub(directed_stake_applied_lamports);
        let deposit_lamports =
            target_total_staked_lamports.saturating_sub(steward_state_total_lamports);
        let increase_lamports = directed_deficit_lamports.min(deposit_lamports);
        directed_stake_meta
            .add_to_total_staked_lamports(directed_stake_meta_index, increase_lamports);
        directed_stake_meta.directed_stake_lamports[validator_list_index] = directed_stake_meta
            .directed_stake_lamports[validator_list_index]
            .saturating_add(increase_lamports);
    }
    Ok(())
}

pub fn handler(ctx: Context<RebalanceDirected>, directed_stake_meta_index: usize) -> Result<()> {
    let mut directed_stake_meta = ctx.accounts.directed_stake_meta.load_mut()?;
    let clock = Clock::get()?;
    let epoch_schedule = EpochSchedule::get()?;
    let config = ctx.accounts.config.load()?;

    // Vote pubkeys from directed stake meta entry and validator list must match
    // if the directed stake meta has valid entries
    let vote_pubkey_from_directed_stake_meta =
        directed_stake_meta.targets[directed_stake_meta_index].vote_pubkey;

    // Now we need to check if the vote_pubkey from above matches the vote_account
    if vote_pubkey_from_directed_stake_meta != ctx.accounts.vote_account.key() {
        return Err(StewardError::DirectedStakeVoteAccountMismatch.into());
    }

    let mut transient_seed = 0;
    let mut validator_list_index = 0;
    let mut found = false;
    {
        let mut validator_list_data = ctx.accounts.validator_list.try_borrow_mut_data()?;
        let (header, validator_list) =
            ValidatorListHeader::deserialize_vec(&mut validator_list_data)?;
        require!(
            header.account_type == spl_stake_pool::state::AccountType::ValidatorList,
            StewardError::ValidatorListTypeMismatch
        );
        let validator_list_size = validator_list.len() as usize;
        let mut state_account = ctx.accounts.state_account.load_mut()?;
        for index in 0..validator_list_size {
            let vote_pubkey = vote_pubkey_at_validator_list_index(&validator_list, index)?;
            if vote_pubkey == vote_pubkey_from_directed_stake_meta {
                transient_seed =
                    get_transient_stake_seed_at_index_from_big_vec(&validator_list, index)?;
                validator_list_index = index;
                found = true;
                let (target_total_staked_lamports, _) =
                    stake_lamports_at_validator_list_index(&validator_list, index)?;
                adjust_directed_stake_for_deposits_and_withdrawals(
                    target_total_staked_lamports,
                    index,
                    directed_stake_meta_index,
                    &mut directed_stake_meta,
                    &mut state_account,
                )?;
                break;
            }
        }
        if !found {
            msg!("Validator not found in validator list");
            directed_stake_meta.targets[directed_stake_meta_index].staked_last_updated_epoch =
                clock.epoch;
            return Ok(());
        }
    }

    let rebalance_type: RebalanceType;
    {
        let mut state_account = ctx.accounts.state_account.load_mut()?;

        // Check state first before allowing any reset logic
        // This ensures we fail with InvalidState if called from wrong state
        require!(
            state_account.state.state_tag == StewardStateEnum::RebalanceDirected,
            StewardError::InvalidState
        );

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

        // Check if staked lamports have been updated for this epoch
        if directed_stake_meta.targets[directed_stake_meta_index].staked_last_updated_epoch
            == clock.epoch
            && clock.epoch > 0
            && directed_stake_meta.targets[directed_stake_meta_index].total_staked_lamports > 0
        {
            return Err(StewardError::ValidatorAlreadyRebalanced.into());
        }

        let minimum_delegation = minimum_delegation(get_minimum_delegation()?);
        let stake_rent = Rent::get()?.minimum_balance(StakeStateV2::size_of());

        rebalance_type = {
            let stake_pool_lamports_with_fixed_cost =
                deserialize_stake_pool(&ctx.accounts.stake_pool)?.total_lamports;
            let reserve_lamports_with_rent = ctx.accounts.reserve_stake.lamports();

            let unstake_state = UnstakeState {
                directed_unstake_total: directed_stake_meta.directed_unstake_total,
            };

            let directed_unstake_cap_lamports = stake_pool_lamports_with_fixed_cost
                .saturating_mul(config.parameters.directed_stake_unstake_cap_bps as u64)
                .saturating_div(10_000);

            let undirected_tvl_lamports = stake_pool_lamports_with_fixed_cost
                .saturating_sub(directed_stake_meta.total_staked_lamports());

            let undirected_floor_cap_reached =
                undirected_tvl_lamports <= config.parameters.undirected_stake_floor_lamports();

            let target_staked_lamports =
                directed_stake_meta.targets[directed_stake_meta_index].total_staked_lamports;

            // Try decrease first, then increase (if undirected floor cap does not apply)
            let decrease_result = decrease_stake_calculation(
                &state_account.state,
                &directed_stake_meta,
                directed_stake_meta_index,
                target_staked_lamports,
                directed_unstake_cap_lamports,
                unstake_state.directed_unstake_total,
                minimum_delegation,
            );

            match decrease_result {
                Ok(RebalanceType::Decrease(_)) => decrease_result,
                _ => increase_stake_calculation(
                    &state_account.state,
                    &directed_stake_meta,
                    directed_stake_meta_index,
                    target_staked_lamports,
                    reserve_lamports_with_rent,
                    undirected_floor_cap_reached,
                    minimum_delegation,
                    stake_rent,
                ),
            }?
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
                directed_stake_meta_index,
                decrease_components.directed_unstake_lamports,
            );
            let mut state_account = ctx.accounts.state_account.load_mut()?;
            if state_account.state.validator_lamport_balances[validator_list_index]
                != LAMPORT_BALANCE_DEFAULT
            {
                state_account.state.validator_lamport_balances[validator_list_index] =
                    state_account.state.validator_lamport_balances[validator_list_index]
                        .saturating_sub(decrease_components.directed_unstake_lamports);
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
            let mut state_account = ctx.accounts.state_account.load_mut()?;
            directed_stake_meta.add_to_total_staked_lamports(directed_stake_meta_index, lamports);
            if state_account.state.validator_lamport_balances[validator_list_index]
                != LAMPORT_BALANCE_DEFAULT
            {
                state_account.state.validator_lamport_balances[validator_list_index] =
                    state_account.state.validator_lamport_balances[validator_list_index]
                        .checked_add(lamports)
                        .ok_or(StewardError::ArithmeticError)?;
            }
        }
        RebalanceType::None => {
            msg!("RebalanceType::None");
        }
    }

    let mut state_account = ctx.accounts.state_account.load_mut()?;
    // No matter the rebalance type or, we need to mark the target as rebalanced for this epoch
    directed_stake_meta.update_staked_last_updated_epoch(directed_stake_meta_index, clock.epoch);

    if let RebalanceType::Decrease(decrease_components) = &rebalance_type {
        directed_stake_meta.directed_unstake_total = directed_stake_meta
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
) -> DirectedRebalanceEvent {
    match rebalance_type {
        RebalanceType::None => DirectedRebalanceEvent {
            vote_account,
            epoch,
            rebalance_type_tag: RebalanceTypeTag::None,
            increase_lamports: 0,
            decrease_lamports: 0,
        },
        RebalanceType::Increase(lamports) => DirectedRebalanceEvent {
            vote_account,
            epoch,
            rebalance_type_tag: RebalanceTypeTag::Increase,
            increase_lamports: lamports,
            decrease_lamports: 0,
        },
        RebalanceType::Decrease(decrease_components) => DirectedRebalanceEvent {
            vote_account,
            epoch,
            rebalance_type_tag: RebalanceTypeTag::Decrease,
            increase_lamports: 0,
            decrease_lamports: decrease_components.directed_unstake_lamports,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        constants::MAX_VALIDATORS, state::directed_stake::DirectedStakeTarget, utils::U8Bool,
        BitMask,
    };
    use anchor_lang::prelude::Pubkey;

    fn create_default_directed_stake_meta() -> DirectedStakeMeta {
        DirectedStakeMeta {
            total_stake_targets: 0,
            directed_unstake_total: 0,
            padding0: [0; 63],
            is_initialized: U8Bool::from(true),
            targets: [DirectedStakeTarget {
                vote_pubkey: Pubkey::default(),
                total_target_lamports: 0,
                total_staked_lamports: 0,
                target_last_updated_epoch: 0,
                staked_last_updated_epoch: 0,
                _padding0: [0; 32],
            }; MAX_VALIDATORS],
            directed_stake_lamports: [0; MAX_VALIDATORS],
            directed_stake_meta_indices: [0; MAX_VALIDATORS],
        }
    }

    fn create_default_steward_state_account() -> StewardStateAccountV2 {
        StewardStateAccountV2 {
            state: crate::state::steward_state::StewardStateV2 {
                state_tag: StewardStateEnum::RebalanceDirected,
                validator_lamport_balances: [0; MAX_VALIDATORS],
                scores: [0; MAX_VALIDATORS],
                sorted_score_indices: [crate::constants::SORTED_INDEX_DEFAULT; MAX_VALIDATORS],
                raw_scores: [0; MAX_VALIDATORS],
                sorted_raw_score_indices: [crate::constants::SORTED_INDEX_DEFAULT; MAX_VALIDATORS],
                delegations: [crate::Delegation::default(); MAX_VALIDATORS],
                instant_unstake: BitMask::default(),
                progress: BitMask::default(),
                validators_to_remove: BitMask::default(),
                validators_for_immediate_removal: BitMask::default(),
                start_computing_scores_slot: 0,
                current_epoch: 0,
                next_cycle_epoch: 10,
                num_pool_validators: 0,
                scoring_unstake_total: 0,
                instant_unstake_total: 0,
                stake_deposit_unstake_total: 0,
                validators_added: 0,
                status_flags: 0,
                _padding0: [0; 2],
            },
            bump: 0,
            _padding0: [0; 7],
        }
    }

    #[test]
    fn test_adjust_directed_stake_no_adjustment_needed() {
        // Scenario: Everything is in sync - no deposits or withdrawals detected
        // target_total_staked_lamports = steward_state_total_lamports = 1000
        // directed_stake_applied_lamports = 500, directed_stake_target_lamports = 1000
        // No adjustment should occur

        let mut directed_stake_meta = create_default_directed_stake_meta();

        let validator_list_index = 0;
        let directed_stake_meta_index = 0;
        let target_total_staked_lamports = 1000;
        let steward_state_total_lamports = 1000;
        let directed_stake_target_lamports = 1000;
        let directed_stake_applied_lamports = 500;

        // Set up the state
        let mut state_account = create_default_steward_state_account();
        state_account.state.validator_lamport_balances[validator_list_index] =
            steward_state_total_lamports;

        directed_stake_meta.targets[directed_stake_meta_index] = DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: directed_stake_target_lamports,
            total_staked_lamports: directed_stake_applied_lamports,
            target_last_updated_epoch: 0,
            staked_last_updated_epoch: 0,
            _padding0: [0; 32],
        };

        let initial_staked_lamports =
            directed_stake_meta.targets[directed_stake_meta_index].total_staked_lamports;

        // Call the function
        let result = adjust_directed_stake_for_deposits_and_withdrawals(
            target_total_staked_lamports,
            validator_list_index,
            directed_stake_meta_index,
            &mut directed_stake_meta,
            &mut state_account,
        );

        assert!(result.is_ok());
        // No adjustment should have occurred
        assert_eq!(
            directed_stake_meta.targets[directed_stake_meta_index].total_staked_lamports,
            initial_staked_lamports
        );
    }

    #[test]
    fn test_adjust_directed_stake_deposit_side_adjustment() {
        let mut directed_stake_meta = create_default_directed_stake_meta();
        let validator_list_index = 0;
        let directed_stake_meta_index = 0;
        let target_total_staked_lamports = 1600; // Stake pool shows higher than expected amount
        let steward_state_total_lamports = 1000;
        let directed_stake_target_lamports = 1000; // Directed stake is at target
        let directed_stake_applied_lamports = 500;

        // Set up the state
        let mut state_account = create_default_steward_state_account();
        state_account.state.validator_lamport_balances[validator_list_index] =
            steward_state_total_lamports;

        directed_stake_meta.targets[directed_stake_meta_index] = DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: directed_stake_target_lamports,
            total_staked_lamports: directed_stake_applied_lamports,
            target_last_updated_epoch: 0,
            staked_last_updated_epoch: 0,
            _padding0: [0; 32],
        };

        // Call the function
        let result = adjust_directed_stake_for_deposits_and_withdrawals(
            target_total_staked_lamports,
            validator_list_index,
            directed_stake_meta_index,
            &mut directed_stake_meta,
            &mut state_account,
        );

        assert!(result.is_ok());
        assert_eq!(
            directed_stake_meta.targets[directed_stake_meta_index].total_staked_lamports,
            directed_stake_applied_lamports + 500 // 500 is the min of deposit 600 and deficit of 500
        );
    }

    #[test]
    fn test_adjust_directed_stake_withdrawal_detected_no_adjustment() {
        let mut directed_stake_meta = create_default_directed_stake_meta();
        let validator_list_index = 0;
        let directed_stake_meta_index = 0;
        let target_total_staked_lamports = 1100; // Stake pool shows higher than expected amount
        let steward_state_total_lamports = 1000;
        let directed_stake_target_lamports = 1000; // Directed stake is at target
        let directed_stake_applied_lamports = 1000;

        // Set up the state
        let mut state_account = create_default_steward_state_account();
        state_account.state.validator_lamport_balances[validator_list_index] =
            steward_state_total_lamports;

        directed_stake_meta.targets[directed_stake_meta_index] = DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: directed_stake_target_lamports,
            total_staked_lamports: directed_stake_applied_lamports,
            target_last_updated_epoch: 0,
            staked_last_updated_epoch: 0,
            _padding0: [0; 32],
        };

        let initial_staked_lamports =
            directed_stake_meta.targets[directed_stake_meta_index].total_staked_lamports;

        // Call the function
        let result = adjust_directed_stake_for_deposits_and_withdrawals(
            target_total_staked_lamports,
            validator_list_index,
            directed_stake_meta_index,
            &mut directed_stake_meta,
            &mut state_account,
        );

        assert!(result.is_ok());
        // No adjustment should have occurred since applied (400) <= target (500)
        assert_eq!(
            directed_stake_meta.targets[directed_stake_meta_index].total_staked_lamports,
            initial_staked_lamports
        );
    }

    #[test]
    fn test_adjust_directed_stake_withdrawal_side_adjustment() {
        let mut directed_stake_meta = create_default_directed_stake_meta();
        let validator_list_index = 0;
        let directed_stake_meta_index = 0;
        let target_total_staked_lamports = 700; // Stake pool shows lower than expected amount
        let steward_state_total_lamports = 1000;
        let directed_stake_target_lamports = 1000; // Directed stake is at target
        let directed_stake_applied_lamports = 1000;

        // Set up the state
        let mut state_account = create_default_steward_state_account();
        state_account.state.validator_lamport_balances[validator_list_index] =
            steward_state_total_lamports;

        directed_stake_meta.targets[directed_stake_meta_index] = DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: directed_stake_target_lamports,
            total_staked_lamports: directed_stake_applied_lamports,
            target_last_updated_epoch: 0,
            staked_last_updated_epoch: 0,
            _padding0: [0; 32],
        };

        // Call the function
        let result = adjust_directed_stake_for_deposits_and_withdrawals(
            target_total_staked_lamports,
            validator_list_index,
            directed_stake_meta_index,
            &mut directed_stake_meta,
            &mut state_account,
        );

        assert!(result.is_ok());
        assert_eq!(
            directed_stake_meta.targets[directed_stake_meta_index].total_staked_lamports,
            directed_stake_applied_lamports - 300
        );
    }

    #[test]
    fn test_adjust_directed_stake_withdrawal_side_adjustment_with_remainder() {
        let mut directed_stake_meta = create_default_directed_stake_meta();
        let validator_list_index = 0;
        let directed_stake_meta_index = 0;
        let target_total_staked_lamports = 700; // Stake pool shows lower than expected amount
        let steward_state_total_lamports = 3000;
        let directed_stake_target_lamports = 1001; // Directed stake is not at target
        let directed_stake_applied_lamports = 1000;

        // Set up the state
        let mut state_account = create_default_steward_state_account();
        state_account.state.validator_lamport_balances[validator_list_index] =
            steward_state_total_lamports;

        directed_stake_meta.targets[directed_stake_meta_index] = DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: directed_stake_target_lamports,
            total_staked_lamports: directed_stake_applied_lamports,
            target_last_updated_epoch: 0,
            staked_last_updated_epoch: 0,
            _padding0: [0; 32],
        };

        // Call the function
        let result = adjust_directed_stake_for_deposits_and_withdrawals(
            target_total_staked_lamports,
            validator_list_index,
            directed_stake_meta_index,
            &mut directed_stake_meta,
            &mut state_account,
        );

        assert!(result.is_ok());
        assert_eq!(
            directed_stake_meta.targets[directed_stake_meta_index].total_staked_lamports,
            0
        );

        let withdrawal_lamports =
            steward_state_total_lamports.saturating_sub(target_total_staked_lamports);
        // The entire remaining lamports should be undirected stake, equal to the old total minus the withdrawal lamports
        assert_eq!(
            state_account.state.validator_lamport_balances[validator_list_index],
            steward_state_total_lamports.saturating_sub(withdrawal_lamports)
        );
    }
}
