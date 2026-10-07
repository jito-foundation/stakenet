use anchor_lang::Result;
use validator_history::{
    constants::TVC_MULTIPLIER, ClusterHistory, EpochCreditsRatio, ValidatorHistory,
};

use crate::{errors::StewardError, Parameters};

/// Whether the validator should be instantly unstaked for delinquency.
///
/// - A complete alpenglow epoch is judged by the share of its expected inflation the validator
///   captured, taken from the previous epoch because alpenglow rewards aren't paid until an epoch
///   ends.
/// - A tower epoch is judged by the current epoch's vote credit rate.
/// - The migration epoch is judged by neither, because its credits mix the two eras and can't be
///   compared against either denominator. See [`Parameters::is_alpenglow_transition_epoch`].
///
/// The epoch right after the migration needs no special case: it holds pure reward lamports, and
/// it is judged by the migration epoch, which is unscorable, so nothing is unstaked.
///
/// Every other instant unstake trigger still applies while delinquency is skipped.
#[allow(clippy::too_many_arguments)]
pub fn calculate_instant_unstake_delinquency(
    validator: &ValidatorHistory,
    cluster: &ClusterHistory,
    params: &Parameters,
    current_epoch: u16,
    tvc_activation_epoch: u64,
    slots_per_epoch: u64,
    total_blocks_latest: u32,
    cluster_history_slot_index: u64,
    epoch_credits_latest: u32,
    validator_history_slot_index: u64,
) -> Result<bool> {
    let instant_unstake_delinquency_threshold_ratio =
        params.instant_unstake_delinquency_threshold_ratio;

    if params.is_alpenglow_transition_epoch(current_epoch) {
        return Ok(false);
    }

    let Some(previous_epoch) = current_epoch.checked_sub(1) else {
        return Ok(false);
    };

    if params.is_alpenglow_epoch(current_epoch) {
        let epoch_credits_ratio = validator.history.epoch_credits_ratio_range(
            cluster,
            previous_epoch,
            previous_epoch,
            tvc_activation_epoch,
            slots_per_epoch,
            params.alpenglow_migration_epoch(),
        );

        Ok(match epoch_credits_ratio.first() {
            Some(&EpochCreditsRatio::Scored(ratio)) => {
                ratio < instant_unstake_delinquency_threshold_ratio
            }
            _ => false,
        })
    } else {
        calculate_instant_unstake_tower_delinquency(
            total_blocks_latest,
            cluster_history_slot_index,
            epoch_credits_latest,
            validator_history_slot_index,
            instant_unstake_delinquency_threshold_ratio,
        )
    }
}

