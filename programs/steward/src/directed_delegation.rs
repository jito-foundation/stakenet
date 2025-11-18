use anchor_lang::prelude::*;

use crate::events::DecreaseComponents;
use crate::state::directed_stake::DirectedStakeMeta;
use crate::{errors::StewardError, StewardStateV2};
#[derive(Debug, Clone)]
pub enum RebalanceType {
    Increase(u64),
    Decrease(DecreaseComponents),
    None,
}

/// Given a target validator, determines how much stake to remove on this validator given the constraints directed unstaking cap
/// and the undirected stake TVL floor. If there is no decrease to be performed given these constraints, we return RebalanceType::None.
/// The total execess lamports of the validator set relative to the target lamports is calculated. Caps are applied to this value
/// and then the decrease is calculated proportionally amongst the validators that still require unstaking.
#[allow(clippy::too_many_arguments)]
pub fn decrease_stake_calculation(
    state: &StewardStateV2,
    directed_stake_meta: &DirectedStakeMeta,
    target_index: usize,
    current_lamports: u64, // active lamports in target stake account adjusted for minimum delegation
    directed_unstake_cap_lamports: u64,
    directed_unstake_total_lamports: u64,
    minimum_delegation: u64,
    stake_rent: u64,
    epoch: u64,
) -> Result<RebalanceType> {
    if target_index >= directed_stake_meta.total_stake_targets as usize {
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

    let target_delta_lamports = current_lamports.saturating_sub(target_lamports);
    msg!(
        "Excess lamports on target validator: {}",
        target_delta_lamports
    );

    let mut total_excess_lamports: u64 = 0u64;
    for target in
        directed_stake_meta.targets[..directed_stake_meta.total_stake_targets as usize].iter()
    {
        if target.staked_last_updated_epoch == epoch {
            continue;
        }
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
        msg!("Total excess lamports is 0, no directed decrease to perform.");
        return Ok(RebalanceType::None);
    }

    let delta_proportion_bps: u128 =
        (target_delta_lamports as u128).saturating_mul(10_000) / (total_excess_lamports as u128);

    // Apply the remaining directed unstake cap to the total excess lamports
    let unstake_total = directed_unstake_cap_lamports
        .saturating_sub(directed_unstake_total_lamports)
        .min(total_excess_lamports);

    // Calculate the proportional decrease amongst the validators that still require decreases
    let proportional_decrease_lamports: u64 =
        ((unstake_total as u128).saturating_mul(delta_proportion_bps) / 10_000)
            .try_into()
            .map_err(|_| StewardError::ArithmeticError)?;

    // Do not unstake more than the excess lamports on the target validator to prevent yield drag
    let adjusted_proportional_decrease_lamports =
        proportional_decrease_lamports.min(target_delta_lamports);

    if adjusted_proportional_decrease_lamports < (minimum_delegation) {
        msg!("Adjusted proportional decrease lamports is less than minimum delegation for transient stake account. No unstake will be performed.");
        return Ok(RebalanceType::None);
    }

    msg!(
        "Decreasing stake by {} lamports",
        adjusted_proportional_decrease_lamports
    );

    Ok(RebalanceType::Decrease(DecreaseComponents {
        scoring_unstake_lamports: 0,
        instant_unstake_lamports: 0,
        stake_deposit_unstake_lamports: 0,
        total_unstake_lamports: adjusted_proportional_decrease_lamports,
        directed_unstake_lamports: adjusted_proportional_decrease_lamports,
    }))
}

/// Given a target validator, determines how much stake to add on this validator given the constraints of reserve stake.
/// If the undirected TVL floor has been reach, no directed increases will be made in an effort to prevent the pool from
/// becoming too concentrated in directed staking.
#[allow(clippy::too_many_arguments)]
pub fn increase_stake_calculation(
    state: &StewardStateV2,
    directed_stake_meta: &DirectedStakeMeta,
    target_index: usize,
    current_lamports: u64,
    reserve_lamports: u64,
    undirected_cap_reached: bool,
    minimum_delegation: u64,
    stake_rent: u64,
    epoch: u64,
) -> Result<RebalanceType> {
    if target_index >= directed_stake_meta.total_stake_targets as usize {
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

    let target_delta_lamports: u64 = target_lamports.saturating_sub(current_lamports);

    if target_delta_lamports == 0 {
        msg!("Target lamports is equal to current lamports, no directed increases can be made.");
        return Ok(RebalanceType::None);
    }

    let mut total_delta_lamports: u64 = 0u64;

    for target in
        directed_stake_meta.targets[..directed_stake_meta.total_stake_targets as usize].iter()
    {
        if target.staked_last_updated_epoch == epoch {
            continue;
        }
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

    // We must preserve at least stake_rent in the reserve stake account for rent-exemption
    let available_lamports = reserve_lamports.saturating_sub(stake_rent);

    let proportional_increase_lamports: u64 =
        ((available_lamports as u128).saturating_mul(delta_proportion_bps) / 10_000)
            .try_into()
            .map_err(|_| StewardError::ArithmeticError)?;

    // Do not over-delegate if proportional increase would exceed the target delta lamports
    // This prevents future yield drag from unstaking excess lamports
    let adjusted_proportional_increase_lamports =
        proportional_increase_lamports
        .min(target_delta_lamports)
        .min(reserve_lamports);

    if adjusted_proportional_increase_lamports < (minimum_delegation) {
        msg!("Adjusted proportional decrease lamports is less than minimum delegation for transient stake account. No unstake will be performed.");
        return Ok(RebalanceType::None);
    }

    msg!(
        "Increasing stake by {} lamports",
        adjusted_proportional_increase_lamports
    );

    Ok(RebalanceType::Increase(
        adjusted_proportional_increase_lamports,
    ))
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
