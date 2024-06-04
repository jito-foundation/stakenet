use borsh::BorshSerialize;
use std::fmt::Display;

use crate::{
    bitmask::BitMask,
    constants::{MAX_VALIDATORS, SORTED_INDEX_DEFAULT},
    delegation::{
        decrease_stake_calculation, increase_stake_calculation, DecreaseComponents, RebalanceType,
        UnstakeState,
    },
    errors::StewardError,
    score::{instant_unstake_validator, validator_score},
    utils::{epoch_progress, get_target_lamports, stake_lamports_at_validator_list_index, U8Bool},
    Config, Parameters,
};
use anchor_lang::prelude::*;

use bytemuck::{Pod, Zeroable};
use spl_stake_pool::big_vec::BigVec;
use validator_history::{ClusterHistory, ValidatorHistory};

// Tests will fail here - comment out msg! to pass
fn invalid_state_error(expected: String, actual: String) -> Error {
    msg!("Invalid state. Expected {}, Actual {}", expected, actual);
    StewardError::InvalidState.into()
}

#[event]
pub struct StateTransition {
    epoch: u64,
    slot: u64,
    previous_state: String,
    new_state: String,
}

pub fn maybe_transition_and_emit(
    state_account: &mut StewardState,
    clock: &Clock,
    params: &Parameters,
    epoch_schedule: &EpochSchedule,
) -> Result<()> {
    let initial_state = state_account.state_tag.to_string();
    state_account.transition(clock, params, epoch_schedule)?;
    if initial_state != state_account.state_tag.to_string() {
        emit!(StateTransition {
            epoch: clock.epoch,
            slot: clock.slot,
            previous_state: initial_state,
            new_state: state_account.state_tag.to_string(),
        });
    }
    Ok(())
}

/// Tracks state of the stake pool.
/// Follow state transitions here: [TODO add link to github diagram]
#[derive(BorshSerialize)]
#[zero_copy]
pub struct StewardState {
    /// Current state of the Steward
    pub state_tag: StewardStateEnum,

    /////// Validator fields. Indices correspond to spl_stake_pool::ValidatorList index ///////
    /// Internal lamport balance of each validator, used to track stake deposits that need to be unstaked,
    /// so not always equal to the stake account balance.
    pub validator_lamport_balances: [u64; MAX_VALIDATORS],

    /// Overall score of validator, used to determine delegates and order for delegation.
    pub scores: [u32; MAX_VALIDATORS],

    /// Indices of validators, sorted by score descending
    pub sorted_score_indices: [u16; MAX_VALIDATORS],

    /// Yield component of the score. Used as secondary priority, to determine order for unstaking.
    pub yield_scores: [u32; MAX_VALIDATORS],

    /// Indices of validators, sorted by yield score descending
    pub sorted_yield_score_indices: [u16; MAX_VALIDATORS],

    /// Target share of pool represented as a proportion, indexed by spl_stake_pool::ValidatorList index
    pub delegations: [Delegation; MAX_VALIDATORS],

    /// Each bit represents a validator, true if validator should be unstaked
    pub instant_unstake: BitMask,

    /// Tracks progress of states that require one instruction per validator
    pub progress: BitMask,

    ////// Cycle metadata fields //////
    /// Slot of the first ComputeScores instruction in the current cycle
    pub start_computing_scores_slot: u64,

    /// Internal current epoch, for tracking when epoch has changed
    pub current_epoch: u64,

    /// Next cycle start
    pub next_cycle_epoch: u64,

    /// Number of validators in the stake pool, used to determine the number of validators to be scored.
    /// Updated at the start of each cycle and when validators are removed.
    pub num_pool_validators: usize,

    /// Total lamports that have been due to scoring this cycle
    pub scoring_unstake_total: u64,

    /// Total lamports that have been due to instant unstaking this cycle
    pub instant_unstake_total: u64,

    /// Total lamports that have been due to stake deposits this cycle
    pub stake_deposit_unstake_total: u64,

