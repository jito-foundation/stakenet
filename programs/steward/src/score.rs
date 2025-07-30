#[cfg(feature = "idl-build")]
use anchor_lang::IdlBuild;
use anchor_lang::{
    prelude::event, solana_program::pubkey::Pubkey, AnchorDeserialize, AnchorSerialize,
    Discriminator, Result,
};
use validator_history::{
    constants::TVC_MULTIPLIER, ClusterHistory, MerkleRootUploadAuthority, ValidatorHistory,
};

use crate::{
    constants::{
        BASIS_POINTS_MAX, COMMISSION_MAX, EPOCH_DEFAULT, VALIDATOR_HISTORY_FIRST_RELIABLE_EPOCH,
    },
    errors::StewardError::{self, ArithmeticError},
    Config,
};

#[event]
#[derive(Debug, PartialEq)]
pub struct ScoreComponentsV3 {
    /// Product of all scoring components
    pub score: f64,

    /// vote_credits_ratio * (1 - commission)
    pub yield_score: f64,

    /// If max mev commission in mev_commission_range epochs is less than threshold, score is 1.0, else 0
    pub mev_commission_score: f64,

    /// If validator is blacklisted, score is 0.0, else 1.0
    pub blacklisted_score: f64,

    /// If validator is not in the superminority, score is 1.0, else 0.0
    pub superminority_score: f64,

    /// If delinquency is not > threshold in any epoch, score is 1.0, else 0.0
    pub delinquency_score: f64,

    /// If validator has a mev commission in the last 10 epochs, score is 1.0, else 0.0
    pub running_jito_score: f64,

    /// If max commission in commission_range epochs is less than commission_threshold, score is 1.0, else 0.0
    pub commission_score: f64,

    /// If max commission in all validator history epochs is less than historical_commission_threshold, score is 1.0, else 0.0
    pub historical_commission_score: f64,

    /// If validator is using TipRouter authority, OR OldJito authority then score is 1.0, else 0.0
    pub merkle_root_upload_authority_score: f64,

    /// Average vote credits in last epoch_credits_range epochs / average blocks in last epoch_credits_range epochs
    /// Excluding current epoch
    pub vote_credits_ratio: f64,

    pub vote_account: Pubkey,

    pub epoch: u16,

    /// Details about why a given score was calculated
    pub details: ScoreDetails,

    /// If validator has realized priority fee commissions > config limits over a lookback range,
    /// score 0.
    pub priority_fee_commission_score: f64,

    /// If validator is using TipRouter authority, OR OldJito authority then score is 1.0, else 0.0
    pub priority_fee_merkle_root_upload_authority_score: f64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, PartialEq)]
pub struct ScoreDetails {
    /// Max MEV commission observed
    pub max_mev_commission: u16,

    /// Epoch of max MEV commission
    pub max_mev_commission_epoch: u16,

    /// Epoch when superminority was detected
    pub superminority_epoch: u16,

    /// Ratio that failed delinquency check
    pub delinquency_ratio: f64,

    /// Epoch when delinquency was detected
    pub delinquency_epoch: u16,

    /// Max commission observed
    pub max_commission: u8,

    /// Epoch of max commission
    pub max_commission_epoch: u16,

    /// Max historical commission observed
    pub max_historical_commission: u8,

    /// Epoch of max historical commission
    pub max_historical_commission_epoch: u16,

    /// Max realized priority fee commission observed
    pub max_priority_fee_commission: u16,

    /// Epoch of realized priority fee commission
    pub max_priority_fee_commission_epoch: u16,
}

