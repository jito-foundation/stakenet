use borsh::BorshSerialize;
use std::fmt::Display;

use crate::{
    bitmask::BitMask,
    constants::{
        LAMPORT_BALANCE_DEFAULT, MAX_VALIDATORS, SORTED_INDEX_DEFAULT, TVC_ACTIVATION_EPOCH,
    },
    delegation::{
        decrease_stake_calculation, increase_stake_calculation, RebalanceType, UnstakeState,
    },
    errors::StewardError,
    events::{DecreaseComponents, StateTransition},
    score::{
        instant_unstake_validator, validator_score, InstantUnstakeComponentsV3, ScoreComponentsV5,
    },
    state::directed_stake::DirectedStakeMeta,
    utils::{epoch_progress, get_target_lamports},
    Config, Parameters,
};

#[cfg(feature = "idl-build")]
use anchor_lang::idl::types::*;
use anchor_lang::prelude::*;
#[cfg(feature = "idl-build")]
use anchor_lang::IdlBuild;

use bytemuck::{Pod, Zeroable};
use spl_stake_pool::big_vec::BigVec;
use validator_history::{ClusterHistory, ValidatorHistory};

pub fn maybe_transition(
    steward_state: &mut StewardStateV2,
    clock: &Clock,
    params: &Parameters,
    epoch_schedule: &EpochSchedule,
) -> Result<Option<StateTransition>> {
    let initial_state = steward_state.state_tag.to_string();
    steward_state.transition(clock, params, epoch_schedule)?;

    if initial_state != steward_state.state_tag.to_string() {
        return Ok(Some(StateTransition {
            epoch: clock.epoch,
            slot: clock.slot,
            previous_state: initial_state,
            new_state: steward_state.state_tag.to_string(),
        }));
    }
    Ok(None)
}

/// V1 State - Pure POD struct for deserialization of existing accounts
/// DO NOT ADD ANY IMPLEMENTATIONS TO THIS STRUCT
#[zero_copy]
pub struct StewardStateV1 {
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

    /// Marks a validator for immediate removal after `remove_validator_from_pool` has been called on the stake pool
    /// This happens when a validator is able to be removed within the same epoch as it was marked
    pub validators_for_immediate_removal: BitMask,

    /// Marks a validator for removal after `remove_validator_from_pool` has been called on the stake pool
    /// This is cleaned up in the next epoch
    pub validators_to_remove: BitMask,

    ////// Cycle metadata fields //////
    /// Slot of the first ComputeScores instruction in the current cycle
    pub start_computing_scores_slot: u64,

    /// Internal current epoch, for tracking when epoch has changed
    pub current_epoch: u64,

    /// Next cycle start
    pub next_cycle_epoch: u64,

    /// Number of validators in the stake pool, used to determine the number of validators to be scored.
    /// Updated at the start of each cycle and when validators are removed.
    pub num_pool_validators: u64,

    /// Total lamports that have been due to scoring this cycle
    pub scoring_unstake_total: u64,

    /// Total lamports that have been due to instant unstaking this cycle
    pub instant_unstake_total: u64,

    /// Total lamports that have been due to stake deposits this cycle
    pub stake_deposit_unstake_total: u64,

    /// Flags to track state transitions and operations
    pub status_flags: u32,

    /// Number of validators added to the pool in the current cycle
    pub validators_added: u16,

    /// Future state and #[repr(C)] alignment
    pub _padding0: [u8; STATE_PADDING_0_SIZE_V1],
}

pub const STATE_PADDING_0_SIZE_V1: usize = MAX_VALIDATORS * 8 + 2;

/// Tracks state of the stake pool.
/// Follow state transitions here:
/// https://github.com/jito-foundation/stakenet/blob/master/programs/steward/state-machine-diagram.png
#[zero_copy]
pub struct StewardStateV2 {
    /// Current state of the Steward
    pub state_tag: StewardStateEnum,

    /////// Validator fields. Indices correspond to spl_stake_pool::ValidatorList index ///////
    /// Internal lamport balance of each validator, used to track stake deposits that need to be unstaked,
    /// so not always equal to the stake account balance.
    pub validator_lamport_balances: [u64; MAX_VALIDATORS],

    /// Overall score of validator, used to determine delegates and order for delegation.
    pub scores: [u64; MAX_VALIDATORS],

    /// Indices of validators, sorted by score descending
    pub sorted_score_indices: [u16; MAX_VALIDATORS],

    /// Indices of validators, sorted by raw score descending
    pub sorted_raw_score_indices: [u16; MAX_VALIDATORS],

    /// Target share of pool represented as a proportion, indexed by spl_stake_pool::ValidatorList index
    pub delegations: [Delegation; MAX_VALIDATORS],

    /// Each bit represents a validator, true if validator should be unstaked
    pub instant_unstake: BitMask,

    /// Tracks progress of states that require one instruction per validator
    pub progress: BitMask,

    /// Marks a validator for immediate removal after `remove_validator_from_pool` has been called on the stake pool
    /// This happens when a validator is able to be removed within the same epoch as it was marked
    pub validators_for_immediate_removal: BitMask,

