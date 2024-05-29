use anchor_lang::prelude::*;
use spl_stake_pool::big_vec::BigVec;

use crate::{
    errors::StewardError,
    utils::{get_target_lamports, stake_lamports_at_validator_list_index},
    StewardState,
};

#[derive(Debug)]
pub enum RebalanceType {
    Increase(u64),
    Decrease(DecreaseComponents),
    None,
}

#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct DecreaseComponents {
    pub scoring_unstake_lamports: u64,
    pub instant_unstake_lamports: u64,
    pub stake_deposit_unstake_lamports: u64,
    pub total_unstake_lamports: u64,
}

/// Given a target validator, determines how much stake to remove on this validator given the constraints of unstaking caps.
/// Validators with lower yield_scores are prioritized for unstaking. We simulate unstaking movements on each validator, starting
/// from the lowest yield_score validator, until we reach the target validator. If the target validator is reached and there is still
/// capacity to unstake, we return the total amount to unstake. If the target validator is reached with no capacity to unstake, we return RebalanceType::None.
///
/// Unstaking is calculated this way because stake account balances can change at any time from users' stake withdrawals and deposits,
/// and this ensures that unstaking is done fairly at the time of the rebalance. In addition, these instructions can run in any order.
pub fn decrease_stake_calculation(
    state: &StewardState,
    target_index: usize,
    mut unstake_state: UnstakeState,
    stake_pool_lamports: u64,
    validator_list: &BigVec<'_>,
    minimum_delegation: u64,
    stake_rent: u64,
) -> Result<RebalanceType> {
    if target_index >= state.num_pool_validators {
        return Err(StewardError::ValidatorIndexOutOfBounds.into());
    }

    let base_lamport_balance = minimum_delegation
        .checked_add(stake_rent)
        .ok_or(StewardError::ArithmeticError)?;

    for idx in state.sorted_yield_score_indices[..state.num_pool_validators]
        .iter()
        .rev()
    {
        let temp_index = *idx as usize;
        let temp_target_lamports = if state.instant_unstake.get_unsafe(temp_index) {
            0
        } else {
            get_target_lamports(&state.delegations[temp_index], stake_pool_lamports)?
        };

        let (mut temp_current_lamports, some_transient_stake) =
            stake_lamports_at_validator_list_index(validator_list, temp_index)?;

        // ValidatorList includes base lamports in active_stake_lamports
        temp_current_lamports = temp_current_lamports.saturating_sub(base_lamport_balance);

        // For the current `temp` validator, calculate how much we can remove and what category it's coming from
        let unstake_amounts =
            if !some_transient_stake && temp_target_lamports < temp_current_lamports {
                unstake_state.simulate_unstake(
                    state,
                    temp_index,
                    temp_current_lamports,
                    temp_target_lamports,
                )?
            } else {
                // If the validator has transient lamports, some rebalancing has already taken place so we skip
                UnstakeAmounts::default()
            };

        if temp_index == target_index {
            let total_unstake_lamports = unstake_amounts.total()?;

            if total_unstake_lamports <= minimum_delegation {
                return Ok(RebalanceType::None);
            }

            return Ok(RebalanceType::Decrease(DecreaseComponents {
                scoring_unstake_lamports: unstake_amounts.scoring_unstake_lamports,
                instant_unstake_lamports: unstake_amounts.instant_unstake_lamports,
                stake_deposit_unstake_lamports: unstake_amounts.stake_deposit_unstake_lamports,
                total_unstake_lamports,
            }));
        }
    }

    Err(StewardError::ValidatorIndexOutOfBounds.into())
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
    target_index: usize,
    current_lamports: u64,
    stake_pool_lamports: u64,
    validator_list: &BigVec<'_>,
    mut reserve_lamports: u64,
    minimum_delegation: u64,
    stake_rent: u64,
) -> Result<RebalanceType> {
    if target_index >= state.num_pool_validators {
        return Err(StewardError::ValidatorIndexOutOfBounds.into());
    }
    let target_lamports: u64 =
        get_target_lamports(&state.delegations[target_index], stake_pool_lamports)?;

    let base_lamport_balance = minimum_delegation
        .checked_add(stake_rent)
        .ok_or(StewardError::ArithmeticError)?;

    if current_lamports < target_lamports {
        for idx in state.sorted_score_indices[..state.num_pool_validators].iter() {
            let temp_index = *idx as usize;
            let lamports = if state.delegations[temp_index].numerator > 0
                && !state.instant_unstake.get(temp_index)?
            {
                let temp_target_lamports =
                    get_target_lamports(&state.delegations[temp_index], stake_pool_lamports)?;

                let (mut temp_current_lamports, some_transient_stake) =
                    stake_lamports_at_validator_list_index(validator_list, temp_index)?;

                // ValidatorList includes base lamports in active_stake_lamports
                temp_current_lamports = temp_current_lamports.saturating_sub(base_lamport_balance);

                if !some_transient_stake && temp_current_lamports < temp_target_lamports {
                    // Stake lamports to this validator up to target or until reserve is depleted
                    let lamports_below_target = temp_target_lamports
                        .checked_sub(temp_current_lamports)
                        .ok_or(StewardError::ArithmeticError)?;
                    let to_stake = lamports_below_target.min(reserve_lamports);
                    reserve_lamports = reserve_lamports
                        .checked_sub(to_stake)
                        .ok_or(StewardError::ArithmeticError)?;
                    to_stake
                } else {
                    // If the validator has transient lamports, some rebalancing has already taken place so we skip
                    0
                }
            } else {
                0
            };

            if temp_index == target_index {
                if lamports <= minimum_delegation {
                    return Ok(RebalanceType::None);
                }

                return Ok(RebalanceType::Increase(lamports));
            }
        }
        return Err(StewardError::ValidatorIndexOutOfBounds.into());
    }
    Err(StewardError::InvalidState.into())
}

