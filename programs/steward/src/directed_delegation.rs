use anchor_lang::prelude::*;

use crate::events::DecreaseComponents;
use crate::state::directed_stake::DirectedStakeMeta;
use crate::{errors::StewardError, StewardState};
#[derive(Debug, Clone)]
pub enum RebalanceType {
    Increase(u64),
    Decrease(DecreaseComponents),
    None,
}

/// Given a target validator, determines how much stake to remove on this validator given the constraints of unstaking caps.
/// Validators with lower yield_scores are prioritized for unstaking. We simulate unstaking movements on each validator, starting
/// from the lowest yield_score validator, until we reach the target validator. If the target validator is reached and there is still
/// capacity to unstake, we return the total amount to unstake. If the target validator is reached with no capacity to unstake, we return RebalanceType::None.
///
/// Unstaking is calculated this way because stake account balances can change at any time from users' stake withdrawals and deposits,
/// and this ensures that unstaking is done fairly at the time of the rebalance. In addition, these instructions can run in any order.
#[allow(clippy::too_many_arguments)]
pub fn decrease_stake_calculation(
    state: &StewardState,
    directed_stake_meta: &DirectedStakeMeta,
    target_index: usize,
    unstake_state: UnstakeState,
    current_lamports: u64, // active lamports in target stake account adjusted for minimum delegation
    _stake_pool_lamports: u64,
    _minimum_delegation: u64,
    _stake_rent: u64,
    directed_unstake_cap_lamports: u64,
) -> Result<RebalanceType> {
    if target_index >= state.num_pool_validators as usize {
        return Err(StewardError::ValidatorIndexOutOfBounds.into());
    }

    let vote_pubkey = directed_stake_meta.targets[target_index].vote_pubkey;
    let target_lamports = directed_stake_meta
        .get_target_lamports(&vote_pubkey)
        .ok_or(StewardError::ValidatorIndexOutOfBounds)?;

    msg!(
        "current_lamports: {}, target_lamports: {}",
        current_lamports,
        target_lamports
    );

    // Check if we need to decrease (current > target)
    if current_lamports <= target_lamports {
        msg!("Current lamports is less than or equal to target lamports, no directed decreases can be made.");
        return Ok(RebalanceType::None);
    }

    let excess_lamports = current_lamports.saturating_sub(target_lamports);
    msg!("Excess lamports on target validator: {}", excess_lamports);
    msg!(
        "Directed unstake cap lamports: {}",
        directed_unstake_cap_lamports
    );

    let mut total_excess_lamports: u64 = 0u64;
    for target in directed_stake_meta.targets.iter() {
        let target_lamports = directed_stake_meta
            .get_target_lamports(&target.vote_pubkey)
            .ok_or(StewardError::ValidatorIndexOutOfBounds)?;
        let staked_lamports = directed_stake_meta
            .get_total_staked_lamports(&target.vote_pubkey)
            .ok_or(StewardError::ValidatorIndexOutOfBounds)?;
        let excess = staked_lamports.saturating_sub(target_lamports);
        total_excess_lamports = total_excess_lamports.saturating_add(excess);
    }

    if total_excess_lamports == 0 {
        msg!("Total excess lamports is 0, no directed decreases can be made.");
        return Ok(RebalanceType::None);
    }

    let excess_proportion_bps: u128 =
        (excess_lamports as u128).saturating_mul(10_000) / (total_excess_lamports as u128);

    let unstake_total = directed_unstake_cap_lamports.min(total_excess_lamports);

    let proportional_decrease_lamports: u64 =
        ((unstake_total as u128).saturating_mul(excess_proportion_bps) / 10_000)
            .try_into()
            .map_err(|_| StewardError::ArithmeticError)?;

    msg!(
        "Decreasing stake by {} lamports",
        proportional_decrease_lamports
    );

    Ok(RebalanceType::Decrease(DecreaseComponents {
        scoring_unstake_lamports: 0,
        instant_unstake_lamports: 0,
        stake_deposit_unstake_lamports: 0,
        total_unstake_lamports: 0,
        directed_unstake_lamports: proportional_decrease_lamports,
    }))
}