    /// Marks a validator for removal after `remove_validator_from_pool` has been called on the stake pool
    /// This is cleaned up in the next epoch
    pub validators_to_remove: BitMask,

    ////// Cycle metadata fields //////
    /// Slot of the first ComputeScores instruction in the current cycle
    pub start_computing_scores_slot: u64,

    /// Internal current epoch, for tracking when epoch has changed
    pub current_epoch: u64,

    /// Next cycle start
    pub next_cycle_epoch: u64,

    /// Number of validators in the stake pool, used to determine the number of validators to be scored.
    /// Updated at the start of each cycle and when validators are removed.
    pub num_pool_validators: u64,

    /// Total lamports that have been due to scoring this cycle
    pub scoring_unstake_total: u64,

    /// Total lamports that have been due to instant unstaking this cycle
    pub instant_unstake_total: u64,

    /// Total lamports that have been due to stake deposits this cycle
    pub stake_deposit_unstake_total: u64,

    /// Flags to track state transitions and operations
    pub status_flags: u32,

    /// Number of validators added to the pool in the current cycle
    pub validators_added: u16,
    pub _padding0: [u8; 2],

    /// Raw score without binary filters applied. Used as secondary priority, to determine order for unstaking.
    pub raw_scores: [u64; MAX_VALIDATORS],
    // TODO ADD MORE PADDING
}

pub const STATE_PADDING_0_SIZE: usize = (MAX_VALIDATORS * 8 + 2) - 8;

#[derive(Clone, Copy, PartialEq, Debug)]
#[repr(u64)]
pub enum StewardStateEnum {
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

    /// Start state
    /// Rebalance directed stake
    RebalanceDirected,
}

#[derive(BorshSerialize, PartialEq, Eq, Debug)]
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
            Self::RebalanceDirected => write!(f, "RebalanceDirected"),
        }
    }
}

#[cfg(feature = "idl-build")]
impl IdlBuild for StewardStateEnum {
    fn get_full_path() -> String {
        "StewardStateEnum".to_string()
    }

    fn create_type() -> Option<IdlTypeDef> {
        Some(IdlTypeDef {
            name: "StewardStateEnum".to_string(),
            ty: IdlTypeDefTy::Enum {
                variants: vec![
                    IdlEnumVariant {
                        name: "ComputeScores".to_string(),
                        fields: None,
                    },
                    IdlEnumVariant {
                        name: "ComputeDelegations".to_string(),
                        fields: None,
                    },
                    IdlEnumVariant {
                        name: "Idle".to_string(),
                        fields: None,
                    },
                    IdlEnumVariant {
                        name: "ComputeInstantUnstake".to_string(),
                        fields: None,
                    },
                    IdlEnumVariant {
                        name: "Rebalance".to_string(),
                        fields: None,
                    },
                    IdlEnumVariant {
                        name: "RebalanceDirected".to_string(),
                        fields: None,
                    },
                ],
            },
            docs: Default::default(),
            generics: Default::default(),
            serialization: Default::default(),
            repr: Default::default(),
        })
    }

    fn insert_types(_types: &mut std::collections::BTreeMap<String, IdlTypeDef>) {}
}

// BITS 0-7 COMPLETED PROGRESS FLAGS
// Used to mark the completion of a particular state
pub const COMPUTE_SCORE: u32 = 1 << 0;
pub const COMPUTE_DELEGATIONS: u32 = 1 << 1;
pub const EPOCH_MAINTENANCE: u32 = 1 << 2;
pub const PRE_LOOP_IDLE: u32 = 1 << 3;
pub const COMPUTE_INSTANT_UNSTAKES: u32 = 1 << 4;
pub const REBALANCE: u32 = 1 << 5;
pub const POST_LOOP_IDLE: u32 = 1 << 6;
pub const REBALANCE_DIRECTED_COMPLETE: u32 = 1 << 7;
// BITS 8-15 RESERVED FOR FUTURE USE
// BITS 16-23 OPERATIONAL FLAGS
/// In epoch maintenance, when a new epoch is detected, we need a flag to tell the
/// state transition layer that it needs to be reset to the IDLE state
/// this flag is set in in epoch_maintenance and unset in the IDLE state transition
pub const RESET_TO_IDLE: u32 = 1 << 16;
// BITS 24-31 RESERVED FOR FUTURE USE

impl StewardStateV2 {
    pub fn set_flag(&mut self, flag: u32) {
        self.status_flags |= flag;
    }

    pub fn clear_flags(&mut self) {
        self.status_flags = 0;
    }

    pub fn unset_flag(&mut self, flag: u32) {
        self.status_flags &= !flag;
    }

    pub fn has_flag(&self, flag: u32) -> bool {
        self.status_flags & flag != 0
    }