pub fn validator_score(
    validator: &ValidatorHistory,
    cluster: &ClusterHistory,
    config: &Config,
    current_epoch: u16,
    tvc_activation_epoch: u64,
) -> Result<ScoreComponentsV3> {
    let params = &config.parameters;

    /////// Shared windows ///////
    let mev_commission_window = validator.history.mev_commission_range(
        current_epoch
            .checked_sub(params.mev_commission_range)
            .ok_or(ArithmeticError)?,
        current_epoch,
    );

    let epoch_credits_start = current_epoch
        .checked_sub(params.epoch_credits_range)
        .ok_or(ArithmeticError)?;
    // Epoch credits should not include current epoch because it is in progress and data would be incomplete
    let epoch_credits_end = current_epoch.checked_sub(1).ok_or(ArithmeticError)?;

    let normalized_epoch_credits_window = validator.history.epoch_credits_range_normalized(
        epoch_credits_start,
        epoch_credits_end,
        tvc_activation_epoch,
    );

    let total_blocks_window = cluster
        .history
        .total_blocks_range(epoch_credits_start, epoch_credits_end);

    let commission_window = validator.history.commission_range(
        current_epoch
            .checked_sub(params.commission_range)
            .ok_or(ArithmeticError)?,
        current_epoch,
    );

    /////// Component calculations ///////
    let (mev_commission_score, max_mev_commission, max_mev_commission_epoch, running_jito_score) =
        calculate_mev_commission(
            &mev_commission_window,
            current_epoch,
            params.mev_commission_bps_threshold,
        )?;

    let (vote_credits_ratio, delinquency_score, delinquency_ratio, delinquency_epoch) =
        calculate_epoch_credits(
            &normalized_epoch_credits_window,
            &total_blocks_window,
            epoch_credits_start,
            params.scoring_delinquency_threshold_ratio,
        )?;

    let (commission_score, max_commission, max_commission_epoch) = calculate_commission(
        &commission_window,
        current_epoch,
        params.commission_threshold,
    )?;

    let (historical_commission_score, max_historical_commission, max_historical_commission_epoch) =
        calculate_historical_commission(
            validator,
            current_epoch,
            params.historical_commission_threshold,
        )?;

    let (superminority_score, superminority_epoch) =
        calculate_superminority(validator, current_epoch, params.commission_range)?;

    let blacklisted_score = calculate_blacklist_score(config, validator.index)?;

    let merkle_root_upload_authority_score = calculate_merkle_root_authority_score(validator)?;
    let priority_fee_merkle_root_upload_authority_score =
        calculate_priority_fee_merkle_root_authority(validator)?;

    let (
        priority_fee_commission_score,
        max_priority_fee_commission,
        max_priority_fee_commission_epoch,
    ) = calculate_priority_fee_commission(config, validator, current_epoch)?;

    /////// Formula ///////

    let yield_score = vote_credits_ratio * (1. - max_commission as f64 / COMMISSION_MAX as f64);

    let score = mev_commission_score
        * commission_score
        * historical_commission_score
        * blacklisted_score
        * superminority_score
        * delinquency_score
        * running_jito_score
        * yield_score
        * merkle_root_upload_authority_score
        * priority_fee_commission_score;

    Ok(ScoreComponentsV3 {
        score,
        yield_score,
        mev_commission_score,
        blacklisted_score,
        superminority_score,
        delinquency_score,
        running_jito_score,
        commission_score,
        historical_commission_score,
        merkle_root_upload_authority_score,
        vote_credits_ratio,
        vote_account: validator.vote_account,
        epoch: current_epoch,
        details: ScoreDetails {
            max_mev_commission,
            max_mev_commission_epoch,
            superminority_epoch,
            delinquency_ratio,
            delinquency_epoch,
            max_commission,
            max_commission_epoch,
            max_historical_commission,
            max_historical_commission_epoch,
            max_priority_fee_commission,
            max_priority_fee_commission_epoch,
        },
        priority_fee_commission_score,
        priority_fee_merkle_root_upload_authority_score,
    })
}

/// Finds max MEV commission in the last `mev_commission_range` epochs and determines if it is above a threshold.
/// Also determines if validator has had a MEV commission in the last 10 epochs to ensure they are running jito-solana
pub fn calculate_mev_commission(
    mev_commission_window: &[Option<u16>],
    current_epoch: u16,
    mev_commission_bps_threshold: u16,
) -> Result<(f64, u16, u16, f64)> {
    let (max_mev_commission, max_mev_commission_epoch) = mev_commission_window
        .iter()
        .rev()
        .enumerate()
        .filter_map(|(i, &commission)| commission.map(|c| (c, current_epoch.checked_sub(i as u16))))
        .max_by_key(|&(commission, _)| commission)
        .unwrap_or((BASIS_POINTS_MAX, Some(current_epoch)));

    let max_mev_commission_epoch = max_mev_commission_epoch.ok_or(StewardError::ArithmeticError)?;

    let mev_commission_score = if max_mev_commission <= mev_commission_bps_threshold {
        1.0
    } else {
        0.0
    };

    /////// Running Jito ///////
    let running_jito_score = if mev_commission_window.iter().any(|i| i.is_some()) {
        1.0
    } else {
        0.0
    };

    Ok((
        mev_commission_score,
        max_mev_commission,
        max_mev_commission_epoch,
        running_jito_score,
    ))
}

