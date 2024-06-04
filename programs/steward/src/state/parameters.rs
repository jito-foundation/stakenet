use anchor_lang::{prelude::Result, zero_copy};
use borsh::{BorshDeserialize, BorshSerialize};
use validator_history::utils::cast_epoch;

use crate::{
    constants::{
        BASIS_POINTS_MAX, COMMISSION_MAX, COMPUTE_SCORE_SLOT_RANGE_MIN, EPOCH_PROGRESS_MAX,
        MAX_VALIDATORS, NUM_EPOCHS_BETWEEN_SCORING_MAX, VALIDATOR_HISTORY_FIRST_RELIABLE_EPOCH,
    },
    errors::StewardError,
};

#[derive(BorshSerialize, BorshDeserialize, Debug, Default, Clone)]
pub struct UpdateParametersArgs {
    // Scoring parameters
    pub mev_commission_range: Option<u16>,
    pub epoch_credits_range: Option<u16>,
    pub commission_range: Option<u16>,
    pub scoring_delinquency_threshold_ratio: Option<f64>,
    pub instant_unstake_delinquency_threshold_ratio: Option<f64>,
    pub mev_commission_bps_threshold: Option<u16>,
    pub commission_threshold: Option<u8>,
    pub historical_commission_threshold: Option<u8>,
    // Delegation parameters
    pub num_delegation_validators: Option<u32>,
    pub scoring_unstake_cap_bps: Option<u32>,
    pub instant_unstake_cap_bps: Option<u32>,
    pub stake_deposit_unstake_cap_bps: Option<u32>,
    // State machine parameters
    pub instant_unstake_epoch_progress: Option<f64>,
    pub compute_score_slot_range: Option<usize>,
    pub instant_unstake_inputs_epoch_progress: Option<f64>,
    pub num_epochs_between_scoring: Option<u64>,
    pub minimum_stake_lamports: Option<u64>,
    pub minimum_voting_epochs: Option<u64>,
}

#[derive(BorshSerialize, Default)]
#[zero_copy]
pub struct Parameters {
    /////// Scoring parameters ///////
    /// Number of epochs to consider for MEV commission
    pub mev_commission_range: u16,

    /// Number of epochs to consider for epoch credits
    pub epoch_credits_range: u16,

    /// Number of epochs to consider for commission
    pub commission_range: u16,

    /// Highest MEV commission rate allowed in bps
    pub mev_commission_bps_threshold: u16,

    /// Proportion of delinquent slots to total slots to trigger delinquency measurement in scoring
    pub scoring_delinquency_threshold_ratio: f64,

    /// Proportion of delinquent slots to total slots to trigger instant unstake
    pub instant_unstake_delinquency_threshold_ratio: f64,

    /// Highest commission rate allowed in commission_range epochs, in percent
    pub commission_threshold: u8,

    /// Highest commission rate allowed in tracked history
    pub historical_commission_threshold: u8,

    /// Required so that the struct is 8-byte aligned
    /// https://doc.rust-lang.org/reference/type-layout.html#reprc-structs
    pub padding0: [u8; 6],

    /////// Delegation parameters ///////
    /// Number of validators to delegate to
    pub num_delegation_validators: u32,

    /// Maximum amount of the pool to be unstaked in a cycle for scoring (in basis points)
    pub scoring_unstake_cap_bps: u32,

    // Maximum amount of the pool to be unstaked in a cycle for instant unstake (in basis points)
    pub instant_unstake_cap_bps: u32,

    /// Maximum amount of the pool to be unstaked in a cycle from stake deposits (in basis points)
    pub stake_deposit_unstake_cap_bps: u32,

    /////// State machine operation parameters ///////
    /// Number of slots that scoring must be completed in
    pub compute_score_slot_range: usize,

    /// Progress in epoch before instant unstake is allowed
    pub instant_unstake_epoch_progress: f64,

    /// Validator history copy_vote_account and Cluster History must be updated past this epoch progress before calculating instant unstake
    pub instant_unstake_inputs_epoch_progress: f64,