    /// Top level transition method. Tries to transition to a new state based on current state and epoch conditions
    pub fn transition(
        &mut self,
        clock: &Clock,
        params: &Parameters,
        epoch_schedule: &EpochSchedule,
    ) -> Result<()> {
        let current_epoch = clock.epoch;
        let epoch_progress = epoch_progress(clock, epoch_schedule)?;
        match self.state_tag {
            StewardStateEnum::ComputeScores => self.transition_compute_scores(),
            StewardStateEnum::ComputeDelegations => self.transition_compute_delegations(),
            StewardStateEnum::Idle => self.transition_idle(
                current_epoch,
                epoch_progress,
                params.instant_unstake_epoch_progress,
                params.compute_score_epoch_progress,
            ),
            StewardStateEnum::ComputeInstantUnstake => self.transition_compute_instant_unstake(),
            StewardStateEnum::Rebalance => self.transition_rebalance(),
            StewardStateEnum::RebalanceDirected => self
                .transition_rebalance_directed(epoch_progress, params.compute_score_epoch_progress),
        }
    }

    #[inline]
    fn transition_compute_scores(&mut self) -> Result<()> {
        if self.progress.is_complete(self.num_pool_validators)? {
            self.state_tag = StewardStateEnum::ComputeDelegations;
            self.progress = BitMask::default();
            self.delegations = [Delegation::default(); MAX_VALIDATORS];
            self.set_flag(COMPUTE_SCORE);
        }
        Ok(())
    }

    #[inline]
    fn transition_compute_delegations(&mut self) -> Result<()> {
        if self.has_flag(COMPUTE_DELEGATIONS) {
            self.state_tag = StewardStateEnum::Idle;
        }
        Ok(())
    }

    #[inline]
    fn transition_idle(
        &mut self,
        current_epoch: u64,
        epoch_progress: f64,
        min_epoch_progress_for_instant_unstake: f64,
        min_epoch_progress_for_compute_scores: f64,
    ) -> Result<()> {
        if current_epoch >= self.next_cycle_epoch {
            self.unset_flag(COMPUTE_SCORE);
            self.unset_flag(COMPUTE_DELEGATIONS);
            self.instant_unstake = BitMask::default();
            self.progress = BitMask::default();
        }

        let completed_loop = self.has_flag(REBALANCE);
        let completed_directed_rebalance = self.has_flag(REBALANCE_DIRECTED_COMPLETE);
        let completed_compute_delegations = self.has_flag(COMPUTE_DELEGATIONS);

        if completed_directed_rebalance
            && !completed_compute_delegations
            && epoch_progress >= min_epoch_progress_for_compute_scores
        {
            self.progress = BitMask::default();
            self.state_tag = StewardStateEnum::ComputeScores;
            self.instant_unstake = BitMask::default();
        } else if !completed_loop {
            self.unset_flag(RESET_TO_IDLE);
            self.set_flag(PRE_LOOP_IDLE);

            if epoch_progress >= min_epoch_progress_for_instant_unstake {
                self.state_tag = StewardStateEnum::ComputeInstantUnstake;
                self.instant_unstake = BitMask::default();
                self.progress = BitMask::default();
            }
        } else if completed_loop {
            self.set_flag(POST_LOOP_IDLE)
        }

        Ok(())
    }

    #[inline]
    fn transition_compute_instant_unstake(&mut self) -> Result<()> {
        if self.progress.is_complete(self.num_pool_validators)? {
            self.state_tag = StewardStateEnum::Rebalance;
            self.progress = BitMask::default();
            self.set_flag(COMPUTE_INSTANT_UNSTAKES);
        }
        Ok(())
    }

    #[inline]
    fn transition_rebalance(&mut self) -> Result<()> {
        if self.has_flag(RESET_TO_IDLE) {
            self.state_tag = StewardStateEnum::Idle;
            self.progress = BitMask::default();
            // NOTE: RESET_TO_IDLE is cleared in the Idle transition
        } else if self.progress.is_complete(self.num_pool_validators)? {
            self.state_tag = StewardStateEnum::Idle;
            self.set_flag(REBALANCE);
        }
        Ok(())
    }

    #[inline]
    fn transition_rebalance_directed(
        &mut self,
        epoch_progress: f64,
        min_epoch_progress_for_compute_scores: f64,
    ) -> Result<()> {
        let directed_rebalance_complete = self.has_flag(REBALANCE_DIRECTED_COMPLETE);
        if directed_rebalance_complete {
            self.state_tag = StewardStateEnum::Idle;
        } else if epoch_progress >= min_epoch_progress_for_compute_scores {
            // Do not stall the state machine if directed rebalance is not complete by the epoch
            // midpoint, undirected stake should be uninterrupted
            self.set_flag(REBALANCE_DIRECTED_COMPLETE);
            self.state_tag = StewardStateEnum::Idle;
        }
        Ok(())
    }

    /// Update internal state when transitioning to a new cycle, and ComputeScores restarts
    pub fn reset_state_for_new_cycle(
        &mut self,
        current_epoch: u64,
        current_slot: u64,
        num_epochs_between_scoring: u64,
    ) -> Result<()> {
        self.scores = [0; MAX_VALIDATORS];
        self.raw_scores = [0; MAX_VALIDATORS];
        self.progress = BitMask::default();
        self.next_cycle_epoch = current_epoch
            .checked_add(num_epochs_between_scoring)
            .ok_or(StewardError::ArithmeticError)?;
        self.start_computing_scores_slot = current_slot;
        self.scoring_unstake_total = 0;
        self.instant_unstake_total = 0;
        self.stake_deposit_unstake_total = 0;
        self.delegations = [Delegation::default(); MAX_VALIDATORS];
        self.instant_unstake = BitMask::default();

        let has_epoch_maintenance = self.has_flag(EPOCH_MAINTENANCE);
        let has_rebalance_directed_complete = self.has_flag(REBALANCE_DIRECTED_COMPLETE);
        self.clear_flags();
        if has_epoch_maintenance {
            self.set_flag(EPOCH_MAINTENANCE);
        }
        if has_rebalance_directed_complete {
            self.set_flag(REBALANCE_DIRECTED_COMPLETE);
        }

        Ok(())
    }