/// Calculates the vote credits ratio and delinquency score for the validator
pub fn calculate_epoch_credits(
    epoch_credits_window: &[Option<u32>],
    total_blocks_window: &[Option<u32>],
    epoch_credits_start: u16,
    scoring_delinquency_threshold_ratio: f64,
) -> Result<(f64, f64, f64, u16)> {
    if epoch_credits_window.is_empty() || total_blocks_window.is_empty() {
        return Err(StewardError::ArithmeticError.into());
    }

    let average_vote_credits = epoch_credits_window.iter().filter_map(|&i| i).sum::<u32>() as f64
        / epoch_credits_window.len() as f64;

    let nonzero_blocks = total_blocks_window.iter().filter(|i| i.is_some()).count();
    if nonzero_blocks == 0 {
        return Err(StewardError::ArithmeticError.into());
    }

    // Get average of total blocks in window, ignoring values where upload was missed
    let average_blocks =
        total_blocks_window.iter().filter_map(|&i| i).sum::<u32>() as f64 / nonzero_blocks as f64;

    // Delinquency heuristic - not actual delinquency
    let mut delinquency_score = 1.0;
    let mut delinquency_ratio = 1.0;
    let mut delinquency_epoch = EPOCH_DEFAULT;

    for (i, (maybe_credits, maybe_blocks)) in epoch_credits_window
        .iter()
        .zip(total_blocks_window.iter())
        .enumerate()
    {
        if let Some(blocks) = maybe_blocks {
            // If vote credits are None, then validator was not active because we retroactively fill credits for last 64 epochs.
            // If total blocks are None, then keepers missed an upload and validator should not be punished.
            let credits = maybe_credits.unwrap_or(0);
            let ratio = credits as f64 / (blocks * TVC_MULTIPLIER) as f64;
            if ratio < scoring_delinquency_threshold_ratio {
                delinquency_score = 0.0;
                delinquency_ratio = ratio;
                delinquency_epoch = epoch_credits_start
                    .checked_add(i as u16)
                    .ok_or(StewardError::ArithmeticError)?;
                break;
            }
        }
    }

    let normalized_vote_credits_ratio =
        average_vote_credits / (average_blocks * (TVC_MULTIPLIER as f64));

    Ok((
        normalized_vote_credits_ratio,
        delinquency_score,
        delinquency_ratio,
        delinquency_epoch,
    ))
}

/// Finds max commission in the last `commission_range` epochs
pub fn calculate_commission(
    commission_window: &[Option<u8>],
    current_epoch: u16,
    commission_threshold: u8,
) -> Result<(f64, u8, u16)> {
    /////// Commission ///////
    let (max_commission, max_commission_epoch) = commission_window
        .iter()
        .rev()
        .enumerate()
        .filter_map(|(i, &commission)| commission.map(|c| (c, current_epoch.checked_sub(i as u16))))
        .max_by_key(|&(commission, _)| commission)
        .unwrap_or((0, Some(current_epoch)));

    let max_commission_epoch = max_commission_epoch.ok_or(StewardError::ArithmeticError)?;

    let commission_score = if max_commission <= commission_threshold {
        1.0
    } else {
        0.0
    };

    Ok((commission_score, max_commission, max_commission_epoch))
}