#[derive(Default)]
pub struct UnstakeAmounts {
    pub stake_deposit_unstake_lamports: u64,
    pub instant_unstake_lamports: u64,
    pub scoring_unstake_lamports: u64,
}

impl UnstakeAmounts {
    pub fn total(&self) -> Result<u64> {
        self.stake_deposit_unstake_lamports
            .checked_add(self.instant_unstake_lamports)
            .and_then(|s| s.checked_add(self.scoring_unstake_lamports))
            .ok_or_else(|| StewardError::ArithmeticError.into())
    }
}

#[derive(Default)]
pub struct UnstakeState {
    pub stake_deposit_unstake_total: u64,
    pub instant_unstake_total: u64,
    pub scoring_unstake_total: u64,
    pub stake_deposit_unstake_cap: u64,
    pub instant_unstake_cap: u64,
    pub scoring_unstake_cap: u64,
}

impl UnstakeState {
    /// Calculate how much stake to remove from the validator at `index`, and return the amount to unstake broken down by unstake category.
    /// Prioritizes stake deposit unstake, then instant unstake, then regular unstake. Won't unstake beyond each category's cap.
    ///
    /// Note: modifies unstake_totals
    pub fn simulate_unstake(
        &mut self,
        state: &StewardState,
        index: usize,
        mut current_lamports: u64,
        target_lamports: u64,
    ) -> Result<UnstakeAmounts> {
        // Stake deposit
        let stake_deposit_unstake_lamports =
            self.stake_deposit_unstake(state, index, current_lamports, target_lamports)?;
        current_lamports = current_lamports.saturating_sub(stake_deposit_unstake_lamports);
        self.stake_deposit_unstake_total = self
            .stake_deposit_unstake_total
            .checked_add(stake_deposit_unstake_lamports)
            .ok_or(StewardError::ArithmeticError)?;

        // Instant unstake
        let instant_unstake_lamports =
            self.instant_unstake(state, index, current_lamports, target_lamports)?;
        current_lamports = current_lamports.saturating_sub(instant_unstake_lamports);
        self.instant_unstake_total = self
            .instant_unstake_total
            .checked_add(instant_unstake_lamports)
            .ok_or(StewardError::ArithmeticError)?;

        // Scoring unstake
        let scoring_unstake_lamports = self.scoring_unstake(current_lamports, target_lamports)?;
        self.scoring_unstake_total = self
            .scoring_unstake_total
            .checked_add(scoring_unstake_lamports)
            .ok_or(StewardError::ArithmeticError)?;

        Ok(UnstakeAmounts {
            stake_deposit_unstake_lamports,
            instant_unstake_lamports,
            scoring_unstake_lamports,
        })
    }

    fn stake_deposit_unstake(
        &self,
        state: &StewardState,
        index: usize,
        current_lamports: u64,
        target_lamports: u64,
    ) -> Result<u64> {
        // Check if the validator has gotten a stake deposit, and if so, destake those additional lamports
        // either to the target or to the previous balance before the deposit, whichever is lower in terms of total lamports unstaked
        if current_lamports > state.validator_lamport_balances[index]
            && self.stake_deposit_unstake_total < self.stake_deposit_unstake_cap
        {
            let lamports_above_target = current_lamports
                .checked_sub(target_lamports)
                .ok_or(StewardError::ArithmeticError)?;

            let lamports_above_balance = current_lamports
                .checked_sub(state.validator_lamport_balances[index])
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

    fn instant_unstake(
        &self,
        state: &StewardState,
        index: usize,
        current_lamports: u64,
        target_lamports: u64,
    ) -> Result<u64> {
        // If this validator is marked for instant unstake, destake to the target
        if state.instant_unstake.get_unsafe(index)
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
        }
        Ok(0)
    }

    fn scoring_unstake(&self, current_lamports: u64, target_lamports: u64) -> Result<u64> {
        // If there are additional lamports to unstake on this validator and the total unstaked lamports is below the cap, destake to the target
        if self.scoring_unstake_total < self.scoring_unstake_cap {
            let lamports_above_target = current_lamports
                .checked_sub(target_lamports)
                .ok_or(StewardError::ArithmeticError)?;

            let cap_limit = self
                .scoring_unstake_cap
                .checked_sub(self.scoring_unstake_total)
                .ok_or(StewardError::ArithmeticError)?;

            let scoring_unstake_lamports = lamports_above_target.min(cap_limit);
            return Ok(scoring_unstake_lamports);
        }
        Ok(0)
    }
}