/// Given a target validator, determines how much stake to add on this validator given the constraints of reserve stake.
/// Validators with higher scores are prioritized for staking. We simulate staking movements on each validator, starting
/// from the highest score validator, until we reach the target validator. If the target validator is reached and there is still
/// reserve lamports to stake, we return the total amount to stake. If the target validator is reached with no reserve lamports to stake,
/// we return RebalanceType::None.
///
/// This allows for a fair staking distribution based on the current state of the pool, and these instructions can run in any order.
#[allow(clippy::too_many_arguments)]
pub fn increase_stake_calculation(
    state: &StewardState,
    directed_stake_meta: &DirectedStakeMeta,
    target_index: usize,
    current_lamports: u64,
    _stake_pool_lamports: u64,
    reserve_lamports: u64,
    _minimum_delegation: u64,
    _stake_rent: u64,
    undirected_cap_reached: bool,
) -> Result<RebalanceType> {
    if target_index >= state.num_pool_validators as usize {
        return Err(StewardError::ValidatorIndexOutOfBounds.into());
    }

    // If the undirected floor has been reached, no directed increases can be made
    if undirected_cap_reached {
        msg!("Undirected TVL floor reached, no directed increases can be made.");
        return Ok(RebalanceType::None);
    }

    let vote_pubkey = directed_stake_meta.targets[target_index].vote_pubkey;
    let target_lamports = directed_stake_meta
        .get_target_lamports(&vote_pubkey)
        .ok_or(StewardError::ValidatorIndexOutOfBounds)?;

    msg!(
        "current_lamports: {}, target_lamports: {}",
        current_lamports,
        target_lamports
    );

    let delta_lamports = target_lamports.saturating_sub(current_lamports);

    if delta_lamports == 0 {
        msg!("Target lamports is equal to current lamports, no directed increases can be made.");
        return Ok(RebalanceType::None);
    }

    let mut total_delta_lamports: u64 = 0u64;
    let target_delta_lamports: u64 = target_lamports.saturating_sub(current_lamports);

    for target in directed_stake_meta.targets.iter() {
        let target_lamports = directed_stake_meta
            .get_target_lamports(&target.vote_pubkey)
            .ok_or(StewardError::ValidatorIndexOutOfBounds)?;
        let staked_lamports = directed_stake_meta
            .get_total_staked_lamports(&target.vote_pubkey)
            .ok_or(StewardError::ValidatorIndexOutOfBounds)?;
        let delta_lamports = target_lamports.saturating_sub(staked_lamports);
        total_delta_lamports = total_delta_lamports.saturating_add(delta_lamports);
    }

    if total_delta_lamports == 0 {
        msg!("Total delta lamports is 0, no directed increases can be made.");
        return Ok(RebalanceType::None);
    }

    let delta_proportion_bps: u128 =
        (target_delta_lamports as u128).saturating_mul(10_000) / (total_delta_lamports as u128);

    let proportional_increase_lamports: u64 =
        ((reserve_lamports as u128).saturating_mul(delta_proportion_bps) / 10_000)
            .try_into()
            .map_err(|_| StewardError::ArithmeticError)?;

    let adjusted_proportional_increase_lamports = {
        let target_difference = target_lamports.saturating_sub(current_lamports);
        let amount = proportional_increase_lamports.min(target_difference);
        amount
    };

    msg!(
        "Increasing stake by {} lamports",
        adjusted_proportional_increase_lamports
    );

    return Ok(RebalanceType::Increase(
        adjusted_proportional_increase_lamports,
    ));
}

#[derive(Default)]
pub struct UnstakeAmounts {
    pub directed_unstake_lamports: u64,
}

impl UnstakeAmounts {
    pub fn total(&self) -> Result<u64> {
        Ok(self.directed_unstake_lamports)
    }
}

#[derive(Default, Clone)]
pub struct UnstakeState {
    pub directed_unstake_total: u64,
}