/// Checks if validator has commission above a threshold in any epoch in their history
pub fn calculate_historical_commission(
    validator: &ValidatorHistory,
    current_epoch: u16,
    historical_commission_threshold: u8,
) -> Result<(f64, u8, u16)> {
    if validator.history.is_empty() {
        return Err(StewardError::ArithmeticError.into());
    }

    let (max_historical_commission, max_historical_commission_epoch) = validator
        .history
        .commission_range(VALIDATOR_HISTORY_FIRST_RELIABLE_EPOCH as u16, current_epoch)
        .iter()
        .rev()
        .enumerate()
        .filter_map(|(i, &commission)| commission.map(|c| (c, current_epoch.checked_sub(i as u16))))
        .max_by_key(|&(commission, _)| commission)
        .unwrap_or((0, Some(VALIDATOR_HISTORY_FIRST_RELIABLE_EPOCH as u16)));

    let max_historical_commission_epoch =
        max_historical_commission_epoch.ok_or(StewardError::ArithmeticError)?;

    let historical_commission_score =
        if max_historical_commission <= historical_commission_threshold {
            1.0
        } else {
            0.0
        };

    Ok((
        historical_commission_score,
        max_historical_commission,
        max_historical_commission_epoch,
    ))
}

/// Checks if validator is in the top 1/3 of validators by stake for the current epoch
pub fn calculate_superminority(
    validator: &ValidatorHistory,
    current_epoch: u16,
    commission_range: u16,
) -> Result<(f64, u16)> {
    /*
        If epoch credits exist, we expect the validator to have a superminority flag set. If not, scoring fails and we wait for
        the stake oracle to call UpdateStakeHistory.
        If epoch credits is not set, we iterate through last `commission_range` epochs to find the latest superminority flag.
        If no entry is found, we assume the validator is not a superminority validator.
    */
    if validator.history.epoch_credits_latest().is_some() {
        if let Some(superminority) = validator.history.superminority_latest() {
            if superminority == 1 {
                Ok((0.0, current_epoch))
            } else {
                Ok((1.0, EPOCH_DEFAULT))
            }
        } else {
            Err(StewardError::StakeHistoryNotRecentEnough.into())
        }
    } else {
        let superminority_window = validator.history.superminority_range(
            current_epoch
                .checked_sub(commission_range)
                .ok_or(ArithmeticError)?,
            current_epoch,
        );

        let (status, epoch) = superminority_window
            .iter()
            .rev()
            .enumerate()
            .filter_map(|(i, &superminority)| {
                superminority.map(|s| (s, current_epoch.checked_sub(i as u16)))
            })
            .next()
            .unwrap_or((0, Some(current_epoch)));

        let epoch = epoch.ok_or(StewardError::ArithmeticError)?;

        if status == 1 {
            Ok((0.0, epoch))
        } else {
            Ok((1.0, EPOCH_DEFAULT))
        }
    }
}

/// Checks if validator is blacklisted using the validator history index in the config's blacklist
pub fn calculate_blacklist_score(config: &Config, validator_index: u32) -> Result<f64> {
    if config
        .validator_history_blacklist
        .get(validator_index as usize)?
    {
        Ok(0.0)
    } else {
        Ok(1.0)
    }
}

/// Checks if validator is using appropriate TDA MerkleRootUploadAuthority
pub fn calculate_merkle_root_authority_score(validator: &ValidatorHistory) -> Result<f64> {
    // calculate_instant_unstake_merkle_root_upload_auth returns whether or not
    // instant unstake should be triggered, so we invert the result to get the score
    if calculate_instant_unstake_merkle_root_upload_auth(
        &validator.history.merkle_root_upload_authority_latest(),
    )? {
        Ok(0.0)
    } else {
        Ok(1.0)
    }
}

/// Checks if validator is using appropriate TDA MerkleRootUploadAuthority
pub fn calculate_priority_fee_merkle_root_authority(validator: &ValidatorHistory) -> Result<f64> {
    if calculate_instant_unstake_merkle_root_upload_auth(
        &validator
            .history
            .priority_fee_merkle_root_upload_authority_latest(),
    )? {
        Ok(0.0)
    } else {
        Ok(1.0)
    }
}

