use anchor_lang::prelude::*;
use spl_stake_pool::big_vec::BigVec;

use crate::constants::LAMPORT_BALANCE_DEFAULT;
use crate::events::DecreaseComponents;
use crate::state::directed_stake::DirectedStakeMeta;
use crate::{
    errors::StewardError,
    utils::{
        vote_pubkey_at_validator_list_index,
    },
    StewardState,
};
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
    mut unstake_state: UnstakeState,
    current_lamports: u64, // active lamports in target stake account adjusted for minimum delegation
    _stake_pool_lamports: u64,
    _minimum_delegation: u64,
    _stake_rent: u64,
) -> Result<RebalanceType> {
    if target_index >= state.num_pool_validators as usize {
        return Err(StewardError::ValidatorIndexOutOfBounds.into());
    }

    // Is this reasonable with the dynamic nature of the stake meta?
    let vote_pubkey = directed_stake_meta.targets[target_index].vote_pubkey;
    let target_lamports = directed_stake_meta
        .get_target_lamports(&vote_pubkey)
        .ok_or(StewardError::ValidatorIndexOutOfBounds)?;

    // Check if we need to decrease (current > target)
    if current_lamports <= target_lamports {
        return Ok(RebalanceType::None);
    }
    
    let excess_lamports = current_lamports.saturating_sub(target_lamports);
    
    let mut total_excess_lamports: u64 = 0u64;
    for target in directed_stake_meta.targets.iter() {
        let target_lamports = directed_stake_meta.get_target_lamports(&target.vote_pubkey).ok_or(StewardError::ValidatorIndexOutOfBounds)?;
        let staked_lamports = directed_stake_meta.get_total_staked_lamports(&target.vote_pubkey).ok_or(StewardError::ValidatorIndexOutOfBounds)?;
        let excess = staked_lamports.saturating_sub(target_lamports);
        total_excess_lamports = total_excess_lamports.saturating_add(excess);
    }
    
    let excess_proportion_bps: u128 =
        (excess_lamports as u128).saturating_mul(10_000) / (total_excess_lamports as u128);

    let proportional_decrease_lamports: u64 =
        ((unstake_state.directed_unstake_cap as u128).saturating_mul(excess_proportion_bps) / 10_000)
            .try_into()
            .map_err(|_| StewardError::ArithmeticError)?;

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
    mut reserve_lamports: u64,
    _minimum_delegation: u64,
    _stake_rent: u64,
) -> Result<RebalanceType> {
    if target_index >= state.num_pool_validators as usize {
        return Err(StewardError::ValidatorIndexOutOfBounds.into());
    }

    // Is this reasonable with the dynamic nature of the stake meta?
    let vote_pubkey = directed_stake_meta.targets[target_index].vote_pubkey;
    let target_lamports = directed_stake_meta
        .get_target_lamports(&vote_pubkey)
        .ok_or(StewardError::ValidatorIndexOutOfBounds)?;

    let delta_lamports = target_lamports.saturating_sub(current_lamports);

    if delta_lamports == 0 {
        return Ok(RebalanceType::None);
    }

    let mut total_delta_lamports: u64 = 0u64;

    for target in directed_stake_meta.targets.iter() {
        let target_lamports = directed_stake_meta.get_target_lamports(&target.vote_pubkey).ok_or(StewardError::ValidatorIndexOutOfBounds)?;
        let staked_lamports = directed_stake_meta.get_total_staked_lamports(&target.vote_pubkey).ok_or(StewardError::ValidatorIndexOutOfBounds)?;
        let delta_lamports = target_lamports.saturating_sub(staked_lamports);
        total_delta_lamports = total_delta_lamports.saturating_add(delta_lamports);
    }

    let delta_proportion_bps: u128 =
        (delta_lamports as u128).saturating_mul(10_000) / (total_delta_lamports as u128);

    let proportional_increase_lamports: u64 =
        ((reserve_lamports as u128).saturating_mul(delta_proportion_bps) / 10_000)
            .try_into()
            .map_err(|_| StewardError::ArithmeticError)?;

    if current_lamports < target_lamports {
        return Ok(RebalanceType::Increase(proportional_increase_lamports));
    }
    Err(StewardError::ValidatorIndexOutOfBounds.into())
}

