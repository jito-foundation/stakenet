#[cfg(feature = "idl-build")]
use anchor_lang::IdlBuild;
use anchor_lang::{
    prelude::event, solana_program::pubkey::Pubkey, AnchorDeserialize, AnchorSerialize,
    Discriminator, Result,
};
use serde::{Deserialize, Serialize};
use validator_history::{
    constants::TVC_MULTIPLIER, ClusterHistory, ClusterHistoryEntry, MerkleRootUploadAuthority,
    ValidatorHistory, ValidatorHistoryEntry,
};

use crate::{
    constants::{
        BASIS_POINTS_MAX, COMMISSION_MAX, EPOCH_DEFAULT, VALIDATOR_HISTORY_FIRST_RELIABLE_EPOCH,
        VOTE_CREDITS_RATIO_MAX,
    },
    errors::StewardError::{self, ArithmeticError},
    score::running_bam::calculate_running_bam_score,
    Config,
};

pub mod running_bam;

/// Components of a validator score, organized by priority tiers.
///
/// Validator scores are encoded as a single u64 value with a hierarchical structure
/// where higher-order bits represent more important factors. When comparing scores,
/// differences in Tier 1 (inflation commission) always dominate lower tiers, creating
/// a strict priority ordering.
///
/// The encoding uses 64 bits distributed across 4 tiers:
/// - Bits 56-63 (8 bits):  Tier 1 - Inflation commission (inverted)
/// - Bits 42-55 (14 bits): Tier 2 - MEV commission (inverted)
/// - Bits 25-41 (17 bits): Tier 3 - Validator age (direct)
/// - Bits 0-24 (25 bits):  Tier 4 - Vote credits ratio (direct)
///
/// Higher raw scores are always better, as commission values are inverted during encoding.
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidatorScoreComponents {
    /// Inflation Commission (0-100%)
    ///
    /// **Tier 1 - Highest Priority** (bits 56-63)
    ///
    /// Lower validator commission is always preferred. This tier uses the maximum
    /// commission observed in the commission_range, inverted so lower commission
    /// yields a higher score. A validator with 5% commission will always rank above
    /// a validator with 10% commission, regardless of other factors.
    pub inflation_commission: u8,

    /// MEV Commission (0-10000 basis points, where 10000 = 100%)
    ///
    /// **Tier 2** (bits 42-55)
    ///
    /// Among validators with equal inflation commission, lower MEV commission is
    /// preferred. This tier uses the average MEV commission over mev_commission_range
    /// epochs, inverted so lower commission yields a higher score.
    pub mev_commission_bps: u16,

    /// Validator Age (epochs with non-zero vote credits)
    ///
    /// **Tier 3** (bits 25-41)
    ///
    /// Among validators equal on both commission tiers, older validators are preferred.
    /// Age is measured in epochs where the validator produced non-zero vote credits,
    /// rewarding longevity and reliability. Maximum representable age is 131,071 epochs
    /// (approximately 716 years at ~2 days per epoch).
    pub validator_age: u32,

    /// Vote Credits Ratio (normalized performance score)
    ///
    /// **Tier 4 - Lowest Priority** (bits 0-24)
    ///
    /// Among validators equal on all above tiers, higher performance is preferred.
    /// This represents vote credits relative to total possible credits, scaled by
    /// VOTE_CREDITS_RATIO_MAX for precision. Maximum representable value is 33,554,431.
    pub vote_credits: u32,
}

impl std::fmt::Display for ValidatorScoreComponents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Inflation Commission: {}\nMEV commission BPS: {}\nValidator Age: {}\nVote Credits: {}\n", self.inflation_commission, self.mev_commission_bps, self.validator_age, self.vote_credits)
    }
}

impl ValidatorScoreComponents {
    /// Decodes a raw validator score into its component parts.
    ///
    /// # Example
    ///
    /// ```
    /// use jito_steward::score::{encode_validator_score, ValidatorScoreComponents};
    ///
    /// // Perfect validator: 0% commissions, max age, max credits
    /// let inflation_commission = 0;
    /// let mev_commission_bps = 0;
    /// let validator_age = 131071;
    /// let vote_credits = 33554431;
    ///
    /// let score = encode_validator_score(inflation_commission, mev_commission_bps, validator_age, vote_credits).unwrap();
    ///
    /// let components = ValidatorScoreComponents::decode(score);
    /// assert_eq!(components.inflation_commission, inflation_commission);
    /// assert_eq!(components.mev_commission_bps, mev_commission_bps);
    /// assert_eq!(components.validator_age, validator_age);
    /// assert_eq!(components.vote_credits, vote_credits);
    /// ```
    pub fn decode(raw_score: u64) -> Self {
        // Tier 1: Extract inflation commission score (bits 56-63) and invert
        let inflation_score = (raw_score >> 56) & 0xFF;
        let inflation_commission = 100u8.saturating_sub(inflation_score as u8);

        // Tier 2: Extract MEV commission score (bits 42-55) and invert
        let mev_score = (raw_score >> 42) & 0x3FFF; // 14 bits
        let mev_commission_bps = 10000u16.saturating_sub(mev_score as u16);

        // Tier 3: Extract validator age directly (bits 25-41)
        let validator_age = ((raw_score >> 25) & 0x1FFFF) as u32; // 17 bits

        // Tier 4: Extract vote credits directly (bits 0-24)
        let vote_credits = (raw_score & 0x1FFFFFF) as u32; // 25 bits

        Self {
            inflation_commission,
            mev_commission_bps,
            validator_age,
            vote_credits,
        }
    }
}