/// Given a validator's tips and total fees, determine their realized commission rate
pub fn calculate_realized_commission_bps(tips: &Option<u64>, total_fees: &Option<u64>) -> u16 {
    // total_fees is None when the ValidatorHistoryEntry has been created, but the
    //  priority_fee_oracle_authority has not called UpdatePriorityFeeHistory
    if total_fees.is_none() || total_fees.iter().all(|&f| f == 0) {
        return 0;
    }
    // Default the tips to 0 because we assume the PFDA was not created and the validator is not
    // distributing priority fees. This forces inverse_commission to 0 and commission to
    // BASIS_POINTS_MAX
    let tips = tips.unwrap_or(0);
    // Default the total_fees to u64::MAX to force inverse_commission towards 0 and commission
    // to BASIS_POINTS_MAX
    let total_fees = total_fees.unwrap_or(u64::MAX);

    let validators_rake = total_fees.saturating_sub(tips);
    // We scale by BASIS_POINTS_MAX before division, so the output is in bps
    let numerator = validators_rake.saturating_mul(BASIS_POINTS_MAX as u64);
    let commission = numerator.checked_div(total_fees).unwrap_or(0 as u64);
    u16::try_from(commission).unwrap_or(BASIS_POINTS_MAX)
}

/// Checks if validator is maintaining < X% realized commission rates over some history of epochs
pub fn calculate_priority_fee_commission(
    config: &Config,
    validator: &ValidatorHistory,
    current_epoch: u16,
) -> Result<(f64, u16, u16)> {
    if current_epoch < config.parameters.priority_fee_scoring_start_epoch {
        return Ok((1.0, 0, EPOCH_DEFAULT));
    }
    let (start_epoch, end_epoch) = config.priority_fee_epoch_range(current_epoch);
    let priority_fee_tips = validator
        .history
        .priority_fee_tips_range(start_epoch, end_epoch);
    let total_priority_fees = validator
        .history
        .total_priority_fees_range(start_epoch, end_epoch);
    let priority_fee_merkle_root_upload_authority = validator
        .history
        .priority_fee_merkle_root_upload_authority_range(start_epoch, end_epoch);

    // determine the highest priority fee commission
    let mut max_priority_fee_commission: u16 = 0;
    let mut max_priority_fee_commission_epoch: u16 = EPOCH_DEFAULT;
    let realized_commissions: Vec<u16> = priority_fee_tips
        .iter()
        .zip(&total_priority_fees)
        .zip(&priority_fee_merkle_root_upload_authority)
        .enumerate()
        .flat_map(
            |(relative_epoch, ((tips, total_fees), priority_fee_merkle_root_upload_authority))| {
                let commission_bps: u16 = calculate_realized_commission_bps(tips, total_fees);
                // this should not happen, but we should not score the epoch if the upload authority is None
                if priority_fee_merkle_root_upload_authority.is_none() {
                    return vec![];
                }
                if let Some(upload_authority) = priority_fee_merkle_root_upload_authority {
                    // do not include this epoch in scoring if the upload authority is Unset
                    if matches!(upload_authority, MerkleRootUploadAuthority::Unset) {
                        return vec![];
                    }
                    // commission is 100% if the PFDA does not exist, validator keeps all fees
                    if matches!(upload_authority, MerkleRootUploadAuthority::DNE) {
                        return vec![BASIS_POINTS_MAX];
                    }
                }
                if max_priority_fee_commission < commission_bps {
                    let max_commission_epoch: u16 =
                        start_epoch.saturating_add(relative_epoch as u16);
                    max_priority_fee_commission = commission_bps;
                    max_priority_fee_commission_epoch = max_commission_epoch;
                }
                vec![commission_bps]
            },
        )
        .collect::<Vec<u16>>();

    // return score 1 when there's not enough history. We assume both fields being None means the
    // priority fee data is non-existent for this epoch.
    if priority_fee_tips[0].is_none() && total_priority_fees[0].is_none() {
        return Ok((1.0, 0u16, max_priority_fee_commission_epoch));
    }

    // if there are no realized commissions due to Unset PFDA, return score 1, default
    // to not penalize the validator for not having a PFDA copied into their history
    if realized_commissions.is_empty() {
        return Ok((1.0, 0, end_epoch));
    }

    let num_epochs: u64 = realized_commissions.len() as u64;
    let total_commission: u64 = realized_commissions
        .into_iter()
        .fold(0, |agg, val| agg.checked_add(u64::from(val)).unwrap());
    // We calculate the avg commission bps, rounding up to the nearest bp
    let avg_commission: u64 = total_commission
        // this addition of (denominator - 1) is used to round up if there is any remainder
        .checked_add(num_epochs.checked_sub(1).ok_or(ArithmeticError)?)
        .ok_or(ArithmeticError)?
        .checked_div(num_epochs)
        .ok_or(ArithmeticError)?;
    let avg_commission: u16 = u16::try_from(avg_commission).map_err(|_| ArithmeticError)?;

    let max_commission = config.max_avg_commission();

    if avg_commission <= max_commission {
        Ok((
            1.0,
            max_priority_fee_commission,
            max_priority_fee_commission_epoch,
        ))
    } else {
        Ok((
            0.0,
            max_priority_fee_commission,
            max_priority_fee_commission_epoch,
        ))
    }
}