    /// Tracks whether delegation computation has been completed
    pub compute_delegations_completed: U8Bool,

    /// Tracks whether unstake and delegate steps have completed
    pub rebalance_completed: U8Bool,

    /// Future state and #[repr(C)] alignment
    pub _padding0: [u8; 6 + MAX_VALIDATORS * 8],
    // TODO ADD MORE PADDING
}

#[derive(Clone, Copy)]
#[repr(u64)]
pub enum StewardStateEnum {
    /// Start state
    /// Every `num_cycle_epochs` epochs, scores are computed and the top `num_delegation_validators` validators are selected.
    ComputeScores,

    /// Once scores are computed, the number of lamports assigned to each validator determined in this step
    ComputeDelegations,

    /// Once delegations are computed, the pool is idle until the 90% mark of the epoch
    Idle,

    /// Once at the 90% mark of the epoch, the pool checks if any validators have met kickable criteria
    ComputeInstantUnstake,

    /// Stake rebalances computed and executed, adjusting delegations if instant_unstake validators are hit
    /// Transition back to Idle, or ComputeScores if new cycle
    Rebalance,
}

#[derive(BorshSerialize, PartialEq, Eq)]
#[zero_copy]
pub struct Delegation {
    pub numerator: u32,
    pub denominator: u32,
}

impl Default for Delegation {
    fn default() -> Self {
        Self {
            numerator: 0,
            denominator: 1,
        }
    }
}

impl Delegation {
    pub const fn new(numerator: u32, denominator: u32) -> Self {
        Self {
            numerator,
            denominator,
        }
    }
}

impl AnchorSerialize for StewardStateEnum {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        (*self as u64).serialize(writer)
    }
}

// With unsafe impl, need to manually ensure that the guarantees of Pod and Zeroable are upheld
// I.e discriminator of zero and C-style alignment
unsafe impl Zeroable for StewardStateEnum {}
unsafe impl Pod for StewardStateEnum {}

impl Display for StewardStateEnum {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::ComputeScores => write!(f, "ComputeScores"),
            Self::ComputeDelegations => write!(f, "ComputeDelegations"),
            Self::Idle => write!(f, "Idle"),
            Self::ComputeInstantUnstake => {
                write!(f, "ComputeInstantUnstake")
            }
            Self::Rebalance => write!(f, "Rebalance"),
        }
    }
}

impl StewardState {
    /// Top level transition method. Tries to transition to a new state based on current state and epoch conditions
    pub fn transition(
        &mut self,
        clock: &Clock,
        params: &Parameters,
        epoch_schedule: &EpochSchedule,
    ) -> Result<()> {
        let current_epoch = clock.epoch;
        let current_slot = clock.slot;
        let epoch_progress = epoch_progress(clock, epoch_schedule)?;
        match self.state_tag {
            StewardStateEnum::ComputeScores => self.transition_compute_scores(
                current_epoch,
                current_slot,
                params.num_epochs_between_scoring,
            ),
            StewardStateEnum::ComputeDelegations => self.transition_compute_delegations(
                current_epoch,
                current_slot,
                params.num_epochs_between_scoring,
            ),
            StewardStateEnum::Idle => self.transition_idle(
                current_epoch,
                current_slot,
                params.num_epochs_between_scoring,
                epoch_progress,
                params.instant_unstake_epoch_progress,
            ),
            StewardStateEnum::ComputeInstantUnstake => self.transition_compute_instant_unstake(
                current_epoch,
                current_slot,
                params.num_epochs_between_scoring,
            ),
            StewardStateEnum::Rebalance => self.transition_rebalance(
                current_epoch,
                current_slot,
                params.num_epochs_between_scoring,
            ),
        }
    }

