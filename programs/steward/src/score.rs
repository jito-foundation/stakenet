use anchor_lang::{
    prelude::event, solana_program::pubkey::Pubkey, AnchorDeserialize, AnchorSerialize, IdlBuild,
    Result,
};
use validator_history::{ClusterHistory, ValidatorHistory};

use crate::{
    constants::{
        BASIS_POINTS_MAX, COMMISSION_MAX, EPOCH_DEFAULT, VALIDATOR_HISTORY_FIRST_RELIABLE_EPOCH,
    },
    errors::StewardError::{self, ArithmeticError},
    Config,
};

#[event]
#[derive(Debug, PartialEq)]
pub struct ScoreComponentsV2 {
    /// Product of all scoring components
    pub score: f64,

    /// vote_credits_ratio * (1 - commission)
    pub yield_score: f64,

    /// If max mev commission in mev_commission_range epochs is less than threshold, score is 1.0, else 0
    pub mev_commission_score: f64,

    /// Max MEV commission observed
    pub max_mev_commission: u16,

    /// Epoch of max MEV commission
    pub max_mev_commission_epoch: u16,

    /// If validator is blacklisted, score is 0.0, else 1.0
    pub blacklisted_score: f64,

    /// If validator is not in the superminority, score is 1.0, else 0.0
    pub superminority_score: f64,

    /// Epoch when superminority was detected
    pub superminority_epoch: u16,

    /// If delinquency is not > threshold in any epoch, score is 1.0, else 0.0
    pub delinquency_score: f64,

    /// Ratio that failed delinquency check
    pub delinquency_ratio: f64,

    /// Epoch when delinquency was detected
    pub delinquency_epoch: u16,

    /// If validator has a mev commission in the last 10 epochs, score is 1.0, else 0.0
    pub running_jito_score: f64,

    /// If max commission in commission_range epochs is less than commission_threshold, score is 1.0, else 0.0
    pub commission_score: f64,

    /// Max commission observed
    pub max_commission: u8,

    /// Epoch of max commission
    pub max_commission_epoch: u16,

    /// If max commission in all validator history epochs is less than historical_commission_threshold, score is 1.0, else 0.0
    pub historical_commission_score: f64,

    /// Average vote credits in last epoch_credits_range epochs / average blocks in last epoch_credits_range epochs
    /// Excluding current epoch
    pub vote_credits_ratio: f64,

    pub vote_account: Pubkey,

    pub epoch: u16,
}

pub fn validator_score(
    validator: &ValidatorHistory,
    cluster: &ClusterHistory,
    config: &Config,
    current_epoch: u16,
) -> Result<ScoreComponentsV2> {
    let params = &config.parameters;

    /////// MEV Commission ///////
    let mev_commission_window = validator.history.mev_commission_range(
        current_epoch
            .checked_sub(params.mev_commission_range)
            .ok_or(ArithmeticError)?,
        current_epoch,
    );
    let (max_mev_commission, max_mev_commission_epoch) = mev_commission_window
        .iter()
        .rev()
        .enumerate()
        .filter_map(|(i, &commission)| commission.map(|c| (c, current_epoch - i as u16)))
        .max_by_key(|&(commission, _)| commission)
        .unwrap_or((BASIS_POINTS_MAX, current_epoch));

    let mev_commission_score = if max_mev_commission <= params.mev_commission_bps_threshold {
        1.0
    } else {
        0.0
    };

    /////// Running Jito ///////
    let running_jito_score =
        if mev_commission_window.iter().any(|i| i.is_some()) {
            1.0
        } else {
            0.0
        };

    /////// Vote Credits Ratio, Delinquency ///////

    let epoch_credits_start = current_epoch
        .checked_sub(params.epoch_credits_range)
        .ok_or(ArithmeticError)?;
    let epoch_credits_end = current_epoch.checked_sub(1).ok_or(ArithmeticError)?;

    let epoch_credits_window = validator
        .history
        .epoch_credits_range(epoch_credits_start, epoch_credits_end);

    let average_vote_credits = epoch_credits_window.iter().filter_map(|&i| i).sum::<u32>() as f64
        / epoch_credits_window.len() as f64;

    let total_blocks_window = cluster
        .history
        .total_blocks_range(epoch_credits_start, epoch_credits_end);

    let average_blocks = total_blocks_window.iter().filter_map(|&i| i).sum::<u32>() as f64
        / total_blocks_window.iter().filter(|i| i.is_some()).count() as f64;

    let mut delinquency_score = 1.0;
    let mut delinquency_ratio = 1.0;
    let mut delinquency_epoch = EPOCH_DEFAULT;

    for (i, (maybe_credits, maybe_blocks)) in epoch_credits_window
        .iter()
        .zip(total_blocks_window.iter())
        .enumerate()
    {
        if let (Some(credits), Some(blocks)) = (maybe_credits, maybe_blocks) {
            let ratio = *credits as f64 / *blocks as f64;
            if ratio < params.scoring_delinquency_threshold_ratio {
                delinquency_score = 0.0;
                delinquency_ratio = ratio;
                delinquency_epoch = epoch_credits_start + i as u16;
                break;
            }
        }
    }

    /////// Commission ///////

    let commission_window = validator.history.commission_range(
        current_epoch
            .checked_sub(params.commission_range)
            .ok_or(ArithmeticError)?,
        current_epoch,
    );
    let (max_commission, max_commission_epoch) = commission_window
        .iter()
        .rev()
        .enumerate()
        .filter_map(|(i, &commission)| commission.map(|c| (c, current_epoch - i as u16)))
        .max_by_key(|&(commission, _)| commission)
        .unwrap_or((0, current_epoch));

    let commission_score = if max_commission <= params.commission_threshold {
        1.0
    } else {
        0.0
    };
    let commission = max_commission as f64 / COMMISSION_MAX as f64;

    /////// Historical Commission ///////

    let historical_commission_max = validator
        .history
        .commission_range(VALIDATOR_HISTORY_FIRST_RELIABLE_EPOCH as u16, current_epoch)
        .iter()
        .filter_map(|&i| i)
        .max()
        .unwrap_or(0);

    let historical_commission_score =
        if historical_commission_max <= params.historical_commission_threshold {
            1.0
        } else {
            0.0
        };

    /////// Superminority ///////
    let (superminority_score, superminority_epoch) =
        if validator.history.epoch_credits_latest().is_some() {
            if let Some(superminority) = validator.history.superminority_latest() {
                if superminority == 1 {
                    (0.0, current_epoch)
                } else {
                    (1.0, EPOCH_DEFAULT)
                }
            } else {
                return Err(StewardError::StakeHistoryNotRecentEnough.into());
            }
        } else {
            let superminority_window = validator.history.superminority_range(
                current_epoch
                    .checked_sub(params.commission_range)
                    .ok_or(ArithmeticError)?,
                current_epoch,
            );

            let (status, epoch) = superminority_window
                .iter()
                .enumerate()
                .rev()
                .find_map(|(i, &superminority)| {
                    superminority.map(|s| (s, current_epoch - i as u16))
                })
                .unwrap_or((0, current_epoch));

            if status == 1 {
                (0.0, epoch)
            } else {
                (1.0, EPOCH_DEFAULT)
            }
        };

    /////// Blacklist ///////
    let blacklisted_score = if config
        .validator_history_blacklist
        .get(validator.index as usize)?
    {
        0.0
    } else {
        1.0
    };

    /////// Formula ///////

    let yield_score = (average_vote_credits / average_blocks) * (1. - commission);

    let score = mev_commission_score
        * commission_score
        * historical_commission_score
        * blacklisted_score
        * superminority_score
        * delinquency_score
        * running_jito_score
        * yield_score;

    Ok(ScoreComponentsV2 {
        score,
        yield_score,
        mev_commission_score,
        max_mev_commission,
        max_mev_commission_epoch,
        blacklisted_score,
        superminority_score,
        superminority_epoch,
        delinquency_score,
        delinquency_ratio,
        delinquency_epoch,
        running_jito_score,
        commission_score,
        max_commission,
        max_commission_epoch,
        historical_commission_score,
        vote_credits_ratio: average_vote_credits / average_blocks,
        vote_account: validator.vote_account,
        epoch: current_epoch,
    })
}