#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct InstantUnstakeComponentsV3 {
    /// Aggregate of all checks
    pub instant_unstake: bool,

    /// Checks if validator has missed > instant_unstake_delinquency_threshold_ratio of votes this epoch
    pub delinquency_check: bool,

    /// Checks if validator has increased commission > commission_threshold
    pub commission_check: bool,

    /// Checks if validator has increased MEV commission > mev_commission_bps_threshold
    pub mev_commission_check: bool,

    /// Checks if validator was added to blacklist
    pub is_blacklisted: bool,

    /// Checks if validator has an unacceptable merkle root upload authority
    pub is_bad_merkle_root_upload_authority: bool,

    /// Checks if validator has an unacceptable priority fee merkle root upload authority
    pub is_bad_priority_fee_merkle_root_upload_authority: bool,

    pub vote_account: Pubkey,

    pub epoch: u16,

    /// Details about why a given check was calculated
    pub details: InstantUnstakeDetails,
}

#[derive(AnchorSerialize, AnchorDeserialize, Debug, PartialEq, Eq)]
pub struct InstantUnstakeDetails {
    /// Latest epoch credits
    pub epoch_credits_latest: u64,

    /// Latest vote account update slot
    pub vote_account_last_update_slot: u64,

    /// Latest total blocks
    pub total_blocks_latest: u32,

    /// Cluster history slot index
    pub cluster_history_slot_index: u64,

    /// Commission value
    pub commission: u8,

    /// MEV commission value
    pub mev_commission: u16,
}

/// Method to calculate if a validator should be unstaked instantly this epoch.
/// Before running, checks are needed on cluster and validator history to be updated this epoch past the halfway point of the epoch.
pub fn instant_unstake_validator(
    validator: &ValidatorHistory,
    cluster: &ClusterHistory,
    config: &Config,
    epoch_start_slot: u64,
    current_epoch: u16,
    tvc_activation_epoch: u64,
) -> Result<InstantUnstakeComponentsV3> {
    let params = &config.parameters;

    /////// Shared calculations ///////
    let cluster_history_slot_index = cluster
        .cluster_history_last_update_slot
        .checked_sub(epoch_start_slot)
        .ok_or(StewardError::ArithmeticError)?;

    let total_blocks_latest = cluster
        .history
        .total_blocks_latest()
        .ok_or(StewardError::ClusterHistoryNotRecentEnough)?;

    let vote_account_last_update_slot = validator
        .history
        .vote_account_last_update_slot_latest()
        .ok_or(StewardError::VoteHistoryNotRecentEnough)?;

    let validator_history_slot_index = vote_account_last_update_slot
        .checked_sub(epoch_start_slot)
        .ok_or(StewardError::ArithmeticError)?;

    let epoch_credits_latest = validator
        .history
        .epoch_credits_latest_normalized(current_epoch as u64, tvc_activation_epoch)
        .unwrap_or(0);

    /////// Component calculations ///////
    let delinquency_check = calculate_instant_unstake_delinquency(
        total_blocks_latest,
        cluster_history_slot_index,
        epoch_credits_latest,
        validator_history_slot_index,
        params.instant_unstake_delinquency_threshold_ratio,
    )?;

    let (mev_commission_check, mev_commission_bps) = calculate_instant_unstake_mev_commission(
        validator,
        current_epoch,
        params.mev_commission_bps_threshold,
    );

    let (commission_check, commission) =
        calculate_instant_unstake_commission(validator, params.commission_threshold);

    let is_blacklisted = calculate_instant_unstake_blacklist(config, validator.index)?;

    let is_bad_merkle_root_upload_authority = calculate_instant_unstake_merkle_root_upload_auth(
        &validator.history.merkle_root_upload_authority_latest(),
    )?;

    let is_bad_priority_fee_merkle_root_upload_authority =
        calculate_instant_unstake_merkle_root_upload_auth(
            &validator
                .history
                .priority_fee_merkle_root_upload_authority_latest(),
        )?;

    let instant_unstake = delinquency_check
        || commission_check
        || mev_commission_check
        || is_blacklisted
        || is_bad_merkle_root_upload_authority
        || is_bad_priority_fee_merkle_root_upload_authority;

    Ok(InstantUnstakeComponentsV3 {
        instant_unstake,
        delinquency_check,
        commission_check,
        mev_commission_check,
        is_blacklisted,
        is_bad_merkle_root_upload_authority,
        is_bad_priority_fee_merkle_root_upload_authority,
        vote_account: validator.vote_account,
        epoch: current_epoch,
        details: InstantUnstakeDetails {
            epoch_credits_latest: epoch_credits_latest as u64,
            vote_account_last_update_slot,
            total_blocks_latest,
            cluster_history_slot_index,
            commission,
            mev_commission: mev_commission_bps,
        },
    })
}