    #[inline]
    fn transition_compute_scores(
        &mut self,
        current_epoch: u64,
        current_slot: u64,
        num_epochs_between_scoring: u64,
    ) -> Result<()> {
        if current_epoch >= self.next_cycle_epoch {
            self.state_tag = StewardStateEnum::ComputeScores;
            self.reset_state_for_new_cycle(
                current_epoch,
                current_slot,
                num_epochs_between_scoring,
            )?;
        } else if self.progress.is_complete(self.num_pool_validators)? {
            self.state_tag = StewardStateEnum::ComputeDelegations;
            self.progress = BitMask::default();
            self.delegations = [Delegation::default(); MAX_VALIDATORS];
        }
        Ok(())
    }

    #[inline]
    fn transition_compute_delegations(
        &mut self,
        current_epoch: u64,
        current_slot: u64,
        num_epochs_between_scoring: u64,
    ) -> Result<()> {
        if current_epoch >= self.next_cycle_epoch {
            self.state_tag = StewardStateEnum::ComputeScores;
            self.reset_state_for_new_cycle(
                current_epoch,
                current_slot,
                num_epochs_between_scoring,
            )?;
        } else if self.compute_delegations_completed.into() {
            self.state_tag = StewardStateEnum::Idle;
            self.current_epoch = current_epoch;
            self.rebalance_completed = false.into();
        }
        Ok(())
    }

    #[inline]
    fn transition_idle(
        &mut self,
        current_epoch: u64,
        current_slot: u64,
        num_epochs_between_scoring: u64,
        epoch_progress: f64,
        min_epoch_progress_for_instant_unstake: f64,
    ) -> Result<()> {
        if current_epoch >= self.next_cycle_epoch {
            self.state_tag = StewardStateEnum::ComputeScores;
            self.reset_state_for_new_cycle(
                current_epoch,
                current_slot,
                num_epochs_between_scoring,
            )?;
        } else if (!self.rebalance_completed).into()
            && epoch_progress >= min_epoch_progress_for_instant_unstake
        {
            self.state_tag = StewardStateEnum::ComputeInstantUnstake;
            self.instant_unstake = BitMask::default();
            self.progress = BitMask::default();
        }
        Ok(())
    }

    #[inline]
    fn transition_compute_instant_unstake(
        &mut self,
        current_epoch: u64,
        current_slot: u64,
        num_epochs_between_scoring: u64,
    ) -> Result<()> {
        if current_epoch >= self.next_cycle_epoch {
            self.state_tag = StewardStateEnum::ComputeScores;
            self.reset_state_for_new_cycle(
                current_epoch,
                current_slot,
                num_epochs_between_scoring,
            )?;
        } else if current_epoch > self.current_epoch {
            self.state_tag = StewardStateEnum::Idle;
            self.current_epoch = current_epoch;
            self.instant_unstake = BitMask::default();
            self.progress = BitMask::default();
        } else if self.progress.is_complete(self.num_pool_validators)? {
            self.state_tag = StewardStateEnum::Rebalance;
            self.progress = BitMask::default();
        }
        Ok(())
    }

    #[inline]
    fn transition_rebalance(
        &mut self,
        current_epoch: u64,
        current_slot: u64,
        num_epochs_between_scoring: u64,
    ) -> Result<()> {
        if current_epoch >= self.next_cycle_epoch {
            self.state_tag = StewardStateEnum::ComputeScores;
            self.reset_state_for_new_cycle(
                current_epoch,
                current_slot,
                num_epochs_between_scoring,
            )?;
        } else if current_epoch > self.current_epoch {
            self.state_tag = StewardStateEnum::Idle;
            self.current_epoch = current_epoch;
            self.progress = BitMask::default();
            self.rebalance_completed = false.into();
        } else if self.progress.is_complete(self.num_pool_validators)? {
            self.state_tag = StewardStateEnum::Idle;
            self.current_epoch = current_epoch;
            self.rebalance_completed = true.into();
        }
        Ok(())
    }