/// Encode a 4-tier validator score into a u64 with the following bit layout:
/// Bits 56-63 (8 bits):  Inflation commission (inverted, 0-100%)
/// Bits 42-55 (14 bits): MEV commission (inverted, 0-10000 bps)
/// Bits 25-41 (17 bits): Validator age (direct, epochs)
/// Bits 0-24 (25 bits):  Vote credits (direct value)
///
/// The tiers are in descending order of importance. The highest bits (56-63) contain
/// the most important factor (inflation commission), so when comparing scores as u64 values,
/// differences in higher-order bits will dominate lower-order bits. This creates a
/// hierarchical comparison where inflation commission > MEV commission > age > credits.
///
/// Higher scores are better in all cases.
pub fn encode_validator_score(
    inflation_commission: u8, // 0-100
    mev_commission_bps: u16,  // 0-10000
    validator_age: u32,       // epochs with non-zero vote credits
    vote_credits: u32,        // normalized vote credits ratio scaled by VOTE_CREDITS_RATIO_MAX
) -> Result<u64> {
    // Tier 1: Inflation commission (inverted so lower commission = higher score)
    let inflation_score = 100u64.saturating_sub(inflation_commission.min(COMMISSION_MAX) as u64);

    // Tier 2: MEV commission (inverted so lower commission = higher score)
    let mev_score =
        (BASIS_POINTS_MAX as u64).saturating_sub(mev_commission_bps.min(BASIS_POINTS_MAX) as u64);

    // Tier 3: Validator age (direct - older validators score higher)
    // Cap at 17 bits max value (131,071 epochs = ~716 years)
    let age_score = (validator_age as u64).min((1u64 << 17) - 1);

    // Tier 4: Vote credits ratio (normalized performance, scaled by 10M for precision)
    // Cap at 25 bits max value (33,554,431)
    let credits_score = (vote_credits as u64).min((1u64 << 25) - 1);

    // Combine into single u64
    let score = (inflation_score << 56) | (mev_score << 42) | (age_score << 25) | credits_score;

    Ok(score)
}

/// Calculate the average MEV commission over a window of epochs
pub fn calculate_avg_mev_commission(
    validator: &ValidatorHistory,
    current_epoch: u16,
    window_size: u16,
) -> u16 {
    let start_epoch = current_epoch.saturating_sub(window_size);
    let mev_commission_window = validator
        .history
        .mev_commission_range(start_epoch, current_epoch);

    // Calculate sum and count without allocating a Vec
    let (sum, count) = mev_commission_window
        .iter()
        .filter_map(|&c| c)
        .fold((0u64, 0u64), |(sum, count), c| {
            (sum.saturating_add(c as u64), count + 1)
        });

    if count == 0 {
        // Default to max if no data
        return BASIS_POINTS_MAX;
    }

    // Calculate average with ceiling (round up to be more strict)
    // Add (count - 1) to implement ceiling division: (sum + count - 1) / count
    let avg = sum
        .checked_add(count.saturating_sub(1))
        .unwrap_or(sum)
        .checked_div(count)
        .unwrap_or(BASIS_POINTS_MAX as u64);

    // Safely convert back to u16, capping at max
    avg.min(BASIS_POINTS_MAX as u64) as u16
}

#[event]
#[derive(Debug, PartialEq)]
pub struct ScoreComponentsV5 {
    /// Final score with binary filters applied to raw_score (0 if any filter fails, raw_score otherwise)
    pub score: u64,

    /// The 4-tier encoded score (before binary filters)
    pub raw_score: u64,

    /// Maximum inflation commission used in scoring (0-100)
    pub commission_max: u8,

    /// Average MEV commission used in scoring (basis points)
    pub mev_commission_avg: u16,

    /// Validator age in epochs (number of epochs with non-zero vote credits)
    pub validator_age: u32,

    /// Average vote credits over the window
    pub vote_credits_avg: u32,

    /// If max mev commission in mev_commission_range epochs is less than threshold, score is 1, else 0
    pub mev_commission_score: u8,

    /// If validator is blacklisted, score is 0, else 1
    pub blacklisted_score: u8,

    /// If validator is not in the superminority, score is 1, else 0
    pub superminority_score: u8,

    /// If delinquency is not > threshold in any epoch, score is 1, else 0
    pub delinquency_score: u8,

    /// Score is 1 if the validator has been connected to BAM for at least
    /// `jito_bam_minimum_epochs` out of the last `jito_bam_window_epochs` epochs, otherwise 0.
    pub running_bam_score: u8,

    /// If max commission in commission_range epochs is less than commission_threshold, score is 1, else 0
    pub commission_score: u8,

    /// If max commission in all validator history epochs is less than historical_commission_threshold, score is 1, else 0
    pub historical_commission_score: u8,

    /// If validator is using TipRouter authority, OR OldJito authority then score is 1, else 0
    pub merkle_root_upload_authority_score: u8,

    pub vote_account: Pubkey,

    pub epoch: u16,

    /// Details about why a given score was calculated
    pub details: ScoreDetails,

    /// If validator has realized priority fee commissions > config limits over a lookback range,
    /// score 0.
    pub priority_fee_commission_score: u8,

    /// If validator is using TipRouter authority, OR OldJito authority then score is 1, else 0
    pub priority_fee_merkle_root_upload_authority_score: u8,
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

    /// Average realized priority fee commission observed
    pub avg_priority_fee_commission: u16,

    /// Epoch of realized priority fee commission
    pub max_priority_fee_commission_epoch: u16,
}