    /// Update internal state when a validator is removed from the pool
    pub fn remove_validator(
        &mut self,
        index: usize,
        directed_stake_meta: &mut DirectedStakeMeta,
    ) -> Result<()> {
        let marked_for_regular_removal = self.validators_to_remove.get(index)?;
        let marked_for_immediate_removal = self.validators_for_immediate_removal.get(index)?;

        require!(
            marked_for_regular_removal || marked_for_immediate_removal,
            StewardError::ValidatorNotMarkedForRemoval
        );

        let num_pool_validators = self.num_pool_validators as usize;
        let num_pool_validators_plus_added = num_pool_validators + self.validators_added as usize;

        require!(
            index < num_pool_validators_plus_added,
            StewardError::ValidatorIndexOutOfBounds
        );

        // If the validator was marked for removal in the current cycle, decrement validators_added
        if index >= num_pool_validators {
            self.validators_added = self
                .validators_added
                .checked_sub(1)
                .ok_or(StewardError::ArithmeticError)?;
        } else {
            self.num_pool_validators = self
                .num_pool_validators
                .checked_sub(1)
                .ok_or(StewardError::ArithmeticError)?;
        }

        // Refresh range bounds after decrement
        let num_pool_validators = self.num_pool_validators as usize;
        let num_pool_validators_plus_added = num_pool_validators + self.validators_added as usize;

        // Shift all validator state to the left
        for i in index..num_pool_validators {
            let next_i = i.checked_add(1).ok_or(StewardError::ArithmeticError)?;
            self.validator_lamport_balances[i] = self.validator_lamport_balances[next_i];
            self.scores[i] = self.scores[next_i];
            self.raw_scores[i] = self.raw_scores[next_i];
            self.delegations[i] = self.delegations[next_i];
            self.instant_unstake
                .set(i, self.instant_unstake.get(next_i)?)?;
            self.progress.set(i, self.progress.get(next_i)?)?;
            directed_stake_meta.directed_stake_lamports[i] =
                directed_stake_meta.directed_stake_lamports[next_i];
            directed_stake_meta.directed_stake_meta_indices[i] =
                directed_stake_meta.directed_stake_meta_indices[next_i];
        }

        // For state that can be valid past num_pool_validators, we still need to shift the values
        for i in index..num_pool_validators_plus_added {
            let next_i = i.checked_add(1).ok_or(StewardError::ArithmeticError)?;
            self.validators_to_remove
                .set(i, self.validators_to_remove.get(next_i)?)?;
            self.validators_for_immediate_removal
                .set(i, self.validators_for_immediate_removal.get(next_i)?)?;
        }

        // Update score indices
        let raw_score_index = self
            .sorted_raw_score_indices
            .iter()
            .position(|&i| i == index as u16);
        let score_index = self
            .sorted_score_indices
            .iter()
            .position(|&i| i == index as u16);

        if let Some(raw_score_index) = raw_score_index {
            for i in raw_score_index..num_pool_validators {
                let next_i = i.checked_add(1).ok_or(StewardError::ArithmeticError)?;
                self.sorted_raw_score_indices[i] = self.sorted_raw_score_indices[next_i];
            }
        }

        if let Some(score_index) = score_index {
            for i in score_index..num_pool_validators {
                let next_i = i.checked_add(1).ok_or(StewardError::ArithmeticError)?;
                self.sorted_score_indices[i] = self.sorted_score_indices[next_i];
            }
        }

        for i in 0..num_pool_validators {
            if self.sorted_raw_score_indices[i] as usize > index {
                self.sorted_raw_score_indices[i] = self.sorted_raw_score_indices[i]
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
        self.validator_lamport_balances[num_pool_validators] = LAMPORT_BALANCE_DEFAULT;
        directed_stake_meta.directed_stake_lamports[num_pool_validators] = 0;
        directed_stake_meta.directed_stake_meta_indices[num_pool_validators] = u64::MAX;
        self.scores[num_pool_validators] = 0;
        self.raw_scores[num_pool_validators] = 0;
        self.sorted_score_indices[num_pool_validators] = SORTED_INDEX_DEFAULT;
        self.sorted_raw_score_indices[num_pool_validators] = SORTED_INDEX_DEFAULT;
        self.delegations[num_pool_validators] = Delegation::default();
        self.instant_unstake.set(num_pool_validators, false)?;
        self.progress.set(num_pool_validators, false)?;
        self.validators_to_remove
            .set(num_pool_validators_plus_added, false)?;
        self.validators_for_immediate_removal
            .set(num_pool_validators_plus_added, false)?;

        Ok(())
    }

    /// Mark a validator for removal from the pool - this happens right after
    /// `remove_validator_from_pool` has been called on the stake pool
    /// This is cleaned up in the next epoch
    pub fn mark_validator_for_removal(&mut self, index: usize) -> Result<()> {
        self.validators_to_remove.set(index, true)
    }

    pub fn mark_validator_for_immediate_removal(&mut self, index: usize) -> Result<()> {
        self.validators_for_immediate_removal.set(index, true)
    }

    /// Called when adding a validator to the pool so that we can ensure a 1-1 mapping between
    /// the validator list and the steward state
    pub fn increment_validator_to_add(&mut self) -> Result<()> {
        self.validators_added = self
            .validators_added
            .checked_add(1)
            .ok_or(StewardError::ArithmeticError)?;
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
        num_pool_validators: u64,
    ) -> Result<Option<ScoreComponentsV5>> {
        if matches!(self.state_tag, StewardStateEnum::ComputeScores) {
            let current_epoch = clock.epoch;
            let current_slot = clock.slot;

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
                || slots_since_scoring_started > config.parameters.compute_score_slot_range
            {
                self.reset_state_for_new_cycle(
                    clock.epoch,
                    clock.slot,
                    config.parameters.num_epochs_between_scoring,
                )?;
                // Updates num_pool_validators at the start of the cycle so validator additions later won't be considered

                require!(
                    num_pool_validators == self.num_pool_validators + self.validators_added as u64,
                    StewardError::ListStateMismatch
                );
                self.num_pool_validators = num_pool_validators;
                self.validators_added = 0;
            }
            // Skip scoring if already processed
            if self.progress.get(index)? {
                return Ok(None);
            }

            // Skip scoring if marked for deletion
            if self.validators_to_remove.get(index)?
                || self.validators_for_immediate_removal.get(index)?
            {
                self.scores[index] = 0_u64;
                self.raw_scores[index] = 0_u64;

                let num_scores_calculated = self.progress.count();
                insert_sorted_index(
                    &mut self.sorted_score_indices,
                    &self.scores,
                    index as u16,
                    self.scores[index],
                    num_scores_calculated,
                )?;
                insert_sorted_index(
                    &mut self.sorted_raw_score_indices,
                    &self.raw_scores,
                    index as u16,
                    self.raw_scores[index],
                    num_scores_calculated,
                )?;

                self.progress.set(index, true)?;

                return Ok(None);
            }

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

            // Calculate score with binary filters applied
            let score_components = validator_score(
                validator,
                cluster,
                config,
                current_epoch as u16,
                TVC_ACTIVATION_EPOCH,
            )?;

            // Store both raw score and final score
            self.raw_scores[index] = score_components.raw_score;
            self.scores[index] = score_components.score;

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
                &mut self.sorted_raw_score_indices,
                &self.raw_scores,
                index as u16,
                self.raw_scores[index],
                num_scores_calculated,
            )?;

            self.progress.set(index, true)?;
            return Ok(Some(score_components));
        }

        msg!("Steward state invalid for compute_score");
        Err(StewardError::InvalidState.into())
    }

    /// Given list of scores, finds top `num_delegation_validators` and assigns an equal share
    /// to each validator, represented as a fraction of total stake
    ///
    /// Mutates: delegations, compute_delegations_completed
    pub fn compute_delegations(&mut self, current_epoch: u64, config: &Config) -> Result<()> {
        if matches!(self.state_tag, StewardStateEnum::ComputeDelegations) {
            if current_epoch >= self.next_cycle_epoch {
                return Err(StewardError::InvalidState.into());
            }

            let validators_to_delegate = select_validators_to_delegate(
                &self.scores[..self.num_pool_validators as usize],
                &self.sorted_score_indices[..self.num_pool_validators as usize],
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

            self.set_flag(COMPUTE_DELEGATIONS);

            return Ok(());
        }
        Err(StewardError::InvalidState.into())
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
    ) -> Result<Option<InstantUnstakeComponentsV3>> {
        if matches!(self.state_tag, StewardStateEnum::ComputeInstantUnstake) {
            if clock.epoch >= self.next_cycle_epoch {
                return Err(StewardError::InvalidState.into());
            }

            if epoch_progress(clock, epoch_schedule)?
                < config.parameters.instant_unstake_epoch_progress
            {
                return Err(StewardError::InstantUnstakeNotReady.into());
            }

            // Skip if already processed
            if self.progress.get(index)? {
                return Ok(None);
            }

            // Skip if marked for deletion
            if self.validators_to_remove.get(index)?
                || self.validators_for_immediate_removal.get(index)?
            {
                self.progress.set(index, true)?;
                return Ok(None);
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
                cluster,
                config,
                first_slot,
                clock.epoch as u16,
                TVC_ACTIVATION_EPOCH,
            )?;

            self.instant_unstake
                .set(index, instant_unstake_result.instant_unstake)?;
            self.progress.set(index, true)?;
            return Ok(Some(instant_unstake_result));
        }
        Err(StewardError::InvalidState.into())
    }

    pub fn simulate_adjust_directed_stake_for_deposits_and_withdrawals(
        &self,
        target_total_staked_lamports: u64,
        validator_list_index: usize,
        directed_stake_meta_index: usize,
        directed_stake_meta: &DirectedStakeMeta,
    ) -> Result<(u64, u64)> {
        if directed_stake_meta.directed_stake_meta_indices[validator_list_index] == u64::MAX {
            return Ok((0, target_total_staked_lamports));
        }
        let (mut new_directed_stake_lamports, mut new_total_stake_lamports) = (0u64, 0u64);
        let steward_state_total_lamports = self.validator_lamport_balances[validator_list_index];
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
                // We subtract the withdrawal lamport from the validator_lamport_balance
                // this in tandem with setting directed stake to 0 ensures that the remainder
                // is subtracted from the undirected stake.
                //
                // When we subtract more from the validator_lamport_balance than the directed stake,
                // the remainder is subtracted from the undirected stake.
                //
                // Ex. 25M total lamports, 1M directed stake, 24M undirected stake, 3M Withdrawal
                // 22M total lamports, 0 directed stake, 22M undirected stake
                new_total_stake_lamports = self.validator_lamport_balances[validator_list_index]
                    .saturating_sub(withdrawal_lamports);
                new_directed_stake_lamports = 0;
            } else {
                new_directed_stake_lamports = directed_stake_meta.directed_stake_lamports
                    [validator_list_index]
                    .saturating_sub(withdrawal_lamports);
                new_total_stake_lamports = self.validator_lamport_balances[validator_list_index]
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
            new_directed_stake_lamports = directed_stake_meta.directed_stake_lamports
                [validator_list_index]
                .saturating_add(increase_lamports);
            new_total_stake_lamports = self.validator_lamport_balances[validator_list_index]
                .saturating_add(increase_lamports);
        }
        Ok((new_directed_stake_lamports, new_total_stake_lamports))
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
        directed_stake_meta: &DirectedStakeMeta,
        current_epoch: u64,
        index: usize,
        validator_list: &BigVec<'_>,
        stake_pool_lamports: u64,
        reserve_lamports: u64,
        stake_account_current_lamports: u64,
        minimum_delegation: u64,
        stake_rent: u64,
        parameters: &Parameters,
    ) -> Result<RebalanceType> {
        if matches!(self.state_tag, StewardStateEnum::Rebalance) {
            if current_epoch >= self.next_cycle_epoch {
                return Err(StewardError::InvalidState.into());
            }

            // Skip if already processed
            if self.progress.get(index)? {
                return Ok(RebalanceType::None);
            }

            // Skip if marked for deletion
            if self.validators_to_remove.get(index)?
                || self.validators_for_immediate_removal.get(index)?
            {
                self.progress.set(index, true)?;
                msg!("Validator marked for deletion");
                return Ok(RebalanceType::None);
            }

            let base_lamport_balance = minimum_delegation
                .checked_add(stake_rent)
                .ok_or(StewardError::ArithmeticError)?;

            // Maximum increase amount is the total lamports in the reserve stake account minus (num_validators + 1) * stake_rent, which covers rent for all validators plus the transient rent
            let all_accounts_needed_reserve_for_rent = validator_list
                .len()
                .checked_add(1)
                .ok_or(StewardError::ArithmeticError)?;

            let accounts_left_needed_reserve_for_rent = all_accounts_needed_reserve_for_rent
                .checked_sub(self.progress.count() as u32)
                .ok_or(StewardError::ArithmeticError)?;

            let reserve_minimum = stake_rent
                .checked_mul(accounts_left_needed_reserve_for_rent as u64)
                .ok_or(StewardError::ArithmeticError)?;
            // Saturating_sub because reserve stake may be less than the reserve_minimum but needs more than the reserve_minimum to be able to delegate
            let reserve_lamports = reserve_lamports.saturating_sub(reserve_minimum);

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

            let directed_stake_lamports = directed_stake_meta.directed_stake_lamports[index];
            let current_total_lamports =
                stake_account_current_lamports.saturating_add(base_lamport_balance);
            let current_undirected_lamports =
                stake_account_current_lamports.saturating_sub(directed_stake_lamports);

            /* This field is used to determine the amount of stake deposits this validator has gotten which push it over the target.
            This is important with calculating withdrawals: we can calculate current_lamports - validator_lamport_balances[index]
            to see the net stake deposits that should be unstaked.

            In all cases where the current_lamports is now below the target or internal balance, we update the internal balance.
            Otherwise, keep the internal balance the same to ensure we still see the stake deposit delta, until it can be unstaked.
            */

            self.validator_lamport_balances[index] = match (
                current_total_lamports < self.validator_lamport_balances[index],
                current_undirected_lamports < target_lamports,
            ) {
                (true, true) => current_total_lamports,
                (true, false) => current_total_lamports,
                (false, true) => current_total_lamports,
                (false, false) => self.validator_lamport_balances[index],
            };

            let rebalance = if target_lamports < current_undirected_lamports
                || self.instant_unstake.get(index)?
            {
                let scoring_unstake_cap: u64 = (stake_pool_lamports as u128)
                    .checked_mul(parameters.scoring_unstake_cap_bps as u128)
                    .and_then(|x| x.checked_div(10_000))
                    .ok_or(StewardError::ArithmeticError)?
                    .try_into()
                    .map_err(|_| StewardError::ArithmeticCastError)?;
                let instant_unstake_cap: u64 = (stake_pool_lamports as u128)
                    .checked_mul(parameters.instant_unstake_cap_bps as u128)
                    .and_then(|x| x.checked_div(10_000))
                    .ok_or(StewardError::ArithmeticError)?
                    .try_into()
                    .map_err(|_| StewardError::ArithmeticCastError)?;
                let stake_deposit_unstake_cap: u64 = (stake_pool_lamports as u128)
                    .checked_mul(parameters.stake_deposit_unstake_cap_bps as u128)
                    .and_then(|x| x.checked_div(10_000))
                    .ok_or(StewardError::ArithmeticError)?
                    .try_into()
                    .map_err(|_| StewardError::ArithmeticCastError)?;

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
                    directed_stake_meta,
                    index,
                    unstake_state,
                    current_undirected_lamports,
                    stake_pool_lamports,
                    validator_list,
                    minimum_delegation,
                    stake_rent,
                )?
            } else if current_undirected_lamports < target_lamports {
                increase_stake_calculation(
                    self,
                    directed_stake_meta,
                    index,
                    current_undirected_lamports,
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
                    directed_unstake_lamports: _,
                }) => {
                    if self.validator_lamport_balances[index] != LAMPORT_BALANCE_DEFAULT {
                        self.validator_lamport_balances[index] = self.validator_lamport_balances
                            [index]
                            .saturating_sub(total_unstake_lamports);
                    }
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
                        for i in next_i..self.num_pool_validators as usize {
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
                    if self.validator_lamport_balances[index] != LAMPORT_BALANCE_DEFAULT {
                        self.validator_lamport_balances[index] = self.validator_lamport_balances
                            [index]
                            .checked_add(amount)
                            .ok_or(StewardError::ArithmeticError)?;
                    }
                }
                RebalanceType::None => {}
            }

            self.progress.set(index, true)?;
            return Ok(rebalance);
        }
        Err(StewardError::InvalidState.into())
    }
}

