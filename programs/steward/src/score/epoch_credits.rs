use anchor_lang::Result;
use validator_history::EpochCreditsRatio;

use crate::{constants::EPOCH_DEFAULT, errors::StewardError, Parameters};

/// Averages the participation ratios of the scorable epochs in the window, and flags the first one
/// that falls below the delinquency threshold.
///
/// Epochs are left out when they can't be measured: their inputs are missing, or they fall in the
/// alpenglow transition where credits mix the two eras. A validator isn't judged on those.
pub fn calculate_scorable_epoch_credits(
    epoch_credits_ratio_window: &[EpochCreditsRatio],
    params: &Parameters,
    epoch_credits_start: u16,
) -> Result<(f64, u8, f64, u16)> {
    let scoring_delinquency_threshold_ratio = params.scoring_delinquency_threshold_ratio;

    let mut scored = Vec::with_capacity(epoch_credits_ratio_window.len());
    for (i, participation) in epoch_credits_ratio_window.iter().enumerate() {
        if let EpochCreditsRatio::Scored(ratio) = *participation {
            let epoch = epoch_credits_start
                .checked_add(i as u16)
                .ok_or(StewardError::ArithmeticError)?;
            scored.push((epoch, ratio));
        }
    }

    if scored.is_empty() {
        return Ok((0., 1, 1., EPOCH_DEFAULT));
    }

    // Delinquency heuristic - not actual delinquency
    let mut delinquency_score = 1u8;
    let mut delinquency_ratio = 1.0;
    let mut delinquency_epoch = EPOCH_DEFAULT;
    for &(epoch, ratio) in scored.iter() {
        if ratio < scoring_delinquency_threshold_ratio {
            delinquency_score = 0;
            delinquency_ratio = ratio;
            delinquency_epoch = epoch;
            break;
        }
    }

    let average_ratio = scored.iter().map(|&(_, ratio)| ratio).sum::<f64>() / scored.len() as f64;

    Ok((
        average_ratio,
        delinquency_score,
        delinquency_ratio,
        delinquency_epoch,
    ))
}

#[cfg(test)]
mod tests {
    use anchor_lang::solana_program::pubkey::Pubkey;
    use bytemuck::Zeroable;
    use validator_history::{
        CircBuf, CircBufCluster, ClusterHistory, ClusterHistoryEntry, ValidatorHistory,
        ValidatorHistoryEntry,
    };

    use super::*;

    /// Parameters with no alpenglow migration declared, so no epoch is a transition epoch
    fn params(scoring_delinquency_threshold_ratio: f64) -> Parameters {
        let mut params = Parameters::zeroed();
        params.scoring_delinquency_threshold_ratio = scoring_delinquency_threshold_ratio;
        params.alpenglow_migration_epoch = u16::MAX;
        params
    }

    #[test]
    fn test_scorable_epoch_credits_averages_scored_epochs() {
        let window = [
            EpochCreditsRatio::Scored(1.),
            EpochCreditsRatio::Scored(0.5),
            EpochCreditsRatio::Scored(0.9),
        ];
        let (average_ratio, delinquency_score, delinquency_ratio, delinquency_epoch) =
            calculate_scorable_epoch_credits(&window, &params(0.4), 100).unwrap();
        assert!((average_ratio - 0.8).abs() < 1e-9);
        assert_eq!(delinquency_score, 1);
        assert_eq!(delinquency_ratio, 1.);
        assert_eq!(delinquency_epoch, EPOCH_DEFAULT);
    }

    #[test]
    fn test_scorable_epoch_credits_flags_first_delinquent_epoch() {
        let window = [
            EpochCreditsRatio::Scored(1.),
            EpochCreditsRatio::Scored(0.5),
            EpochCreditsRatio::Scored(0.2),
        ];
        let (_, delinquency_score, delinquency_ratio, delinquency_epoch) =
            calculate_scorable_epoch_credits(&window, &params(0.97), 100).unwrap();
        assert_eq!(delinquency_score, 0);
        assert_eq!(delinquency_ratio, 0.5);
        assert_eq!(delinquency_epoch, 101);
    }

    #[test]
    fn test_scorable_epoch_credits_skips_unscorable_epochs() {
        let window = [
            EpochCreditsRatio::Scored(1.),
            EpochCreditsRatio::Unscorable,
            EpochCreditsRatio::Scored(1.),
        ];
        let (average_ratio, delinquency_score, _, delinquency_epoch) =
            calculate_scorable_epoch_credits(&window, &params(0.97), 100).unwrap();
        assert_eq!(average_ratio, 1.);
        assert_eq!(delinquency_score, 1);
        assert_eq!(delinquency_epoch, EPOCH_DEFAULT);

        let window = [
            EpochCreditsRatio::Unscorable,
            EpochCreditsRatio::Scored(1.),
            EpochCreditsRatio::Scored(0.),
        ];
        let (_, delinquency_score, delinquency_ratio, delinquency_epoch) =
            calculate_scorable_epoch_credits(&window, &params(0.97), 100).unwrap();
        assert_eq!(delinquency_score, 0);
        assert_eq!(delinquency_ratio, 0.);
        assert_eq!(delinquency_epoch, 102);

        assert_eq!(
            calculate_scorable_epoch_credits(
                &[EpochCreditsRatio::Unscorable; 3],
                &params(0.97),
                100
            )
            .unwrap(),
            (0., 1, 1., EPOCH_DEFAULT)
        );
    }