/// Compares the validator's vote credit rate against the cluster's block rate so far this epoch.
/// Only valid while the cluster is on tower.
pub fn calculate_instant_unstake_tower_delinquency(
    total_blocks_latest: u32,
    cluster_history_slot_index: u64,
    epoch_credits_latest: u32,
    validator_history_slot_index: u64,
    instant_unstake_delinquency_threshold_ratio: f64,
) -> Result<bool> {
    if cluster_history_slot_index == 0 || validator_history_slot_index == 0 {
        return Err(StewardError::ArithmeticError.into());
    }

    let blocks_produced_rate =
        u64::from(total_blocks_latest) as f64 / cluster_history_slot_index as f64;
    let vote_credits_rate =
        u64::from(epoch_credits_latest) as f64 / validator_history_slot_index as f64;

    if blocks_produced_rate > 0. {
        Ok(
            (vote_credits_rate / (blocks_produced_rate * u64::from(TVC_MULTIPLIER) as f64))
                < instant_unstake_delinquency_threshold_ratio,
        )
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use anchor_lang::solana_program::pubkey::Pubkey;
    use bytemuck::Zeroable;
    use validator_history::{
        utils::MAX_EPOCH_CREDITS, CircBuf, CircBufCluster, ClusterHistoryEntry,
        ValidatorHistoryEntry,
    };

    use super::*;

    const SLOTS_PER_EPOCH: u64 = 432_000;
    const TOTAL_BLOCKS: u32 = 1_000;
    const THRESHOLD: f64 = 0.7;

    // A validator holding 0.1% of the stake is expected to earn 0.1% of the epoch's inflation
    const REWARD_STAKE: u64 = 1_000_000_000_000;
    const TOTAL_REWARD_STAKE: u64 = 1_000 * REWARD_STAKE;
    const INFLATION_REWARDS: u64 = 86_400_000_000_000;
    const EXPECTED_LAMPORTS: u64 = INFLATION_REWARDS / 1_000;

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

    /// The cluster migrates in epoch 10, so epochs 10 and 11 are the transition epochs and 12
    /// onward are alpenglow. The validator earns `tower_credits` in tower epochs and its full
    /// expected share in alpenglow ones.
    const MIGRATION_EPOCH: u16 = 10;

    fn migrating_history(tower_credits: u32) -> (ValidatorHistory, ClusterHistory) {
        let mut validator = validator_history();
        let mut cluster = cluster_history();
        for epoch in 8..=14u16 {
            let is_alpenglow = epoch > MIGRATION_EPOCH;
            validator.history.push(ValidatorHistoryEntry {
                epoch,
                epoch_credits: if is_alpenglow {
                    EXPECTED_LAMPORTS.min(u64::from(u32::MAX - 1)) as u32
                } else {
                    tower_credits
                },
                reward_lamports: if is_alpenglow {
                    EXPECTED_LAMPORTS
                } else {
                    u64::from(tower_credits)
                },
                epoch_stake_lamports: REWARD_STAKE,
                vote_account_last_update_slot: SLOTS_PER_EPOCH / 2,
                ..ValidatorHistoryEntry::default()
            });
            cluster.history.push(ClusterHistoryEntry {
                epoch,
                total_blocks: TOTAL_BLOCKS,
                total_epoch_stake_lamports: TOTAL_REWARD_STAKE,
                total_inflation_rewards: INFLATION_REWARDS,
                is_alpenglow: if is_alpenglow {
                    1
                } else {
                    ClusterHistoryEntry::default().is_alpenglow
                },
                ..ClusterHistoryEntry::default()
            });
        }
        (validator, cluster)
    }

    /// Clears the era of every epoch at or after `current_epoch`, modelling the oracle's real
    /// publishing lag: an epoch's era is only written once its rewards are paid, during the epoch
    /// after it, so the current epoch's flag is always unset.
    fn apply_oracle_lag(cluster: &mut ClusterHistory, current_epoch: u16) {
        for entry in cluster
            .history
            .arr_mut()
            .iter_mut()
            .filter(|entry| entry.epoch >= current_epoch)
        {
            entry.is_alpenglow = ClusterHistoryEntry::default().is_alpenglow;
        }
    }

    /// Parameters declaring the migration at `alpenglow_migration_epoch`, or `u16::MAX` for a
    /// cluster that hasn't migrated
    fn params(alpenglow_migration_epoch: u16) -> Parameters {
        let mut params = Parameters::zeroed();
        params.instant_unstake_delinquency_threshold_ratio = THRESHOLD;
        params.alpenglow_migration_epoch = alpenglow_migration_epoch;
        params
    }

    fn check(
        validator: &ValidatorHistory,
        cluster: &ClusterHistory,
        current_epoch: u16,
    ) -> Result<bool> {
        check_with(validator, cluster, &params(MIGRATION_EPOCH), current_epoch)
    }

    fn check_with(
        validator: &ValidatorHistory,
        cluster: &ClusterHistory,
        params: &Parameters,
        current_epoch: u16,
    ) -> Result<bool> {
        let epoch_credits_latest = validator
            .history
            .epoch_range(current_epoch, current_epoch)
            .first()
            .and_then(|entry| *entry)
            .map(|entry| entry.epoch_credits)
            .filter(|credits| *credits != ValidatorHistoryEntry::default().epoch_credits)
            .unwrap_or(0);

        calculate_instant_unstake_delinquency(
            validator,
            cluster,
            params,
            current_epoch,
            0,
            SLOTS_PER_EPOCH,
            TOTAL_BLOCKS,
            SLOTS_PER_EPOCH / 2,
            epoch_credits_latest,
            SLOTS_PER_EPOCH / 2,
        )
    }

    #[test]
    fn test_alpenglow_epoch_judged_by_previous_epoch() {
        let (validator, cluster) = migrating_history(TOTAL_BLOCKS * TVC_MULTIPLIER);
        assert!(!check(&validator, &cluster, 14).unwrap());
    }

    #[test]
    fn test_alpenglow_epoch_unstakes_when_under_threshold() {
        let (mut validator, cluster) = migrating_history(TOTAL_BLOCKS * TVC_MULTIPLIER);
        validator
            .history
            .arr_mut()
            .iter_mut()
            .find(|entry| entry.epoch == 13)
            .unwrap()
            .reward_lamports = EXPECTED_LAMPORTS / 2;
        assert!(check(&validator, &cluster, 14).unwrap());
    }

    #[test]
    fn test_migration_epoch_is_not_judged() {
        // The migration epoch mixes the two eras' credits, so neither measure applies — not even
        // for a validator that earned nothing
        let (validator, cluster) = migrating_history(TOTAL_BLOCKS * TVC_MULTIPLIER);
        assert!(!check(&validator, &cluster, MIGRATION_EPOCH).unwrap());

        let (validator, cluster) = migrating_history(0);
        assert!(!check(&validator, &cluster, MIGRATION_EPOCH).unwrap());
    }

    #[test]
    fn test_first_full_alpenglow_epoch_is_judged_by_the_migration_epoch() {
        // The epoch after the migration holds pure reward lamports, so it is measured normally.
        // It is judged by the migration epoch, which is unscorable, so nothing is unstaked.
        let (validator, cluster) = migrating_history(0);
        assert!(!check(&validator, &cluster, MIGRATION_EPOCH + 1).unwrap());
    }

    #[test]
    fn test_tower_epoch_uses_the_current_epoch() {
        let (validator, cluster) = migrating_history(TOTAL_BLOCKS * TVC_MULTIPLIER);
        assert!(!check(&validator, &cluster, 9).unwrap());

        let (validator, cluster) = migrating_history(0);
        assert!(check(&validator, &cluster, 9).unwrap());
    }

    #[test]
    fn test_missing_previous_epoch_is_not_judged() {
        let (validator, cluster) = migrating_history(TOTAL_BLOCKS * TVC_MULTIPLIER);
        assert!(!check(&validator, &cluster, 8).unwrap());
    }

    #[test]
    fn test_alpenglow_epoch_judged_before_its_era_is_recorded() {
        let (mut validator, mut cluster) = migrating_history(TOTAL_BLOCKS * TVC_MULTIPLIER);
        apply_oracle_lag(&mut cluster, 13);
        validator
            .history
            .arr_mut()
            .iter_mut()
            .find(|entry| entry.epoch == 13)
            .unwrap()
            .reward_lamports = EXPECTED_LAMPORTS / 2;

        assert!(check(&validator, &cluster, 14).unwrap());
    }

    #[test]
    fn test_cluster_that_never_migrates_is_still_judged() {
        // With no migration declared every epoch is tower, so epoch 9's own vote credits decide
        let (validator, cluster) = migrating_history(0);
        assert!(check_with(&validator, &cluster, &params(u16::MAX), 9).unwrap());

        let (validator, cluster) = migrating_history(TOTAL_BLOCKS * TVC_MULTIPLIER);
        assert!(!check_with(&validator, &cluster, &params(u16::MAX), 9).unwrap());
    }

    #[test]
    fn test_tower_delinquency_compares_vote_rate_to_block_rate() {
        assert!(!calculate_instant_unstake_tower_delinquency(
            1_000,
            SLOTS_PER_EPOCH,
            1_000 * TVC_MULTIPLIER,
            SLOTS_PER_EPOCH,
            THRESHOLD
        )
        .unwrap());

        assert!(!calculate_instant_unstake_tower_delinquency(
            1_000,
            SLOTS_PER_EPOCH,
            900 * TVC_MULTIPLIER,
            SLOTS_PER_EPOCH,
            THRESHOLD
        )
        .unwrap());

        assert!(calculate_instant_unstake_tower_delinquency(
            1_000,
            SLOTS_PER_EPOCH,
            500 * TVC_MULTIPLIER,
            SLOTS_PER_EPOCH,
            THRESHOLD
        )
        .unwrap());

        assert!(!calculate_instant_unstake_tower_delinquency(
            1_000,
            SLOTS_PER_EPOCH,
            MAX_EPOCH_CREDITS,
            SLOTS_PER_EPOCH,
            THRESHOLD
        )
        .unwrap());
    }

    #[test]
    fn test_tower_delinquency_edge_cases() {
        assert!(!calculate_instant_unstake_tower_delinquency(
            0,
            SLOTS_PER_EPOCH,
            1_000 * TVC_MULTIPLIER,
            SLOTS_PER_EPOCH,
            THRESHOLD
        )
        .unwrap());

        assert!(calculate_instant_unstake_tower_delinquency(
            1_000,
            SLOTS_PER_EPOCH,
            0,
            SLOTS_PER_EPOCH,
            THRESHOLD
        )
        .unwrap());

        assert!(calculate_instant_unstake_tower_delinquency(
            1_000,
            0,
            0,
            SLOTS_PER_EPOCH,
            THRESHOLD
        )
        .is_err());
        assert!(calculate_instant_unstake_tower_delinquency(
            1_000,
            SLOTS_PER_EPOCH,
            0,
            0,
            THRESHOLD
        )
        .is_err());
    }
}