    /// Update internal state when transitioning to a new cycle, and ComputeScores restarts
    fn reset_state_for_new_cycle(
        &mut self,
        current_epoch: u64,
        current_slot: u64,
        num_epochs_between_scoring: u64,
    ) -> Result<()> {
        self.state_tag = StewardStateEnum::ComputeScores;
        self.scores = [0; MAX_VALIDATORS];
        self.yield_scores = [0; MAX_VALIDATORS];
        self.progress = BitMask::default();
        self.current_epoch = current_epoch;
        self.next_cycle_epoch = current_epoch
            .checked_add(num_epochs_between_scoring)
            .ok_or(StewardError::ArithmeticError)?;
        self.start_computing_scores_slot = current_slot;
        self.scoring_unstake_total = 0;
        self.instant_unstake_total = 0;
        self.stake_deposit_unstake_total = 0;
        self.delegations = [Delegation::default(); MAX_VALIDATORS];
        self.instant_unstake = BitMask::default();
        self.compute_delegations_completed = false.into();
        self.rebalance_completed = false.into();
        Ok(())
    }

    /// Update internal state when a validator is removed from the pool
    pub fn remove_validator(&mut self, index: usize) -> Result<()> {
        self.num_pool_validators = self
            .num_pool_validators
            .checked_sub(1)
            .ok_or(StewardError::ArithmeticError)?;

        // Shift all validator state to the left
        for i in index..self.num_pool_validators {
            let next_i = i.checked_add(1).ok_or(StewardError::ArithmeticError)?;
            self.validator_lamport_balances[i] = self.validator_lamport_balances[next_i];
            self.scores[i] = self.scores[next_i];
            self.yield_scores[i] = self.yield_scores[next_i];
            self.delegations[i] = self.delegations[next_i];
            self.instant_unstake
                .set(i, self.instant_unstake.get(next_i)?)?;
            self.progress.set(i, self.progress.get(next_i)?)?;
        }
        // Update score indices
        let yield_score_index = self
            .sorted_yield_score_indices
            .iter()
            .position(|&i| i == index as u16)
            .ok_or(StewardError::ValidatorIndexOutOfBounds)?;
        let score_index = self
            .sorted_score_indices
            .iter()
            .position(|&i| i == index as u16)
            .ok_or(StewardError::ValidatorIndexOutOfBounds)?;

        for i in yield_score_index..self.num_pool_validators {
            let next_i = i.checked_add(1).ok_or(StewardError::ArithmeticError)?;
            self.sorted_yield_score_indices[i] = self.sorted_yield_score_indices[next_i];
        }

        for i in score_index..self.num_pool_validators {
            let next_i = i.checked_add(1).ok_or(StewardError::ArithmeticError)?;
            self.sorted_score_indices[i] = self.sorted_score_indices[next_i];
        }

        for i in 0..self.num_pool_validators {
            if self.sorted_yield_score_indices[i] as usize > index {
                self.sorted_yield_score_indices[i] = self.sorted_yield_score_indices[i]
                    .checked_sub(1)
                    .ok_or(StewardError::ArithmeticError)?;
            }
            if self.sorted_score_indices[i] as usize > index {
                self.sorted_score_indices[i] = self.sorted_score_indices[i]
                    .checked_sub(1)
                    .ok_or(StewardError::ArithmeticError)?;
            }
        }

        // Clear values on empty last index
        self.validator_lamport_balances[self.num_pool_validators] = 0;
        self.scores[self.num_pool_validators] = 0;
        self.yield_scores[self.num_pool_validators] = 0;
        self.sorted_score_indices[self.num_pool_validators] = SORTED_INDEX_DEFAULT;
        self.sorted_yield_score_indices[self.num_pool_validators] = SORTED_INDEX_DEFAULT;
        self.delegations[self.num_pool_validators] = Delegation::default();
        self.instant_unstake.set(self.num_pool_validators, false)?;
        self.progress.set(self.num_pool_validators, false)?;

        Ok(())
    }