pub fn validator_score(
    validator: &ValidatorHistory,
    cluster: &ClusterHistory,
    config: &Config,
    current_epoch: u16,
    tvc_activation_epoch: u64,
    slots_per_epoch: u64,
) -> Result<ScoreComponentsV5> {
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

    let normalized_epoch_credits_window = epoch_credits_range(
        validator,
        cluster,
        epoch_credits_start,
        epoch_credits_end,
        tvc_activation_epoch,
        slots_per_epoch,
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

    /////// Binary filter calculations ///////
    let (mev_commission_score, max_mev_commission, max_mev_commission_epoch) =
        calculate_max_mev_commission(
            &mev_commission_window,
            current_epoch,
            params.mev_commission_bps_threshold,
        )?;

    let (vote_credits_ratio, delinquency_score, delinquency_ratio, delinquency_epoch) =
        calculate_scorable_epoch_credits(
            &normalized_epoch_credits_window,
            &total_blocks_window,
            epoch_credits_start,
            params.scoring_delinquency_threshold_ratio,
        )?;

    let (commission_score, max_commission, max_commission_epoch) = calculate_max_commission(
        &commission_window,
        current_epoch,
        params.commission_threshold,
    )?;

    /////// Calculate 4-tier score components ///////
    // Use max_commission from the binary filter calculation above
    let mev_commission_avg =
        calculate_avg_mev_commission(validator, current_epoch, params.mev_commission_range);
    let validator_age = validator.validator_age;

    // Scale the normalized vote credits ratio for precision and cap at 25 bits
    let scaled_ratio = (vote_credits_ratio * VOTE_CREDITS_RATIO_MAX as f64) as u64;
    let vote_credits_avg = scaled_ratio.min((1u64 << 25) - 1) as u32;

    // Calculate raw 4-tier score
    let raw_score = encode_validator_score(
        max_commission,
        mev_commission_avg,
        validator_age,
        vote_credits_avg,
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
        calculate_priority_fee_merkle_root_authority_score(validator)?;

    let (
        priority_fee_commission_score,
        avg_priority_fee_commission,
        max_priority_fee_commission_epoch,
    ) = calculate_priority_fee_commission(config, validator, current_epoch)?;

    // Exclude the current epoch from the BAM window so the cranker has time
    // to upload BAM eligibility for this epoch (it's a permissioned field).
    let bam_window_end = current_epoch.checked_sub(1).ok_or(ArithmeticError)?;
    let bam_window_start = current_epoch
        .checked_sub(params.jito_bam_window_epochs as u16)
        .ok_or(ArithmeticError)?;
    let is_bam_connected_window = validator
        .history
        .is_bam_connected_range(bam_window_start, bam_window_end);

    let running_bam_score =
        calculate_running_bam_score(&is_bam_connected_window, params.jito_bam_minimum_epochs);

    /////// Apply binary filters to raw score ///////
    // Binary filters are 0 or 1, multiply them with the raw_score
    let score = raw_score
        * mev_commission_score as u64
        * commission_score as u64
        * historical_commission_score as u64
        * blacklisted_score as u64
        * superminority_score as u64
        * delinquency_score as u64
        * running_bam_score as u64
        * merkle_root_upload_authority_score as u64
        * priority_fee_commission_score as u64
        * priority_fee_merkle_root_upload_authority_score as u64;

    Ok(ScoreComponentsV5 {
        score,
        raw_score,
        commission_max: max_commission,
        mev_commission_avg,
        validator_age,
        vote_credits_avg,
        mev_commission_score,
        blacklisted_score,
        superminority_score,
        delinquency_score,
        running_bam_score,
        commission_score,
        historical_commission_score,
        merkle_root_upload_authority_score,
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
            avg_priority_fee_commission,
            max_priority_fee_commission_epoch,
        },
        priority_fee_commission_score,
        priority_fee_merkle_root_upload_authority_score,
    })
}

/// Finds max MEV commission in the last `mev_commission_range` epochs and determines if it is above a threshold.
pub fn calculate_max_mev_commission(
    mev_commission_window: &[Option<u16>],
    current_epoch: u16,
    mev_commission_bps_threshold: u16,
) -> Result<(u8, u16, u16)> {
    let (max_mev_commission, max_mev_commission_epoch) = mev_commission_window
        .iter()
        .rev()
        .enumerate()
        .filter_map(|(i, &commission)| commission.map(|c| (c, current_epoch.checked_sub(i as u16))))
        .max_by_key(|&(commission, _)| commission)
        .unwrap_or((BASIS_POINTS_MAX, Some(current_epoch)));

    let max_mev_commission_epoch = max_mev_commission_epoch.ok_or(StewardError::ArithmeticError)?;

    let mev_commission_score = if max_mev_commission <= mev_commission_bps_threshold {
        1
    } else {
        0
    };

    Ok((
        mev_commission_score,
        max_mev_commission,
        max_mev_commission_epoch,
    ))
}

/// Calculates the vote credits ratio and delinquency score for the validator
pub fn calculate_epoch_credits(
    epoch_credits_window: &[Option<u32>],
    total_blocks_window: &[Option<u32>],
    epoch_credits_start: u16,
    scoring_delinquency_threshold_ratio: f64,
) -> Result<(f64, u8, f64, u16)> {
    if epoch_credits_window.is_empty() || total_blocks_window.is_empty() {
        return Err(StewardError::ArithmeticError.into());
    }

    let average_vote_credits = epoch_credits_window
        .iter()
        .filter_map(|&i| i)
        .map(u64::from)
        .sum::<u64>() as f64
        / epoch_credits_window.len() as f64;

    let nonzero_blocks = total_blocks_window.iter().filter(|i| i.is_some()).count();
    if nonzero_blocks == 0 {
        return Err(StewardError::ArithmeticError.into());
    }

    // Get average of total blocks in window, ignoring values where upload was missed
    let average_blocks =
        total_blocks_window.iter().filter_map(|&i| i).sum::<u32>() as f64 / nonzero_blocks as f64;

    // Delinquency heuristic - not actual delinquency
    let mut delinquency_score = 1u8;
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
                delinquency_score = 0;
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

/// Epoch credits a validator earned in one epoch, normalized to timely vote credits
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EpochCredits {
    /// Credits earned, `None` if the validator earned none
    Scored(Option<u32>),

    /// Inputs are missing, or the credits mix tower vote credits with alpenglow reward lamports
    Unscorable,
}

/// Epoch credits for each epoch in [start_epoch, end_epoch], normalized so that
/// `credits / (total_blocks * TVC_MULTIPLIER)` means the same in tower and alpenglow epochs.
///
/// Epochs flagged `is_alpenglow` in cluster history hold vote reward lamports, which are converted
/// with `alpenglow_epoch_credits`. Any other epoch holding more credits than tower can pay out must
/// contain alpenglow lamports (the migration epoch, or an alpenglow epoch whose inflation rewards
/// haven't been recorded yet), so it is unscorable.
pub fn epoch_credits_range(
    validator: &ValidatorHistory,
    cluster: &ClusterHistory,
    start_epoch: u16,
    end_epoch: u16,
    tvc_activation_epoch: u64,
    slots_per_epoch: u64,
) -> Vec<EpochCredits> {
    let tower_credits = validator.history.epoch_credits_range_normalized(
        start_epoch,
        end_epoch,
        tvc_activation_epoch,
    );
    let max_tower_credits = u64::from(TVC_MULTIPLIER).saturating_mul(slots_per_epoch);

    // Alpenglow pays each epoch's vote rewards against the stake recorded in the epoch before it
    let lookback_epoch = start_epoch.saturating_sub(1);
    let validator_entries = validator.history.epoch_range(lookback_epoch, end_epoch);
    let cluster_entries = cluster.history.epoch_range(lookback_epoch, end_epoch);

    (start_epoch..=end_epoch)
        .zip(tower_credits)
        .map(|(epoch, tower_credits)| {
            let index = (epoch - lookback_epoch) as usize;
            if let Some(cluster_entry) = cluster_entries[index].filter(|e| e.is_alpenglow == 1) {
                let previous_index = index.checked_sub(1);
                return alpenglow_entry_credits(
                    validator_entries[index],
                    previous_index.and_then(|i| validator_entries[i]),
                    cluster_entry,
                    previous_index.and_then(|i| cluster_entries[i]),
                    slots_per_epoch,
                );
            }

            match tower_credits {
                Some(credits) if u64::from(credits) > max_tower_credits => EpochCredits::Unscorable,
                credits => EpochCredits::Scored(credits),
            }
        })
        .collect()
}

/// Reads the inputs of `alpenglow_epoch_credits` for one alpenglow epoch from validator and cluster history.
fn alpenglow_entry_credits(
    entry: Option<&ValidatorHistoryEntry>,
    previous_entry: Option<&ValidatorHistoryEntry>,
    cluster_entry: &ClusterHistoryEntry,
    previous_cluster_entry: Option<&ClusterHistoryEntry>,
    slots_per_epoch: u64,
) -> EpochCredits {
    let default_entry = ValidatorHistoryEntry::default();
    let default_cluster_entry = ClusterHistoryEntry::default();

    // `copy_vote_account` backfills every epoch the vote account earned in, so no entry means no credits
    let Some(entry) = entry else {
        return EpochCredits::Scored(None);
    };
    if entry.epoch_credits_uncapped == default_entry.epoch_credits_uncapped {
        return if entry.epoch_credits == default_entry.epoch_credits {
            EpochCredits::Scored(None)
        } else {
            // Copied before uncapped credits were recorded
            EpochCredits::Unscorable
        };
    }

    // Prefer the stake alpenglow paid against, falling back to the stake oracle's upload
    let reward_stake_lamports = match previous_entry {
        Some(previous) if previous.epoch_stake_lamports != default_entry.epoch_stake_lamports => {
            previous.epoch_stake_lamports
        }
        Some(previous)
            if previous.activated_stake_lamports != default_entry.activated_stake_lamports =>
        {
            previous.activated_stake_lamports
        }
        _ => return EpochCredits::Unscorable,
    };
    let total_reward_stake_lamports = match previous_cluster_entry {
        Some(previous)
            if previous.total_epoch_stake_lamports
                != default_cluster_entry.total_epoch_stake_lamports =>
        {
            previous.total_epoch_stake_lamports
        }
        _ => return EpochCredits::Unscorable,
    };
    if cluster_entry.total_inflation_rewards == default_cluster_entry.total_inflation_rewards
        || cluster_entry.distributed_inflation_rewards
            == default_cluster_entry.distributed_inflation_rewards
    {
        return EpochCredits::Unscorable;
    }

    match alpenglow_epoch_credits(
        entry.epoch_credits_uncapped,
        reward_stake_lamports,
        total_reward_stake_lamports,
        cluster_entry.total_inflation_rewards,
        cluster_entry.distributed_inflation_rewards,
        slots_per_epoch,
    ) {
        Some(credits) => EpochCredits::Scored(Some(credits)),
        None => EpochCredits::Unscorable,
    }
}

/// Converts an alpenglow epoch's vote reward lamports into the timely vote credits they're worth, so
/// the epoch can be scored against `total_blocks * TVC_MULTIPLIER` like a tower epoch.
///
/// Every block's reward certificate pays each voter in it
/// `total_inflation_rewards * reward_stake / (2 * slots_per_epoch * total_reward_stake)`, and pays
/// the block's leader the other half of every voter's reward. The leader's share is estimated from
/// the validator's share of the stake, since the leader schedule is stake-weighted, rather than from
/// the oracle's block counts; dividing what's left by the reward per certificate counts the
/// certificates the validator voted in.
///
/// Returns `None` if the validator had no stake to earn with, or the cluster inputs are empty.
pub fn alpenglow_epoch_credits(
    reward_lamports: u64,
    reward_stake_lamports: u64,
    total_reward_stake_lamports: u64,
    total_inflation_rewards: u64,
    distributed_inflation_rewards: u64,
    slots_per_epoch: u64,
) -> Option<u32> {
    if reward_stake_lamports == 0
        || total_reward_stake_lamports == 0
        || total_inflation_rewards == 0
        || slots_per_epoch == 0
    {
        return None;
    }

    // A stake-weighted leader schedule gives the validator `reward_stake / total_reward_stake` of
    // the epoch's blocks, each paying half of what that block's certificate paid out
    let leader_lamports = distributed_inflation_rewards as f64 * reward_stake_lamports as f64
        / (2. * total_reward_stake_lamports as f64);
    let vote_lamports = (reward_lamports as f64 - leader_lamports).max(0.);
    let lamports_per_vote = total_inflation_rewards as f64 * reward_stake_lamports as f64
        / (2. * slots_per_epoch as f64 * total_reward_stake_lamports as f64);
    let votes = vote_lamports / lamports_per_vote;

    Some((votes * TVC_MULTIPLIER as f64).round() as u32)
}

/// `calculate_epoch_credits` over only the scorable epochs in the window, so unscorable epochs are
/// neither averaged in nor checked for delinquency.
pub fn calculate_scorable_epoch_credits(
    epoch_credits_window: &[EpochCredits],
    total_blocks_window: &[Option<u32>],
    epoch_credits_start: u16,
    scoring_delinquency_threshold_ratio: f64,
) -> Result<(f64, u8, f64, u16)> {
    let mut epochs = Vec::with_capacity(epoch_credits_window.len());
    let mut credits_window = Vec::with_capacity(epoch_credits_window.len());
    let mut blocks_window = Vec::with_capacity(epoch_credits_window.len());
    for (i, (epoch_credits, total_blocks)) in epoch_credits_window
        .iter()
        .zip(total_blocks_window.iter())
        .enumerate()
    {
        if let EpochCredits::Scored(credits) = epoch_credits {
            epochs.push(
                epoch_credits_start
                    .checked_add(i as u16)
                    .ok_or(StewardError::ArithmeticError)?,
            );
            credits_window.push(*credits);
            blocks_window.push(*total_blocks);
        }
    }

    if epochs.is_empty() {
        // Nothing to judge the validator by, so it isn't treated as delinquent
        return Ok((0., 1, 1., EPOCH_DEFAULT));
    }

    let (vote_credits_ratio, delinquency_score, delinquency_ratio, delinquency_index) =
        calculate_epoch_credits(
            &credits_window,
            &blocks_window,
            0,
            scoring_delinquency_threshold_ratio,
        )?;
    let delinquency_epoch = if delinquency_score == 0 {
        *epochs
            .get(delinquency_index as usize)
            .ok_or(StewardError::ArithmeticError)?
    } else {
        EPOCH_DEFAULT
    };

    Ok((
        vote_credits_ratio,
        delinquency_score,
        delinquency_ratio,
        delinquency_epoch,
    ))
}

/// Finds max commission in the last `commission_range` epochs
pub fn calculate_max_commission(
    commission_window: &[Option<u8>],
    current_epoch: u16,
    commission_threshold: u8,
) -> Result<(u8, u8, u16)> {
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
        1
    } else {
        0
    };

    Ok((commission_score, max_commission, max_commission_epoch))
}

/// Checks if validator has commission above a threshold in any epoch in their history
pub fn calculate_historical_commission(
    validator: &ValidatorHistory,
    current_epoch: u16,
    historical_commission_threshold: u8,
) -> Result<(u8, u8, u16)> {
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
            1
        } else {
            0
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
) -> Result<(u8, u16)> {
    /*
        If epoch credits exist, we expect the validator to have a superminority flag set. If not, scoring fails and we wait for
        the stake oracle to call UpdateStakeHistory.
        If epoch credits is not set, we iterate through last `commission_range` epochs to find the latest superminority flag.
        If no entry is found, we assume the validator is not a superminority validator.
    */
    if validator.history.epoch_credits_latest().is_some() {
        if let Some(superminority) = validator.history.superminority_latest() {
            if superminority == 1 {
                Ok((0, current_epoch))
            } else {
                Ok((1, EPOCH_DEFAULT))
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
            Ok((0, epoch))
        } else {
            Ok((1, EPOCH_DEFAULT))
        }
    }
}

/// Checks if validator is blacklisted using the validator history index in the config's blacklist
pub fn calculate_blacklist_score(config: &Config, validator_index: u32) -> Result<u8> {
    if config
        .validator_history_blacklist
        .get(validator_index as usize)?
    {
        Ok(0)
    } else {
        Ok(1)
    }
}

/// Checks if validator is using appropriate TDA MerkleRootUploadAuthority
pub fn calculate_merkle_root_authority_score(validator: &ValidatorHistory) -> Result<u8> {
    // calculate_instant_unstake_merkle_root_upload_auth returns whether or not
    // instant unstake should be triggered, so we invert the result to get the score
    if calculate_instant_unstake_merkle_root_upload_auth(
        &validator.history.merkle_root_upload_authority_latest(),
    )? {
        Ok(0)
    } else {
        Ok(1)
    }
}

/// Checks if validator is using appropriate TDA MerkleRootUploadAuthority
pub fn calculate_priority_fee_merkle_root_authority_score(
    validator: &ValidatorHistory,
) -> Result<u8> {
    if calculate_instant_unstake_merkle_root_upload_auth(
        &validator
            .history
            .priority_fee_merkle_root_upload_authority_latest(),
    )? {
        Ok(0)
    } else {
        Ok(1)
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
    let commission = numerator.checked_div(total_fees).unwrap_or(0u64);
    u16::try_from(commission).unwrap_or(BASIS_POINTS_MAX)
}

/// Checks if validator is maintaining < X% realized commission rates over some history of epochs
pub fn calculate_priority_fee_commission(
    config: &Config,
    validator: &ValidatorHistory,
    current_epoch: u16,
) -> Result<(u8, u16, u16)> {
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
                let mut commission_bps: u16 = calculate_realized_commission_bps(tips, total_fees);
                if priority_fee_merkle_root_upload_authority.is_none() {
                    return vec![];
                }
                if let Some(upload_authority) = priority_fee_merkle_root_upload_authority {
                    if matches!(upload_authority, MerkleRootUploadAuthority::Unset) {
                        return vec![];
                    }
                    if matches!(upload_authority, MerkleRootUploadAuthority::DNE) {
                        commission_bps = BASIS_POINTS_MAX;
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
        return Ok((1, 0u16, max_priority_fee_commission_epoch));
    }

    // if there are no realized commissions due to Unset PFDA, return score 1, default
    // to not penalize the validator for not having a PFDA copied into their history
    if realized_commissions.is_empty() {
        return Ok((1, 0u16, max_priority_fee_commission_epoch));
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
    // We would still like to emit avg_commission before the go-live epoch
    if current_epoch < config.parameters.priority_fee_scoring_start_epoch {
        return Ok((1, avg_commission, EPOCH_DEFAULT));
    }
    if avg_commission <= max_commission {
        Ok((1, avg_commission, max_priority_fee_commission_epoch))
    } else {
        Ok((0, avg_commission, max_priority_fee_commission_epoch))
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
    slots_per_epoch: u64,
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
    // Alpenglow votes can only be counted once an epoch's inflation rewards are paid out, so after the
    // migration the validator is judged by the previous, complete epoch instead of the current one
    let previous_alpenglow_epoch = current_epoch.checked_sub(1).filter(|&epoch| {
        cluster
            .history
            .epoch_range(epoch, epoch)
            .first()
            .and_then(|entry| *entry)
            .is_some_and(|entry| entry.is_alpenglow == 1)
    });
    let (delinquency_check, epoch_credits_latest, total_blocks_latest) =
        match previous_alpenglow_epoch {
            Some(epoch) => calculate_alpenglow_instant_unstake_delinquency(
                validator,
                cluster,
                epoch,
                tvc_activation_epoch,
                slots_per_epoch,
                params.instant_unstake_delinquency_threshold_ratio,
            ),
            None => (
                calculate_instant_unstake_delinquency(
                    total_blocks_latest,
                    cluster_history_slot_index,
                    epoch_credits_latest,
                    validator_history_slot_index,
                    params.instant_unstake_delinquency_threshold_ratio,
                )?,
                epoch_credits_latest,
                total_blocks_latest,
            ),
        };

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

/// Checks whether the validator was delinquent in `epoch`, a complete alpenglow epoch, using the same
/// ratio as scoring. Returns the check along with the epoch credits and total blocks behind it.
fn calculate_alpenglow_instant_unstake_delinquency(
    validator: &ValidatorHistory,
    cluster: &ClusterHistory,
    epoch: u16,
    tvc_activation_epoch: u64,
    slots_per_epoch: u64,
    instant_unstake_delinquency_threshold_ratio: f64,
) -> (bool, u32, u32) {
    let epoch_credits = epoch_credits_range(
        validator,
        cluster,
        epoch,
        epoch,
        tvc_activation_epoch,
        slots_per_epoch,
    );
    let total_blocks = cluster
        .history
        .total_blocks_range(epoch, epoch)
        .first()
        .copied()
        .flatten()
        .unwrap_or(0);

    match epoch_credits.first() {
        Some(EpochCredits::Scored(credits)) if total_blocks > 0 => {
            let credits = credits.unwrap_or(0);
            let ratio = credits as f64 / (total_blocks as f64 * TVC_MULTIPLIER as f64);
            (
                ratio < instant_unstake_delinquency_threshold_ratio,
                credits,
                total_blocks,
            )
        }
        // An epoch that can't be scored is no reason to unstake
        _ => (false, 0, total_blocks),
    }
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
            // Although the statement above will cover Unset, we want to be explicit about it
            // and safegaurd against any future changes to the latest_authority that gets passed in
            MerkleRootUploadAuthority::Unset => Ok(false),
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

#[cfg(test)]
mod tests {
    use validator_history::{utils::MAX_EPOCH_CREDITS, CircBuf, CircBufCluster};

    use super::*;

    const SLOTS_PER_EPOCH: u64 = 432_000;
    const TOTAL_BLOCKS: u32 = 1_000;

    // 86_400 SOL of inflation over 432_000 slots, split with the leader, pays a validator holding
    // 0.1% of the stake 100_000 lamports per reward certificate
    const REWARD_STAKE: u64 = 1_000_000_000_000;
    const TOTAL_REWARD_STAKE: u64 = 1_000 * REWARD_STAKE;
    const INFLATION_REWARDS: u64 = 86_400_000_000_000;
    const LAMPORTS_PER_VOTE: u64 = 100_000;

    /// What a validator earns for voting in `votes` reward certificates and leading `blocks_produced`
    /// blocks, when `participation` of the stake votes in every certificate
    fn reward_lamports(votes: u64, blocks_produced: u64, participation: f64) -> u64 {
        let leader_lamports_per_block =
            INFLATION_REWARDS as f64 * participation / (2. * SLOTS_PER_EPOCH as f64);
        votes * LAMPORTS_PER_VOTE + (blocks_produced as f64 * leader_lamports_per_block) as u64
    }

    fn distributed_rewards(participation: f64) -> u64 {
        (TOTAL_BLOCKS as f64 * INFLATION_REWARDS as f64 * participation / SLOTS_PER_EPOCH as f64)
            as u64
    }

    fn credits(reward_lamports: u64, participation: f64) -> Option<u32> {
        alpenglow_epoch_credits(
            reward_lamports,
            REWARD_STAKE,
            TOTAL_REWARD_STAKE,
            INFLATION_REWARDS,
            distributed_rewards(participation),
            SLOTS_PER_EPOCH,
        )
    }

    fn entry_mut(validator: &mut ValidatorHistory, epoch: u16) -> &mut ValidatorHistoryEntry {
        validator
            .history
            .arr_mut()
            .iter_mut()
            .find(|entry| entry.epoch == epoch)
            .unwrap()
    }

    fn validator_history() -> ValidatorHistory {
        ValidatorHistory {
            struct_version: 0,
            vote_account: Pubkey::default(),
            index: 0,
            bump: 0,
            _padding0: [0; 7],
            last_ip_timestamp: 0,
            last_version_timestamp: 0,
            validator_age: 0,
            validator_age_last_updated_epoch: 0,
            _padding1: [0; 226],
            history: CircBuf::default(),
        }
    }

    fn cluster_history() -> ClusterHistory {
        ClusterHistory {
            struct_version: 0,
            bump: 0,
            _padding0: [0; 7],
            cluster_history_last_update_slot: 0,
            _padding1: [0; 232],
            history: CircBufCluster::default(),
        }
    }

    /// Epochs 10 and 11 are tower, 12 is the migration epoch, and 13 and 14 are alpenglow. The
    /// validator votes in every reward certificate and leads one block per epoch.
    fn migrating_history() -> (ValidatorHistory, ClusterHistory) {
        let mut validator = validator_history();
        let mut cluster = cluster_history();
        for epoch in 10..=14u16 {
            let epoch_credits_uncapped = match epoch {
                10 | 11 => 1_000 * u64::from(TVC_MULTIPLIER),
                // Tower credits for part of the epoch, plus alpenglow lamports for the rest
                12 => 500 * u64::from(TVC_MULTIPLIER) + reward_lamports(500, 1, 1.),
                _ => reward_lamports(1_000, 1, 1.),
            };
            validator.history.push(ValidatorHistoryEntry {
                epoch,
                epoch_credits: epoch_credits_uncapped.min(u64::from(MAX_EPOCH_CREDITS)) as u32,
                epoch_credits_uncapped,
                epoch_stake_lamports: REWARD_STAKE,
                ..ValidatorHistoryEntry::default()
            });
            cluster.history.push(ClusterHistoryEntry {
                epoch,
                total_blocks: TOTAL_BLOCKS,
                total_epoch_stake_lamports: TOTAL_REWARD_STAKE,
                total_inflation_rewards: INFLATION_REWARDS,
                distributed_inflation_rewards: distributed_rewards(1.),
                is_alpenglow: (epoch >= 13) as u8,
                ..ClusterHistoryEntry::default()
            });
        }
        (validator, cluster)
    }

    #[test]
    fn test_alpenglow_epoch_credits_counts_votes() {
        // The validator holds 0.1% of the stake, so it is expected to lead 1 of the 1_000 blocks
        assert_eq!(
            credits(reward_lamports(1_000, 1, 0.95), 0.95),
            Some(1_000 * TVC_MULTIPLIER)
        );
        assert_eq!(
            credits(reward_lamports(800, 1, 0.95), 0.95),
            Some(800 * TVC_MULTIPLIER)
        );
    }

    #[test]
    fn test_alpenglow_epoch_credits_charges_the_expected_leader_share() {
        // Leader rewards are indistinguishable from vote rewards in the lamport total, so the
        // stake-weighted expectation is deducted no matter how many blocks the validator led. Each
        // block pays its leader 950 votes' worth here, so leader luck moves the credits.
        assert_eq!(
            credits(reward_lamports(1_000, 0, 0.95), 0.95),
            Some(50 * TVC_MULTIPLIER)
        );
        assert_eq!(
            credits(reward_lamports(1_000, 3, 0.95), 0.95),
            Some(2_900 * TVC_MULTIPLIER)
        );
    }

    #[test]
    fn test_alpenglow_epoch_credits_floors_at_zero() {
        // Earned less than the leader share it is expected to have been paid
        assert_eq!(credits(0, 0.95), Some(0));
    }

    #[test]
    fn test_alpenglow_epoch_credits_needs_stake_and_rewards() {
        let distributed = distributed_rewards(0.95);
        assert_eq!(
            alpenglow_epoch_credits(
                0,
                0,
                TOTAL_REWARD_STAKE,
                INFLATION_REWARDS,
                distributed,
                SLOTS_PER_EPOCH
            ),
            None
        );
        assert_eq!(
            alpenglow_epoch_credits(
                0,
                REWARD_STAKE,
                TOTAL_REWARD_STAKE,
                0,
                distributed,
                SLOTS_PER_EPOCH
            ),
            None
        );
    }

    #[test]
    fn test_epoch_credits_range_across_migration() {
        let (validator, cluster) = migrating_history();
        assert_eq!(
            epoch_credits_range(&validator, &cluster, 10, 14, 0, SLOTS_PER_EPOCH),
            vec![
                EpochCredits::Scored(Some(1_000 * TVC_MULTIPLIER)),
                EpochCredits::Scored(Some(1_000 * TVC_MULTIPLIER)),
                EpochCredits::Unscorable,
                EpochCredits::Scored(Some(1_000 * TVC_MULTIPLIER)),
                EpochCredits::Scored(Some(1_000 * TVC_MULTIPLIER)),
            ]
        );
    }

    #[test]
    fn test_epoch_credits_range_alpenglow_inputs() {
        let (mut validator, mut cluster) = migrating_history();

        // Falls back to the stake oracle when the stake snapshot is missing
        let entry = entry_mut(&mut validator, 12);
        entry.epoch_stake_lamports = u64::MAX;
        entry.activated_stake_lamports = REWARD_STAKE;
        // Epoch 14 can't be scored without the total stake that paid it
        let cluster_entry = cluster
            .history
            .arr_mut()
            .iter_mut()
            .find(|entry| entry.epoch == 13)
            .unwrap();
        cluster_entry.total_epoch_stake_lamports = u64::MAX;

        assert_eq!(
            epoch_credits_range(&validator, &cluster, 13, 14, 0, SLOTS_PER_EPOCH),
            vec![
                EpochCredits::Scored(Some(1_000 * TVC_MULTIPLIER)),
                EpochCredits::Unscorable
            ]
        );

        // Copied before uncapped credits were recorded
        entry_mut(&mut validator, 13).epoch_credits_uncapped = u64::MAX;
        assert_eq!(
            epoch_credits_range(&validator, &cluster, 13, 13, 0, SLOTS_PER_EPOCH),
            vec![EpochCredits::Unscorable]
        );

        // Earned nothing
        entry_mut(&mut validator, 13).epoch_credits = u32::MAX;
        assert_eq!(
            epoch_credits_range(&validator, &cluster, 13, 13, 0, SLOTS_PER_EPOCH),
            vec![EpochCredits::Scored(None)]
        );
    }

    #[test]
    fn test_scorable_epoch_credits_matches_tower_scoring() {
        let credits = [Some(16_000), None, Some(8_000), Some(16_000)];
        let total_blocks = [Some(1_000), Some(1_000), None, Some(1_000)];
        assert_eq!(
            calculate_scorable_epoch_credits(
                &credits.map(EpochCredits::Scored),
                &total_blocks,
                100,
                0.9
            )
            .unwrap(),
            calculate_epoch_credits(&credits, &total_blocks, 100, 0.9).unwrap()
        );
    }

    #[test]
    fn test_scorable_epoch_credits_skips_unscorable_epochs() {
        let window = [
            EpochCredits::Scored(Some(16_000)),
            EpochCredits::Unscorable,
            EpochCredits::Scored(Some(16_000)),
        ];
        let (vote_credits_ratio, delinquency_score, _, delinquency_epoch) =
            calculate_scorable_epoch_credits(&window, &[Some(1_000); 3], 100, 0.97).unwrap();
        assert_eq!(vote_credits_ratio, 1.);
        assert_eq!(delinquency_score, 1);
        assert_eq!(delinquency_epoch, EPOCH_DEFAULT);

        let window = [
            EpochCredits::Unscorable,
            EpochCredits::Scored(Some(16_000)),
            EpochCredits::Scored(Some(0)),
        ];
        let (_, delinquency_score, delinquency_ratio, delinquency_epoch) =
            calculate_scorable_epoch_credits(&window, &[Some(1_000); 3], 100, 0.97).unwrap();
        assert_eq!(delinquency_score, 0);
        assert_eq!(delinquency_ratio, 0.);
        assert_eq!(delinquency_epoch, 102);

        assert_eq!(
            calculate_scorable_epoch_credits(
                &[EpochCredits::Unscorable; 3],
                &[Some(1_000); 3],
                100,
                0.97
            )
            .unwrap(),
            (0., 1, 1., EPOCH_DEFAULT)
        );
    }

    #[test]
    fn test_alpenglow_instant_unstake_delinquency() {
        let (mut validator, cluster) = migrating_history();
        assert_eq!(
            calculate_alpenglow_instant_unstake_delinquency(
                &validator,
                &cluster,
                13,
                0,
                SLOTS_PER_EPOCH,
                0.7
            ),
            (false, 1_000 * TVC_MULTIPLIER, TOTAL_BLOCKS)
        );

        // Voted in half of the reward certificates
        entry_mut(&mut validator, 13).epoch_credits_uncapped = reward_lamports(500, 1, 1.);
        assert_eq!(
            calculate_alpenglow_instant_unstake_delinquency(
                &validator,
                &cluster,
                13,
                0,
                SLOTS_PER_EPOCH,
                0.7
            ),
            (true, 500 * TVC_MULTIPLIER, TOTAL_BLOCKS)
        );

        // The migration epoch can't be scored, so it's no reason to unstake
        assert_eq!(
            calculate_alpenglow_instant_unstake_delinquency(
                &validator,
                &cluster,
                12,
                0,
                SLOTS_PER_EPOCH,
                0.7
            ),
            (false, 0, TOTAL_BLOCKS)
        );
    }

    #[test]
    fn test_large_alpenglow_credits_do_not_overflow() {
        let epoch_credits = [Some(u32::MAX - 1); 30];
        let total_blocks = [Some(1000); 30];
        let (vote_credits_ratio, delinquency_score, _, _) =
            calculate_epoch_credits(&epoch_credits, &total_blocks, 0, 0.97).unwrap();
        assert_eq!(delinquency_score, 1);
        assert!(vote_credits_ratio > 1.0);
    }

    #[test]
    fn test_saturated_alpenglow_credits() {
        let result =
            calculate_instant_unstake_delinquency(1000, 1000, MAX_EPOCH_CREDITS, 1000, 0.7)
                .unwrap();
        assert!(!result);

        let result = calculate_instant_unstake_delinquency(1000, 1000, 0, 1000, 0.7).unwrap();
        assert!(result);
    }
}