    /// Number of epochs a given validator set will be delegated to before recomputing scores
    pub num_epochs_between_scoring: u64,

    /// Minimum stake required to be added to pool ValidatorList and eligible for delegation
    pub minimum_stake_lamports: u64,

    /// Minimum epochs voting required to be in the pool ValidatorList and eligible for delegation
    pub minimum_voting_epochs: u64,
}

impl Parameters {
    /// Merges the updated parameters with the current parameters and validates them
    pub fn get_valid_updated_parameters(
        self,
        args: &UpdateParametersArgs,
        current_epoch: u64,
        slots_per_epoch: u64,
    ) -> Result<Parameters> {
        // Updates parameters and validates them
        let UpdateParametersArgs {
            mev_commission_range,
            epoch_credits_range,
            commission_range,
            scoring_delinquency_threshold_ratio,
            instant_unstake_delinquency_threshold_ratio,
            mev_commission_bps_threshold,
            commission_threshold,
            historical_commission_threshold,
            num_delegation_validators,
            scoring_unstake_cap_bps,
            instant_unstake_cap_bps,
            stake_deposit_unstake_cap_bps,
            instant_unstake_epoch_progress,
            instant_unstake_inputs_epoch_progress,
            compute_score_slot_range,
            num_epochs_between_scoring,
            minimum_stake_lamports,
            minimum_voting_epochs,
        } = *args;

        let mut new_parameters = self;

        if let Some(mev_commission_range) = mev_commission_range {
            new_parameters.mev_commission_range = mev_commission_range;
        }

        if let Some(epoch_credits_range) = epoch_credits_range {
            new_parameters.epoch_credits_range = epoch_credits_range;
        }

        if let Some(commission_range) = commission_range {
            new_parameters.commission_range = commission_range;
        }

        if let Some(scoring_delinquency_threshold_ratio) = scoring_delinquency_threshold_ratio {
            new_parameters.scoring_delinquency_threshold_ratio =
                scoring_delinquency_threshold_ratio;
        }

        if let Some(instant_unstake_delinquency_threshold_ratio) =
            instant_unstake_delinquency_threshold_ratio
        {
            new_parameters.instant_unstake_delinquency_threshold_ratio =
                instant_unstake_delinquency_threshold_ratio;
        }

        if let Some(mev_commission_bps_threshold) = mev_commission_bps_threshold {
            new_parameters.mev_commission_bps_threshold = mev_commission_bps_threshold;
        }

        if let Some(commission_threshold) = commission_threshold {
            new_parameters.commission_threshold = commission_threshold;
        }

        if let Some(historical_commission_threshold) = historical_commission_threshold {
            new_parameters.historical_commission_threshold = historical_commission_threshold;
        }

        if let Some(num_delegation_validators) = num_delegation_validators {
            new_parameters.num_delegation_validators = num_delegation_validators;
        }

        if let Some(scoring_unstake_cap_bps) = scoring_unstake_cap_bps {
            new_parameters.scoring_unstake_cap_bps = scoring_unstake_cap_bps;
        }

        if let Some(instant_unstake_cap_bps) = instant_unstake_cap_bps {
            new_parameters.instant_unstake_cap_bps = instant_unstake_cap_bps;
        }

        if let Some(stake_deposit_unstake_cap_bps) = stake_deposit_unstake_cap_bps {
            new_parameters.stake_deposit_unstake_cap_bps = stake_deposit_unstake_cap_bps;
        }

        if let Some(instant_unstake_epoch_progress) = instant_unstake_epoch_progress {
            new_parameters.instant_unstake_epoch_progress = instant_unstake_epoch_progress;
        }

        if let Some(vote_account_update_epoch_progress) = instant_unstake_inputs_epoch_progress {
            new_parameters.instant_unstake_inputs_epoch_progress =
                vote_account_update_epoch_progress;
        }

        if let Some(compute_score_slot_range) = compute_score_slot_range {
            new_parameters.compute_score_slot_range = compute_score_slot_range;
        }

        if let Some(num_epochs_between_scoring) = num_epochs_between_scoring {
            new_parameters.num_epochs_between_scoring = num_epochs_between_scoring;
        }

        if let Some(minimum_stake_lamports) = minimum_stake_lamports {
            new_parameters.minimum_stake_lamports = minimum_stake_lamports;
        }

        if let Some(minimum_voting_epochs) = minimum_voting_epochs {
            new_parameters.minimum_voting_epochs = minimum_voting_epochs;
        }

        // Validation will throw an error if any of the parameters are invalid
        new_parameters.validate(current_epoch, slots_per_epoch)?;

        Ok(new_parameters)
    }