    const SLOTS_PER_EPOCH: u64 = 432_000;
    const INFLATION_REWARDS: u64 = 86_400_000_000_000;
    const MIGRATION_EPOCH: u16 = 50;
    const FIRST_EPOCH: u16 = 100;
    const EPOCH_CREDITS_RANGE: u16 = 30;

    // A validator holding 0.01% of the stake: 43.2 leader slots and 8,640,000,000 lamports of
    // inflation on average each epoch
    const STAKE: u64 = 1_000_000_000_000;
    const TOTAL_STAKE: u64 = 10_000 * STAKE;

    /// What a validator holding `STAKE` earns voting on `voted` of the epoch's slots and producing
    /// `blocks_produced` blocks, with each slot paying half to its voters and half to its leader
    fn earned(voted: f64, blocks_produced: u32) -> u64 {
        let voter =
            (INFLATION_REWARDS as f64 / 2. * STAKE as f64 / TOTAL_STAKE as f64 * voted) as u64;
        voter + INFLATION_REWARDS / 2 * u64::from(blocks_produced) / SLOTS_PER_EPOCH
    }

    /// A 30-epoch scoring window plus the epoch before it, all alpenglow. `epoch` returns the
    /// fraction of slots voted, leader slots scheduled and blocks produced for each epoch.
    fn alpenglow_history(
        epoch: impl Fn(u16) -> (f64, u32, u32),
        record_leader_slots: bool,
    ) -> (ValidatorHistory, ClusterHistory) {
        let mut validator = ValidatorHistory {
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
        };
        let mut cluster = ClusterHistory {
            struct_version: 0,
            bump: 0,
            _padding0: [0; 7],
            cluster_history_last_update_slot: 0,
            _padding1: [0; 232],
            history: CircBufCluster::default(),
        };
        for e in FIRST_EPOCH - 1..FIRST_EPOCH + EPOCH_CREDITS_RANGE {
            let (voted, leader_slots, blocks_produced) = epoch(e);
            let reward_lamports = earned(voted, blocks_produced);
            validator.history.push(ValidatorHistoryEntry {
                epoch: e,
                epoch_credits: reward_lamports.min(u64::from(u32::MAX - 1)) as u32,
                reward_lamports,
                epoch_stake_lamports: STAKE,
                total_leader_slots: if record_leader_slots {
                    leader_slots
                } else {
                    u32::MAX
                },
                blocks_produced,
                ..ValidatorHistoryEntry::default()
            });
            cluster.history.push(ClusterHistoryEntry {
                epoch: e,
                total_blocks: SLOTS_PER_EPOCH as u32,
                total_epoch_stake_lamports: TOTAL_STAKE,
                total_inflation_rewards: INFLATION_REWARDS,
                is_alpenglow: 1,
                ..ClusterHistoryEntry::default()
            });
        }
        (validator, cluster)
    }

    fn score(validator: &ValidatorHistory, cluster: &ClusterHistory) -> (f64, u8, f64, u16) {
        let window = validator.history.epoch_credits_ratio_range(
            cluster,
            FIRST_EPOCH,
            FIRST_EPOCH + EPOCH_CREDITS_RANGE - 1,
            0,
            SLOTS_PER_EPOCH,
            MIGRATION_EPOCH,
        );
        let mut params = params(0.9);
        params.alpenglow_migration_epoch = MIGRATION_EPOCH;
        calculate_scorable_epoch_credits(&window, &params, FIRST_EPOCH).unwrap()
    }

    #[test]
    fn test_small_validator_with_unlucky_schedule_is_not_delinquent() {
        // Votes on every slot and produces every block it leads, but draws 12 leader slots in
        // epoch 115 against an average of 43.2
        let schedule = |e: u16| match e {
            115 => (1., 12, 12),
            _ => (1., 43, 43),
        };

        let (validator, cluster) = alpenglow_history(schedule, false);
        let (_, delinquency_score, delinquency_ratio, delinquency_epoch) =
            score(&validator, &cluster);
        assert_eq!(delinquency_score, 0);
        assert_eq!(delinquency_epoch, 115);
        assert!(delinquency_ratio < 0.64);

        let (validator, cluster) = alpenglow_history(schedule, true);
        let (average_ratio, delinquency_score, _, delinquency_epoch) = score(&validator, &cluster);
        assert_eq!(delinquency_score, 1);
        assert_eq!(delinquency_epoch, EPOCH_DEFAULT);
        assert!((average_ratio - 1.).abs() < 1e-6);
    }

    #[test]
    fn test_poor_participation_is_still_delinquent() {
        // Votes on 70% of the slots in epoch 120 with a normal schedule
        let (validator, cluster) = alpenglow_history(
            |e| match e {
                120 => (0.7, 43, 43),
                _ => (1., 43, 43),
            },
            true,
        );
        let (_, delinquency_score, delinquency_ratio, delinquency_epoch) =
            score(&validator, &cluster);
        assert_eq!(delinquency_score, 0);
        assert_eq!(delinquency_epoch, 120);
        assert!((delinquency_ratio - 0.85).abs() < 0.01);

        // Skips most of its own leader slots in epoch 120, however lucky its schedule
        let (validator, cluster) = alpenglow_history(
            |e| match e {
                120 => (1., 86, 20),
                _ => (1., 43, 43),
            },
            true,
        );
        let (_, delinquency_score, delinquency_ratio, delinquency_epoch) =
            score(&validator, &cluster);
        assert_eq!(delinquency_score, 0);
        assert_eq!(delinquency_epoch, 120);
        assert!(delinquency_ratio < 0.9);
    }
}