#[derive(Default)]
pub struct UnstakeAmounts {
    pub stake_deposit_unstake_lamports: u64,
    pub instant_unstake_lamports: u64,
    pub scoring_unstake_lamports: u64,
    pub directed_unstake_lamports: u64,
}

impl UnstakeAmounts {
    pub fn total(&self) -> Result<u64> {
        self.stake_deposit_unstake_lamports
            .checked_add(self.instant_unstake_lamports)
            .and_then(|s| s.checked_add(self.scoring_unstake_lamports))
            .and_then(|s| s.checked_add(self.directed_unstake_lamports))
            .ok_or_else(|| StewardError::ArithmeticError.into())
    }
}

#[derive(Default, Clone)]
pub struct UnstakeState {
    pub stake_deposit_unstake_total: u64,
    pub instant_unstake_total: u64,
    pub scoring_unstake_total: u64,
    pub stake_deposit_unstake_cap: u64,
    pub instant_unstake_cap: u64,
    pub scoring_unstake_cap: u64,
    pub directed_unstake_total: u64,
    pub directed_unstake_cap: u64,
}

impl UnstakeState {

    pub fn stake_deposit_unstake(
        &self,
        directed_stake_meta: &DirectedStakeMeta,
        index: usize,
        current_lamports: u64,
        target_lamports: u64,
    ) -> Result<u64> {
        // Check if the validator has gotten a stake deposit, and if so, destake those additional lamports
        // either to the target or to the previous balance before the deposit, whichever is lower in terms of total lamports unstaked
        let vote_pubkey = directed_stake_meta.targets[index].vote_pubkey;
        let staked_lamport_balance = directed_stake_meta.get_total_staked_lamports(&vote_pubkey).ok_or(StewardError::ValidatorIndexOutOfBounds)?;
        if current_lamports > staked_lamport_balance
            && self.stake_deposit_unstake_total < self.stake_deposit_unstake_cap
            && staked_lamport_balance != LAMPORT_BALANCE_DEFAULT
        {
            let lamports_above_target = current_lamports
                .checked_sub(target_lamports)
                .ok_or(StewardError::ArithmeticError)?;

            let lamports_above_balance = current_lamports
                .checked_sub(staked_lamport_balance)
                .ok_or(StewardError::ArithmeticError)?;

            let cap_limit = self
                .stake_deposit_unstake_cap
                .checked_sub(self.stake_deposit_unstake_total)
                .ok_or(StewardError::ArithmeticError)?;

            let stake_deposit_unstake_lamports = lamports_above_target
                .min(lamports_above_balance)
                .min(cap_limit);

            return Ok(stake_deposit_unstake_lamports);
        }
        Ok(0)
    }

    pub fn instant_unstake(
        &self,
        state: &StewardState,
        index: usize,
        current_lamports: u64,
        target_lamports: u64,
    ) -> Result<u64> {
        // If this validator is marked for instant unstake, destake to the target
        /*if state.instant_unstake.get_unsafe(index)
            && self.instant_unstake_total < self.instant_unstake_cap
        {
            let lamports_above_target = current_lamports
                .checked_sub(target_lamports)
                .ok_or(StewardError::ArithmeticError)?;

            let cap_limit = self
                .instant_unstake_cap
                .checked_sub(self.instant_unstake_total)
                .ok_or(StewardError::ArithmeticError)?;

            let instant_unstake_lamports = lamports_above_target.min(cap_limit);

            return Ok(instant_unstake_lamports);
        }*/
        Ok(0)
    }

    pub fn scoring_unstake(&self, current_lamports: u64, target_lamports: u64) -> Result<u64> {
        // If there are additional lamports to unstake on this validator and the total unstaked lamports is below the cap, destake to the target
        /*if self.scoring_unstake_total < self.scoring_unstake_cap {
            let lamports_above_target = current_lamports
                .checked_sub(target_lamports)
                .ok_or(StewardError::ArithmeticError)?;

            let cap_limit = self
                .scoring_unstake_cap
                .checked_sub(self.scoring_unstake_total)
                .ok_or(StewardError::ArithmeticError)?;

            let scoring_unstake_lamports = lamports_above_target.min(cap_limit);
            return Ok(scoring_unstake_lamports);
        }*/
        Ok(0)
    }
}