    /// Validate reasonable bounds on parameters
    pub fn validate(&self, current_epoch: u64, slots_per_epoch: u64) -> Result<()> {
        // Cannot evaluate epochs before VALIDATOR_HISTORY_FIRST_RELIABLE_EPOCH or beyond the CircBuf length
        let window_max = (current_epoch as usize)
            .checked_sub(VALIDATOR_HISTORY_FIRST_RELIABLE_EPOCH)
            .ok_or(StewardError::ArithmeticError)?
            .min(validator_history::ValidatorHistory::MAX_ITEMS - 1);
        let window_max = cast_epoch(window_max as u64)?;

        if self.mev_commission_range > window_max {
            return Err(StewardError::InvalidParameterValue.into());
        }

        if self.epoch_credits_range > window_max {
            return Err(StewardError::InvalidParameterValue.into());
        }

        if self.commission_range > window_max {
            return Err(StewardError::InvalidParameterValue.into());
        }

        // Proportion between 0 and 1
        if !(0. ..=1.).contains(&self.scoring_delinquency_threshold_ratio) {
            return Err(StewardError::InvalidParameterValue.into());
        }

        // Proportion between 0 and 1
        if !(0. ..=1.).contains(&self.instant_unstake_delinquency_threshold_ratio) {
            return Err(StewardError::InvalidParameterValue.into());
        }

        if self.mev_commission_bps_threshold > BASIS_POINTS_MAX {
            return Err(StewardError::InvalidParameterValue.into());
        }

        if self.commission_threshold > COMMISSION_MAX {
            return Err(StewardError::InvalidParameterValue.into());
        }

        if self.historical_commission_threshold > COMMISSION_MAX {
            return Err(StewardError::InvalidParameterValue.into());
        }

        if self.num_delegation_validators == 0
            || self.num_delegation_validators > MAX_VALIDATORS as u32
        {
            return Err(StewardError::InvalidParameterValue.into());
        }

        if self.scoring_unstake_cap_bps > BASIS_POINTS_MAX as u32 {
            return Err(StewardError::InvalidParameterValue.into());
        }

        if self.instant_unstake_cap_bps > BASIS_POINTS_MAX as u32 {
            return Err(StewardError::InvalidParameterValue.into());
        }

        if self.stake_deposit_unstake_cap_bps > BASIS_POINTS_MAX as u32 {
            return Err(StewardError::InvalidParameterValue.into());
        }

        if !(0. ..=EPOCH_PROGRESS_MAX).contains(&self.instant_unstake_epoch_progress) {
            return Err(StewardError::InvalidParameterValue.into());
        }

        if !(0. ..=EPOCH_PROGRESS_MAX).contains(&self.instant_unstake_inputs_epoch_progress) {
            return Err(StewardError::InvalidParameterValue.into());
        }

        if self.minimum_voting_epochs > window_max as u64 {
            return Err(StewardError::InvalidParameterValue.into());
        }

        if !(COMPUTE_SCORE_SLOT_RANGE_MIN..=slots_per_epoch as usize)
            .contains(&self.compute_score_slot_range)
        {
            return Err(StewardError::InvalidParameterValue.into());
        }

        if self.num_epochs_between_scoring == 0
            || self.num_epochs_between_scoring > NUM_EPOCHS_BETWEEN_SCORING_MAX
        {
            return Err(StewardError::InvalidParameterValue.into());
        }

        Ok(())
    }
}