    /// One instruction per validator. Can be done in any order.
    /// Computes score for a validator for the current epoch, stores score, and yield score component.
    /// Inserts this validator's index into sorted_score_indices and sorted_yield_score_indices, sorted by
    /// score and yield score respectively, descending.
    ///
    /// Mutates: scores, yield_scores, sorted_score_indices, sorted_yield_score_indices, progress
    #[allow(clippy::too_many_arguments)]
    pub fn compute_score(
        &mut self,
        clock: &Clock,
        epoch_schedule: &EpochSchedule,
        validator: &ValidatorHistory,
        index: usize,
        cluster: &ClusterHistory,
        config: &Config,
        num_pool_validators: usize,
    ) -> Result<()> {
        if matches!(self.state_tag, StewardStateEnum::ComputeScores) {
            let current_epoch = clock.epoch;
            let current_slot = clock.slot;

            // Check that latest_update_slot is within the current epoch to guarantee previous epoch data is complete
            let last_update_slot = validator
                .history
                .vote_account_last_update_slot_latest()
                .ok_or(StewardError::VoteHistoryNotRecentEnough)?;
            if last_update_slot < epoch_schedule.get_first_slot_in_epoch(current_epoch) {
                return Err(StewardError::VoteHistoryNotRecentEnough.into());
            }

            // Check that cluster history is within current epoch to guarantee previous epoch data is complete
            if cluster.cluster_history_last_update_slot
                < epoch_schedule.get_first_slot_in_epoch(current_epoch)
            {
                return Err(StewardError::ClusterHistoryNotRecentEnough.into());
            }

            /* Reset common state if:
                - it's a new delegation cycle
                - it's been more than `compute_score_slot_range` slots since compute scores started
                - computation started last epoch and it's a new epoch
            */
            let slots_since_scoring_started = current_slot
                .checked_sub(self.start_computing_scores_slot)
                .ok_or(StewardError::ArithmeticError)?;
            if self.progress.is_empty()
                || current_epoch > self.current_epoch
                || slots_since_scoring_started > config.parameters.compute_score_slot_range as u64
            {
                self.reset_state_for_new_cycle(
                    clock.epoch,
                    clock.slot,
                    config.parameters.num_epochs_between_scoring,
                )?;
                // Updates num_pool_validators at the start of the cycle so validator additions later won't be considered
                self.num_pool_validators = num_pool_validators;
            }

            let score = validator_score(validator, index, cluster, config, current_epoch as u16)?;
            emit!(score);

            self.scores[index] = (score.score * 1_000_000_000.) as u32;
            self.yield_scores[index] = (score.yield_score * 1_000_000_000.) as u32;

            // Insertion sort scores into sorted_indices
            let num_scores_calculated = self.progress.count();
            insert_sorted_index(
                &mut self.sorted_score_indices,
                &self.scores,
                index as u16,
                self.scores[index],
                num_scores_calculated,
            )?;
            insert_sorted_index(
                &mut self.sorted_yield_score_indices,
                &self.yield_scores,
                index as u16,
                self.yield_scores[index],
                num_scores_calculated,
            )?;

            self.progress.set(index, true)?;
            return Ok(());
        }
        Err(invalid_state_error(
            "ComputeScores".to_string(),
            self.state_tag.to_string(),
        ))
    }

    /// Given list of scores, finds top `num_delegation_validators` and assigns an equal share
    /// to each validator, represented as a fraction of total stake
    ///
    /// Mutates: delegations, compute_delegations_completed
    pub fn compute_delegations(&mut self, current_epoch: u64, config: &Config) -> Result<()> {
        if matches!(self.state_tag, StewardStateEnum::ComputeDelegations) {
            if current_epoch >= self.next_cycle_epoch {
                return Err(invalid_state_error(
                    "ComputeScores".to_string(),
                    self.state_tag.to_string(),
                ));
            }

            let validators_to_delegate = select_validators_to_delegate(
                &self.scores[..self.num_pool_validators],
                &self.sorted_score_indices[..self.num_pool_validators],
                config.parameters.num_delegation_validators as usize,
            );

            let num_delegation_validators = validators_to_delegate.len();

            // Assign equal share of pool to each validator
            for index in validators_to_delegate {
                self.delegations[index as usize] = Delegation {
                    numerator: 1,
                    denominator: num_delegation_validators as u32,
                };
            }

            self.compute_delegations_completed = true.into();

            return Ok(());
        }
        Err(invalid_state_error(
            "ComputeDelegations".to_string(),
            self.state_tag.to_string(),
        ))
    }