/// Calculates if the validator should be unstaked due to delinquency
pub fn calculate_instant_unstake_delinquency(
    total_blocks_latest: u32,
    cluster_history_slot_index: u64,
    epoch_credits_latest: u32,
    validator_history_slot_index: u64,
    instant_unstake_delinquency_threshold_ratio: f64,
) -> Result<bool> {
    if cluster_history_slot_index == 0 || validator_history_slot_index == 0 {
        return Err(StewardError::ArithmeticError.into());
    }

    let blocks_produced_rate = total_blocks_latest as f64 / cluster_history_slot_index as f64;
    let vote_credits_rate = epoch_credits_latest as f64 / validator_history_slot_index as f64;

    if blocks_produced_rate > 0. {
        Ok(
            (vote_credits_rate / (blocks_produced_rate * (TVC_MULTIPLIER as f64)))
                < instant_unstake_delinquency_threshold_ratio,
        )
    } else {
        Ok(false)
    }
}

/// Calculates if the validator should be unstaked due to MEV commission
pub fn calculate_instant_unstake_mev_commission(
    validator: &ValidatorHistory,
    current_epoch: u16,
    mev_commission_bps_threshold: u16,
) -> (bool, u16) {
    let previous_epoch = current_epoch.saturating_sub(1);
    let mev_commission_previous_current = validator
        .history
        .mev_commission_range(previous_epoch, current_epoch);
    let mev_commission_bps = mev_commission_previous_current
        .iter()
        .filter_map(|&i| i)
        .max()
        .unwrap_or(0);
    let mev_commission_check = mev_commission_bps > mev_commission_bps_threshold;
    (mev_commission_check, mev_commission_bps)
}

/// Calculates if the validator should be unstaked due to commission
pub fn calculate_instant_unstake_commission(
    validator: &ValidatorHistory,
    commission_threshold: u8,
) -> (bool, u8) {
    let commission = validator
        .history
        .commission_latest()
        .unwrap_or(COMMISSION_MAX);
    let commission_check = commission > commission_threshold;
    (commission_check, commission)
}

/// Checks if the validator is blacklisted
pub fn calculate_instant_unstake_blacklist(config: &Config, validator_index: u32) -> Result<bool> {
    config
        .validator_history_blacklist
        .get(validator_index as usize)
}

/// Checks if the validator is using allowed Tip Distribution merkle root upload authority
pub fn calculate_instant_unstake_merkle_root_upload_auth(
    latest_authority: &Option<MerkleRootUploadAuthority>,
) -> Result<bool> {
    if let Some(merkle_root_upload_authority) = latest_authority {
        match merkle_root_upload_authority {
            MerkleRootUploadAuthority::OldJitoLabs => Ok(false),
            MerkleRootUploadAuthority::TipRouter => Ok(false),
            _ => Ok(true),
        }
    } else {
        // Default to false (score 1) to be conservative. There are plenty of other mechanisms
        // that prevent a validator with no history from getting stake, so we don't want this to be
        // the hidden linchpin
        Ok(false)
    }
}