#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct InstantUnstakeComponentsV2 {
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

    pub vote_account: Pubkey,

    pub epoch: u16,

    /// Latest epoch credits
    pub epoch_credits_latest: u64,

    /// Latest vote account slot
    pub vote_account_latest_slot: u64,

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
) -> Result<InstantUnstakeComponentsV2> {
    let params = &config.parameters;

    /////// Delinquency ///////
    // Compare validator vote rate against cluster block production rate this epoch
    let cluster_history_slot_index = cluster
        .cluster_history_last_update_slot
        .checked_sub(epoch_start_slot)
        .ok_or(StewardError::ArithmeticError)?;

    let total_blocks_latest = cluster
        .history
        .total_blocks_latest()
        .ok_or(StewardError::ClusterHistoryNotRecentEnough)?;

    let blocks_produced_rate = total_blocks_latest as f64 / cluster_history_slot_index as f64;

    let vote_account_latest_slot = validator
        .history
        .vote_account_last_update_slot_latest()
        .ok_or(StewardError::VoteHistoryNotRecentEnough)?;

    let validator_history_slot_index = vote_account_latest_slot
        .checked_sub(epoch_start_slot)
        .ok_or(StewardError::ArithmeticError)?;

    let epoch_credits_latest = validator.history.epoch_credits_latest().unwrap_or(0);
    let vote_credits_rate = epoch_credits_latest as f64 / validator_history_slot_index as f64;

    let delinquency_check = if blocks_produced_rate > 0. {
        (vote_credits_rate / blocks_produced_rate)
            < params.instant_unstake_delinquency_threshold_ratio
    } else {
        false
    };

    /////// MEV Commission ///////
    // If MEV commission isn't set, we won't unstake because there may be issues setting tip distribution acct.
    // Checks previous and current in case this validator happens to have its first slot late in the epoch
    let previous_epoch = current_epoch.saturating_sub(1);
    let mev_commission_previous_current = validator
        .history
        .mev_commission_range(previous_epoch, current_epoch);
    let mev_commission_bps = mev_commission_previous_current
        .iter()
        .filter_map(|&i| i)
        .max()
        .unwrap_or(0);
    let mev_commission_check = mev_commission_bps > params.mev_commission_bps_threshold;

    /////// Commission ///////

    let commission = validator
        .history
        .commission_latest()
        .unwrap_or(COMMISSION_MAX);

    let commission_check = commission > params.commission_threshold;

    /////// Blacklist ///////
    let is_blacklisted = config
        .validator_history_blacklist
        .get(validator.index as usize)?;

    let instant_unstake =
        delinquency_check || commission_check || mev_commission_check || is_blacklisted;
    Ok(InstantUnstakeComponentsV2 {
        instant_unstake,
        delinquency_check,
        commission_check,
        mev_commission_check,
        is_blacklisted,
        vote_account: validator.vote_account,
        epoch: current_epoch,
        epoch_credits_latest: epoch_credits_latest as u64,
        vote_account_latest_slot,
        total_blocks_latest,
        cluster_history_slot_index,
        commission,
        mev_commission: mev_commission_bps,
    })
}