    /// One instruction per validator.
    /// Check a set of criteria that determine whether a validator should be kicked from the pool
    /// If so, set the validator.index bit in `instant_unstake` to true
    ///
    /// Mutates: instant_unstake, progress
    pub fn compute_instant_unstake(
        &mut self,
        clock: &Clock,
        epoch_schedule: &EpochSchedule,
        validator: &ValidatorHistory,
        index: usize,
        cluster: &ClusterHistory,
        config: &Config,
    ) -> Result<()> {
        if matches!(self.state_tag, StewardStateEnum::ComputeInstantUnstake) {
            if clock.epoch >= self.next_cycle_epoch {
                return Err(invalid_state_error(
                    "ComputeScores".to_string(),
                    self.state_tag.to_string(),
                ));
            }

            if epoch_progress(clock, epoch_schedule)?
                < config.parameters.instant_unstake_epoch_progress
            {
                return Err(StewardError::InstantUnstakeNotReady.into());
            }

            let first_slot = epoch_schedule.get_first_slot_in_epoch(clock.epoch);

            // Epoch credits and cluster history must be updated in the current epoch and after the midpoint of the epoch
            let min_acceptable_slot = first_slot
                .checked_add(
                    (epoch_schedule.get_slots_in_epoch(clock.epoch) as f64
                        * config.parameters.instant_unstake_inputs_epoch_progress)
                        .round() as u64,
                )
                .ok_or(StewardError::ArithmeticError)?;

            let last_update_slot = validator
                .history
                .vote_account_last_update_slot_latest()
                .ok_or(StewardError::VoteHistoryNotRecentEnough)?;
            if last_update_slot < min_acceptable_slot {
                return Err(StewardError::VoteHistoryNotRecentEnough.into());
            }
            if cluster.cluster_history_last_update_slot < min_acceptable_slot {
                return Err(StewardError::ClusterHistoryNotRecentEnough.into());
            }

            let instant_unstake_result = instant_unstake_validator(
                validator,
                index,
                cluster,
                config,
                first_slot,
                clock.epoch as u16,
            )?;
            emit!(instant_unstake_result);
            self.instant_unstake
                .set(index, instant_unstake_result.instant_unstake)?;
            self.progress.set(index, true)?;
            return Ok(());
        }
        Err(invalid_state_error(
            "ComputeInstantUnstake".to_string(),
            self.state_tag.to_string(),
        ))
    }