/// Inserts index into sorted_indices at the correct position, shifting elements as needed. Sorted by score descending.
/// mutates `sorted_indices` in place
pub fn insert_sorted_index(
    sorted_indices: &mut [u16],
    scores: &[u64],
    index: u16,
    score: u64,
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
    scores: &[u64],
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

#[cfg(test)]
mod tests {
    use crate::constants::SORTED_INDEX_DEFAULT;

    use super::*;

    fn default_state() -> StewardStateV2 {
        StewardStateV2 {
            state_tag: StewardStateEnum::Idle,
            validator_lamport_balances: [0; MAX_VALIDATORS],
            scores: [0; MAX_VALIDATORS],
            sorted_score_indices: [SORTED_INDEX_DEFAULT; MAX_VALIDATORS],
            raw_scores: [0; MAX_VALIDATORS],
            sorted_raw_score_indices: [SORTED_INDEX_DEFAULT; MAX_VALIDATORS],
            delegations: [Delegation::default(); MAX_VALIDATORS],
            instant_unstake: BitMask::default(),
            progress: BitMask::default(),
            validators_for_immediate_removal: BitMask::default(),
            validators_to_remove: BitMask::default(),
            start_computing_scores_slot: 0,
            current_epoch: 20,
            next_cycle_epoch: 30,
            num_pool_validators: 3,
            scoring_unstake_total: 0,
            instant_unstake_total: 0,
            stake_deposit_unstake_total: 0,
            status_flags: 0,
            validators_added: 0,
            _padding0: [0; 2],
        }
    }

    /// When current_epoch reaches next_cycle_epoch, COMPUTE_SCORE and COMPUTE_DELEGATIONS
    /// flags should be cleared, allowing a new scoring cycle to begin.
    #[test]
    fn test_transition_idle_clears_flags_at_next_cycle_epoch() {
        let mut state = default_state();
        state.set_flag(COMPUTE_SCORE);
        state.set_flag(COMPUTE_DELEGATIONS);
        state.set_flag(REBALANCE_DIRECTED_COMPLETE);
        state.next_cycle_epoch = 30;

        // Before next_cycle_epoch: flags should remain
        state.transition_idle(29, 0.6, 0.9, 0.5).unwrap();
        assert!(state.has_flag(COMPUTE_SCORE));
        assert!(state.has_flag(COMPUTE_DELEGATIONS));

        // At next_cycle_epoch: flags should be cleared
        state.transition_idle(30, 0.6, 0.9, 0.5).unwrap();
        assert!(!state.has_flag(COMPUTE_SCORE));
        assert!(!state.has_flag(COMPUTE_DELEGATIONS));
    }

    /// Idle -> ComputeScores when:
    /// - REBALANCE_DIRECTED_COMPLETE is set
    /// - COMPUTE_DELEGATIONS is NOT set
    /// - epoch_progress >= compute_score_epoch_progress
    #[test]
    fn test_transition_idle_to_compute_scores() {
        let mut state = default_state();
        state.set_flag(REBALANCE_DIRECTED_COMPLETE);
        // No COMPUTE_DELEGATIONS flag

        // Below compute_score_epoch_progress (0.5): stays Idle
        state.transition_idle(20, 0.4, 0.9, 0.5).unwrap();
        assert!(matches!(state.state_tag, StewardStateEnum::Idle));

        // At compute_score_epoch_progress: transitions to ComputeScores
        state.transition_idle(20, 0.5, 0.9, 0.5).unwrap();
        assert!(matches!(state.state_tag, StewardStateEnum::ComputeScores));
        assert!(state.progress.is_empty());
        assert!(state.instant_unstake.is_empty());
    }

    /// Idle -> ComputeInstantUnstake when:
    /// - REBALANCE flag is NOT set (loop not completed)
    /// - epoch_progress >= instant_unstake_epoch_progress
    /// - Does NOT qualify for ComputeScores branch
    #[test]
    fn test_transition_idle_to_compute_instant_unstake() {
        let mut state = default_state();
        state.set_flag(REBALANCE_DIRECTED_COMPLETE);
        state.set_flag(COMPUTE_DELEGATIONS);
        // REBALANCE not set -> !completed_loop

        // Below instant_unstake_epoch_progress: stays Idle
        state.transition_idle(20, 0.8, 0.9, 0.5).unwrap();
        assert!(matches!(state.state_tag, StewardStateEnum::Idle));
        assert!(state.has_flag(PRE_LOOP_IDLE));

        // At instant_unstake_epoch_progress: transitions
        state.transition_idle(20, 0.9, 0.9, 0.5).unwrap();
        assert!(matches!(
            state.state_tag,
            StewardStateEnum::ComputeInstantUnstake
        ));
        assert!(state.progress.is_empty());
        assert!(state.instant_unstake.is_empty());
    }

    /// When the loop is completed (REBALANCE set), Idle should set POST_LOOP_IDLE
    /// and NOT transition to ComputeInstantUnstake.
    #[test]
    fn test_transition_idle_post_loop() {
        let mut state = default_state();
        state.set_flag(REBALANCE_DIRECTED_COMPLETE);
        state.set_flag(COMPUTE_DELEGATIONS);
        state.set_flag(REBALANCE);

        state.transition_idle(20, 0.95, 0.9, 0.5).unwrap();
        assert!(matches!(state.state_tag, StewardStateEnum::Idle));
        assert!(state.has_flag(POST_LOOP_IDLE));
    }

    /// Reproduces the reported bug scenario:
    /// - Previous cycle's ComputeScores started very late (>90% epoch mark)
    /// - COMPUTE_SCORE and COMPUTE_DELEGATIONS flags are still set from previous cycle
    /// - At the next_cycle_epoch, the epoch-based check clears the flags
    /// - This allows ComputeScores to start instead of falling through to ComputeInstantUnstake
    #[test]
    fn test_transition_idle_late_scoring_does_not_skip_compute_scores() {
        let mut state = default_state();
        // Simulate previous cycle completed: both flags set
        state.set_flag(COMPUTE_SCORE);
        state.set_flag(COMPUTE_DELEGATIONS);
        state.set_flag(REBALANCE_DIRECTED_COMPLETE);
        state.next_cycle_epoch = 30;

        // We are now at epoch 30 (next_cycle_epoch), past the 50% mark
        // The epoch-based check should clear COMPUTE_DELEGATIONS,
        // enabling the ComputeScores branch
        state.transition_idle(30, 0.6, 0.9, 0.5).unwrap();

        assert!(
            matches!(state.state_tag, StewardStateEnum::ComputeScores),
            "Expected ComputeScores but got {:?}. \
             Flags were not cleared at next_cycle_epoch, causing the scoring cycle to be skipped.",
            state.state_tag
        );
    }

    /// Verifies that if we are before next_cycle_epoch AND COMPUTE_DELEGATIONS is still set,
    /// the ComputeScores branch is NOT taken — instead we fall through to
    /// ComputeInstantUnstake at the 90% mark.
    #[test]
    fn test_transition_idle_before_next_cycle_epoch_skips_compute_scores() {
        let mut state = default_state();
        state.set_flag(COMPUTE_SCORE);
        state.set_flag(COMPUTE_DELEGATIONS);
        state.set_flag(REBALANCE_DIRECTED_COMPLETE);
        state.next_cycle_epoch = 30;

        // Epoch 29: flags NOT cleared, COMPUTE_DELEGATIONS blocks ComputeScores branch
        // At 95% epoch progress, should go to ComputeInstantUnstake
        state.transition_idle(29, 0.95, 0.9, 0.5).unwrap();
        assert!(matches!(
            state.state_tag,
            StewardStateEnum::ComputeInstantUnstake
        ));
    }

    /// Idle stays idle when nothing qualifies for a transition:
    /// - epoch_progress below both thresholds
    /// - no completed loop
    #[test]
    fn test_transition_idle_noop() {
        let mut state = default_state();
        state.set_flag(REBALANCE_DIRECTED_COMPLETE);
        state.set_flag(COMPUTE_DELEGATIONS);

        state.transition_idle(20, 0.3, 0.9, 0.5).unwrap();
        assert!(matches!(state.state_tag, StewardStateEnum::Idle));
        assert!(state.has_flag(PRE_LOOP_IDLE));
    }
}