    /// One instruction per validator.
    /// Based on target delegation amounts, instant unstake status, reserve stake, and unstaking caps, this determines whether
    /// this validator should get more or less stake, and updates internal state. If the validator is being instant-unstaked,
    /// delegations are distributed to other eligible validators.
    /// stake_pool_lamports and reserve_lamports are the raw values from stake pool and reserve accounts, respectively, not adjusted for rent.
    ///
    ///
    /// Mutates: validator_lamport_balances delegations, unstake_total, stake_deposit_unstake_total, instant_unstake_total, progress
    #[allow(clippy::too_many_arguments)]
    pub fn rebalance(
        &mut self,
        current_epoch: u64,
        index: usize,
        validator_list: &BigVec<'_>,
        stake_pool_lamports: u64,
        reserve_lamports: u64,
        minimum_delegation: u64,
        stake_rent: u64,
        parameters: &Parameters,
    ) -> Result<RebalanceType> {
        if matches!(self.state_tag, StewardStateEnum::Rebalance) {
            if current_epoch >= self.next_cycle_epoch {
                return Err(invalid_state_error(
                    "ComputeScores".to_string(),
                    self.state_tag.to_string(),
                ));
            }
            let base_lamport_balance = minimum_delegation
                .checked_add(stake_rent)
                .ok_or(StewardError::ArithmeticError)?;

            // Maximum increase amount is the total lamports in the reserve stake account minus 2 * stake_rent, which accounts for reserve rent + transient rent
            // Saturating_sub because reserve stake may be less than 2 * stake_rent, but needs more than 2 * stake_rent to be able to delegate
            let reserve_lamports = reserve_lamports.saturating_sub(
                stake_rent
                    .checked_mul(2)
                    .ok_or(StewardError::ArithmeticError)?,
            );

            // Represents the amount of lamports that can be delegated to validators beyond the fixed costs of rent and minimum_delegation
            let stake_pool_lamports = stake_pool_lamports
                .checked_sub(
                    base_lamport_balance
                        .checked_mul(validator_list.len() as u64)
                        .ok_or(StewardError::ArithmeticError)?,
                )
                .ok_or(StewardError::ArithmeticError)?;

            let target_lamports =
                get_target_lamports(&self.delegations[index], stake_pool_lamports)?;

            let (mut current_lamports, some_transient_lamports) =
                stake_lamports_at_validator_list_index(validator_list, index)?;

            current_lamports = current_lamports.saturating_sub(base_lamport_balance);

            if !some_transient_lamports {
                /* This field is used to determine the amount of stake deposits this validator has gotten which push it over the target.
                This is important with calculating withdrawals: we can calculate current_lamports - validator_lamport_balances[index]
                to see the net stake deposits that should be unstaked.

                In all cases where the current_lamports is now below the target or internal balance, we update the internal balance.
                Otherwise, keep the internal balance the same to ensure we still see the stake deposit delta, until it can be unstaked.
                */
                self.validator_lamport_balances[index] = match (
                    current_lamports < self.validator_lamport_balances[index],
                    current_lamports < target_lamports,
                ) {
                    (true, true) => current_lamports,
                    (true, false) => current_lamports,
                    (false, true) => current_lamports,
                    (false, false) => self.validator_lamport_balances[index],
                }
            }

            let rebalance = if !some_transient_lamports
                && (target_lamports < current_lamports || self.instant_unstake.get(index)?)
            {
                let scoring_unstake_cap = stake_pool_lamports
                    .checked_mul(parameters.scoring_unstake_cap_bps as u64)
                    .and_then(|x| x.checked_div(10000))
                    .ok_or(StewardError::ArithmeticError)?;
                let instant_unstake_cap = stake_pool_lamports
                    .checked_mul(parameters.instant_unstake_cap_bps as u64)
                    .and_then(|x| x.checked_div(10000))
                    .ok_or(StewardError::ArithmeticError)?;
                let stake_deposit_unstake_cap = stake_pool_lamports
                    .checked_mul(parameters.stake_deposit_unstake_cap_bps as u64)
                    .and_then(|x| x.checked_div(10000))
                    .ok_or(StewardError::ArithmeticError)?;

                let unstake_state = UnstakeState {
                    stake_deposit_unstake_total: self.stake_deposit_unstake_total,
                    instant_unstake_total: self.instant_unstake_total,
                    scoring_unstake_total: self.scoring_unstake_total,
                    stake_deposit_unstake_cap,
                    instant_unstake_cap,
                    scoring_unstake_cap,
                };

                decrease_stake_calculation(
                    self,
                    index,
                    unstake_state,
                    stake_pool_lamports,
                    validator_list,
                    minimum_delegation,
                    stake_rent,
                )?
            } else if !some_transient_lamports && current_lamports < target_lamports {
                increase_stake_calculation(
                    self,
                    index,
                    current_lamports,
                    stake_pool_lamports,
                    validator_list,
                    reserve_lamports,
                    minimum_delegation,
                    stake_rent,
                )?
            } else {
                RebalanceType::None
            };

            // Update internal state based on rebalance
            match rebalance {
                RebalanceType::Decrease(DecreaseComponents {
                    scoring_unstake_lamports,
                    instant_unstake_lamports,
                    stake_deposit_unstake_lamports,
                    total_unstake_lamports,
                }) => {
                    emit!(DecreaseComponents {
                        scoring_unstake_lamports,
                        instant_unstake_lamports,
                        stake_deposit_unstake_lamports,
                        total_unstake_lamports,
                    });

                    self.validator_lamport_balances[index] = self.validator_lamport_balances[index]
                        .saturating_sub(total_unstake_lamports);

                    self.scoring_unstake_total = self
                        .scoring_unstake_total
                        .checked_add(scoring_unstake_lamports)
                        .ok_or(StewardError::ArithmeticError)?;

                    self.stake_deposit_unstake_total = self
                        .stake_deposit_unstake_total
                        .checked_add(stake_deposit_unstake_lamports)
                        .ok_or(StewardError::ArithmeticError)?;

                    self.instant_unstake_total = self
                        .instant_unstake_total
                        .checked_add(instant_unstake_lamports)
                        .ok_or(StewardError::ArithmeticError)?;

                    if instant_unstake_lamports > 0 && self.delegations[index].numerator > 0 {
                        // Ensure this validator gets no more stake and distribute the delegation to the other eligible
                        // by lowering their denominator
                        for i in 0..index {
                            if self.delegations[i].numerator > 0 {
                                self.delegations[i].denominator =
                                    self.delegations[i].denominator.saturating_sub(1).max(1);
                            }
                        }

                        let next_i = index.checked_add(1).ok_or(StewardError::ArithmeticError)?;
                        for i in next_i..self.num_pool_validators {
                            if self.delegations[i].numerator > 0 {
                                self.delegations[i].denominator =
                                    self.delegations[i].denominator.saturating_sub(1).max(1);
                            }
                        }
                        self.delegations[index] = Delegation {
                            numerator: 0,
                            denominator: 1,
                        };
                    }
                }
                RebalanceType::Increase(amount) => {
                    self.validator_lamport_balances[index] = self.validator_lamport_balances[index]
                        .checked_add(amount)
                        .ok_or(StewardError::ArithmeticError)?;
                }
                RebalanceType::None => {}
            }

            self.progress.set(index, true)?;
            return Ok(rebalance);
        }
        Err(invalid_state_error(
            "Rebalance".to_string(),
            self.state_tag.to_string(),
        ))
    }
}

/// Inserts index into sorted_indices at the correct position, shifting elements as needed. Sorted by score descending.
/// mutates `sorted_indices` in place
pub fn insert_sorted_index(
    sorted_indices: &mut [u16],
    scores: &[u32],
    index: u16,
    score: u32,
    current_len: usize,
) -> Result<()> {
    // Ensure the current_len is within the bounds of the sorted_indices slice
    assert!(current_len <= sorted_indices.len());

    // Find the correct position to insert the new index
    let position = sorted_indices[..current_len]
        .iter()
        .position(|&i| scores[i as usize] < score);

    // If no such position, insert at the end of the current elements
    let insert_at = position.unwrap_or(current_len);

    // Shift elements to the right to make room for the new index
    for i in (insert_at..current_len).rev() {
        let next_i = i.checked_add(1).ok_or(StewardError::ArithmeticError)?;
        sorted_indices[next_i] = sorted_indices[i];
    }

    // Insert the new index
    sorted_indices[insert_at] = index;
    Ok(())
}

/// Selects top `num_delegation_validators` validators by score descending.
/// If there are fewer than `num_delegation_validators` validators with non-zero scores, all non-zero scores are selected.
pub fn select_validators_to_delegate(
    scores: &[u32],
    sorted_score_indices: &[u16],
    num_delegation_validators: usize,
) -> Vec<u16> {
    let mut validators_to_delegate = Vec::with_capacity(num_delegation_validators);
    let last_valid_index = sorted_score_indices
        .iter()
        .position(|&i| scores[i as usize] == 0)
        .unwrap_or(num_delegation_validators);

    validators_to_delegate.extend(
        sorted_score_indices[..last_valid_index.min(num_delegation_validators)]
            .iter()
            .copied(),
    );

    validators_to_delegate
}
