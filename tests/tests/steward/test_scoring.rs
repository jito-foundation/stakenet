use jito_steward::{score::*, Config, LargeBitMask, Parameters};
use solana_sdk::pubkey::Pubkey;
use validator_history::{CircBuf, CircBufCluster, ClusterHistory, ValidatorHistory};

// Fixtures

fn create_config(
    mev_commission_bps_threshold: u16,
    commission_threshold: u8,
    historical_commission_threshold: u8,
) -> Config {
    Config {
        parameters: Parameters {
            mev_commission_bps_threshold,
            commission_threshold,
            historical_commission_threshold,
            mev_commission_range: 10,
            epoch_credits_range: 10,
            commission_range: 10,
            scoring_delinquency_threshold_ratio: 0.9,
            instant_unstake_delinquency_threshold_ratio: 0.8,
            ..Default::default()
        },
        stake_pool: Pubkey::new_unique(),
        validator_list: Pubkey::new_unique(),
        admin: Pubkey::new_unique(),
        parameters_authority: Pubkey::new_unique(),
        blacklist_authority: Pubkey::new_unique(),
        validator_history_blacklist: LargeBitMask::default(),
        paused: false.into(),
        _padding: [0; 1023],
    }
}

fn create_validator_history(
    mev_commissions: &[u16],
    commissions: &[u8],
    epoch_credits: &[u32],
    superminority: &[u8],
) -> ValidatorHistory {
    let mut history = CircBuf::default();
    for (i, (((&mev, &comm), &credits), &super_min)) in mev_commissions
        .iter()
        .zip(commissions)
        .zip(epoch_credits)
        .zip(superminority)
        .enumerate()
    {
        history.push(validator_history::ValidatorHistoryEntry {
            epoch: i as u16,
            mev_commission: mev,
            commission: comm,
            epoch_credits: credits,
            is_superminority: super_min,
            ..Default::default()
        });
    }
    ValidatorHistory {
        history,
        struct_version: 0,
        vote_account: Pubkey::new_unique(),
        index: 0,
        bump: 0,
        _padding0: [0; 7],
        last_ip_timestamp: 0,
        last_version_timestamp: 0,
        _padding1: [0; 232],
    }
}

#[allow(dead_code)]
fn create_cluster_history(total_blocks: &[u32]) -> ClusterHistory {
    let mut history = CircBufCluster::default();
    for (i, &blocks) in total_blocks.iter().enumerate() {
        history.push(validator_history::ClusterHistoryEntry {
            epoch: i as u16,
            total_blocks: blocks,
            ..Default::default()
        });
    }
    ClusterHistory {
        history,
        struct_version: 0,
        bump: 0,
        _padding0: [0; 7],
        cluster_history_last_update_slot: 0,
        _padding1: [0; 232],
    }
}

// Tests

mod test_calculate_mev_commission {
    use jito_steward::score::calculate_mev_commission;

    #[test]
    fn test_normal() {
        let mev_commissions = [100, 200, 300, 400, 500];
        let window = mev_commissions.iter().map(|&c| Some(c)).collect::<Vec<_>>();
        let current_epoch = 4;
        let threshold = 300;

        let (score, max_commission, max_epoch, running_jito) =
            calculate_mev_commission(&window, current_epoch, threshold).unwrap();

        assert_eq!(score, 0.0);
        assert_eq!(max_commission, 500);
        assert_eq!(max_epoch, 4);
        assert_eq!(running_jito, 1.0);
    }

    #[test]
    fn test_edge_cases() {
        // All commissions below threshold
        let window = [Some(100), Some(200), Some(250)];
        let (score, max_commission, max_epoch, running_jito) =
            calculate_mev_commission(&window, 2, 300).unwrap();
        assert_eq!(score, 1.0);
        assert_eq!(max_commission, 250);
        assert_eq!(max_epoch, 2);
        assert_eq!(running_jito, 1.0);

        // Empty window
        let window: [Option<u16>; 0] = [];
        let (score, max_commission, max_epoch, running_jito) =
            calculate_mev_commission(&window, 0, 300).unwrap();
        assert_eq!(score, 1.0);
        assert_eq!(max_commission, 10000);
        assert_eq!(max_epoch, 0);
        assert_eq!(running_jito, 0.0);

        // Window with Some(0) values
        let window = [Some(0), Some(0), Some(0)];
        let (score, max_commission, max_epoch, running_jito) =
            calculate_mev_commission(&window, 2, 300).unwrap();
        assert_eq!(score, 1.0);
        assert_eq!(max_commission, 0);
        assert_eq!(max_epoch, 2);
        assert_eq!(running_jito, 1.0);
    }
}

mod test_calculate_epoch_credits {
    use super::*;

    #[test]
    fn test_normal() {
        let epoch_credits = [Some(800), Some(900), Some(1000)];
        let total_blocks = [Some(1000), Some(1000), Some(1000)];
        let epoch_start = 0;
        let threshold = 0.9;

        let (ratio, delinquency_score, delinquency_ratio, delinquency_epoch) =
            calculate_epoch_credits(&epoch_credits, &total_blocks, epoch_start, threshold).unwrap();

        assert_eq!(ratio, 0.9);
        assert_eq!(delinquency_score, 1.0);
        assert_eq!(delinquency_ratio, 1.0);
        assert_eq!(delinquency_epoch, 65535);
    }

    #[test]
    fn test_edge_cases() {
        // Delinquency detected
        let epoch_credits = [Some(700), Some(800), Some(850)];
        let total_blocks = [Some(1000), Some(1000), Some(1000)];
        let (_ratio, delinquency_score, delinquency_ratio, delinquency_epoch) =
            calculate_epoch_credits(&epoch_credits, &total_blocks, 0, 0.9).unwrap();
        assert_eq!(delinquency_score, 0.0);
        assert_eq!(delinquency_ratio, 0.7);
        assert_eq!(delinquency_epoch, 0);

        // Missing data
        let epoch_credits = [None, Some(800), Some(900)];
        let total_blocks = [Some(1000), None, Some(1000)];
        let (ratio, delinquency_score, _delinquency_ratio, _delinquency_epoch) =
            calculate_epoch_credits(&epoch_credits, &total_blocks, 0, 0.9).unwrap();
        assert_eq!(ratio, 0.85);
        assert_eq!(delinquency_score, 1.0);

        // Empty windows
        let epoch_credits: [Option<u32>; 0] = [];
        let total_blocks: [Option<u32>; 0] = [];
        let result = calculate_epoch_credits(&epoch_credits, &total_blocks, 0, 0.9);
        assert!(result.is_err());
    }
}

mod test_calculate_commission {
    use super::*;

    #[test]
    fn test_normal() {
        let commission_window = [Some(5), Some(7), Some(6)];
        let current_epoch = 2;
        let threshold = 8;

        let (score, max_commission, max_epoch) =
            calculate_commission(&commission_window, current_epoch, threshold).unwrap();

        assert_eq!(score, 1.0);
        assert_eq!(max_commission, 7);
        assert_eq!(max_epoch, 1);
    }

    #[test]
    fn test_edge_cases() {
        // Commission above threshold
        let commission_window = [Some(5), Some(10), Some(6)];
        let (score, max_commission, max_epoch) =
            calculate_commission(&commission_window, 2, 8).unwrap();
        assert_eq!(score, 0.0);
        assert_eq!(max_commission, 10);
        assert_eq!(max_epoch, 1);

        // Empty window
        let commission_window: [Option<u8>; 0] = [];
        let (score, max_commission, max_epoch) =
            calculate_commission(&commission_window, 0, 8).unwrap();
        assert_eq!(score, 1.0);
        assert_eq!(max_commission, 0);
        assert_eq!(max_epoch, 0);

        // Window with None values
        let commission_window = [None, Some(5), None];
        let (score, max_commission, max_epoch) =
            calculate_commission(&commission_window, 2, 8).unwrap();
        assert_eq!(score, 1.0);
        assert_eq!(max_commission, 5);
        assert_eq!(max_epoch, 1);
    }
}

mod test_calculate_historical_commission {
    use super::*;

    #[test]
    fn test_normal() {
        let validator = create_validator_history(
            &[100; 10],
            &[5, 6, 7, 8, 7, 6, 5, 4, 3, 2],
            &[1000; 10],
            &[0; 10],
        );
        let current_epoch = 9;
        let threshold = 8;

        let (score, max_commission, max_epoch) =
            calculate_historical_commission(&validator, current_epoch, threshold).unwrap();

        assert_eq!(score, 1.0);
        assert_eq!(max_commission, 8);
        assert_eq!(max_epoch, 3);
    }

    #[test]
    fn test_edge_cases() {
        // Commission above threshold
        let validator = create_validator_history(
            &[100; 10],
            &[5, 6, 7, 9, 7, 6, 5, 4, 3, 2],
            &[1000; 10],
            &[0; 10],
        );
        let (score, max_commission, max_epoch) =
            calculate_historical_commission(&validator, 9, 8).unwrap();
        assert_eq!(score, 0.0);
        assert_eq!(max_commission, 9);
        assert_eq!(max_epoch, 3);

        // Empty history
        let validator = create_validator_history(&[], &[], &[], &[]);
        let result = calculate_historical_commission(&validator, 0, 8);
        assert!(result.is_err());

        // History with None values
        let mut validator = create_validator_history(
            &[100; 10],
            &[5, 6, 7, 8, 7, 6, 5, 4, 3, 2],
            &[1000; 10],
            &[0; 10],
        );
        validator
            .history
            .push(validator_history::ValidatorHistoryEntry::default());
        let (score, max_commission, max_epoch) =
            calculate_historical_commission(&validator, 10, 8).unwrap();
        assert_eq!(score, 1.0);
        assert_eq!(max_commission, 8);
        assert_eq!(max_epoch, 3);
    }
}

mod test_calculate_superminority {
    use super::*;

    #[test]
    fn test_normal() {
        let validator = create_validator_history(
            &[100; 10],
            &[5; 10],
            &[1000; 10],
            &[0, 0, 0, 1, 0, 0, 0, 0, 0, 0],
        );
        let current_epoch = 9;
        let commission_range = 10;

        let (score, epoch) =
            calculate_superminority(&validator, current_epoch, commission_range).unwrap();

        assert_eq!(score, 1.0);
        assert_eq!(epoch, 65535);
    }

    #[test]
    fn test_edge_cases() {
        // Superminority detected
        let validator = create_validator_history(
            &[100; 10],
            &[5; 10],
            &[1000; 10],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        );
        let (score, epoch) = calculate_superminority(&validator, 9, 10).unwrap();
        assert_eq!(score, 0.0);
        assert_eq!(epoch, 9);

        // Empty history
        let validator = create_validator_history(&[], &[], &[], &[]);
        let result = calculate_superminority(&validator, 0, 10);
        assert!(result.is_err());

        // History with None values
        let mut validator = create_validator_history(&[100; 10], &[5; 10], &[1000; 10], &[0; 10]);
        validator
            .history
            .push(validator_history::ValidatorHistoryEntry::default());
        let (score, epoch) = calculate_superminority(&validator, 10, 10).unwrap();
        assert_eq!(score, 1.0);
        assert_eq!(epoch, 65535);
    }
}

mod test_calculate_blacklist {
    use super::*;

    #[test]
    fn test_normal() {
        let mut config = create_config(300, 8, 10);
        config.validator_history_blacklist.set(5, true).unwrap();

        let score = calculate_blacklist(&config, 5).unwrap();
        assert_eq!(score, 0.0);

        let score = calculate_blacklist(&config, 6).unwrap();
        assert_eq!(score, 1.0);
    }

    #[test]
    fn test_edge_cases() {
        let config = create_config(300, 8, 10);

        // Index out of bounds
        let result = calculate_blacklist(&config, u32::MAX);
        assert!(result.is_err());

        // Empty blacklist
        let score = calculate_blacklist(&config, 0).unwrap();
        assert_eq!(score, 1.0);
    }
}

mod test_calculate_instant_unstake_delinquency {
    use super::*;

    #[test]
    fn test_normal() {
        let total_blocks_latest = 1000;
        let cluster_history_slot_index = 1000;
        let epoch_credits_latest = 900;
        let validator_history_slot_index = 1000;
        let threshold = 0.8;

        let result = calculate_instant_unstake_delinquency(
            total_blocks_latest,
            cluster_history_slot_index,
            epoch_credits_latest,
            validator_history_slot_index,
            threshold,
        );

        assert_eq!(result, false);
    }

    #[test]
    fn test_edge_cases() {
        // Delinquency detected
        let result = calculate_instant_unstake_delinquency(1000, 1000, 700, 1000, 0.8);
        assert_eq!(result, true);

        // Zero blocks produced
        let result = calculate_instant_unstake_delinquency(0, 1000, 900, 1000, 0.8);
        assert_eq!(result, false);

        // Zero slots
        let result = calculate_instant_unstake_delinquency(1000, 0, 900, 1000, 0.8);
        assert_eq!(result, false);
    }
}

mod test_calculate_instant_unstake_mev_commission {
    use super::*;

    #[test]
    fn test_normal() {
        let validator =
            create_validator_history(&[100, 200, 300, 400, 500], &[5; 5], &[1000; 5], &[0; 5]);
        let current_epoch = 4;
        let threshold = 300;

        let (check, commission) =
            calculate_instant_unstake_mev_commission(&validator, current_epoch, threshold);

        assert_eq!(check, true);
        assert_eq!(commission, 500);
    }

    #[test]
    fn test_edge_cases() {
        // MEV commission below threshold
        let validator =
            create_validator_history(&[100, 200, 300, 200, 100], &[5; 5], &[1000; 5], &[0; 5]);
        let current_epoch = 4;
        let threshold = 300;

        let (check, commission) =
            calculate_instant_unstake_mev_commission(&validator, current_epoch, threshold);

        assert_eq!(check, false);
        assert_eq!(commission, 200);

        // No MEV commission data
        let validator = create_validator_history(
            &[u16::MAX, u16::MAX, u16::MAX, u16::MAX, u16::MAX],
            &[5; 5],
            &[1000; 5],
            &[0; 5],
        );
        let (check, commission) =
            calculate_instant_unstake_mev_commission(&validator, current_epoch, threshold);

        assert_eq!(check, false);
        assert_eq!(commission, 0);

        // Only one epoch of data
        let validator = create_validator_history(
            &[u16::MAX, u16::MAX, u16::MAX, u16::MAX, 400],
            &[5; 5],
            &[1000; 5],
            &[0; 5],
        );
        let (check, commission) =
            calculate_instant_unstake_mev_commission(&validator, current_epoch, threshold);

        assert_eq!(check, true);
        assert_eq!(commission, 400);

        // Threshold at exactly the highest commission
        let validator =
            create_validator_history(&[100, 200, 300, 400, 500], &[5; 5], &[1000; 5], &[0; 5]);
        let threshold = 500;

        let (check, commission) =
            calculate_instant_unstake_mev_commission(&validator, current_epoch, threshold);

        assert_eq!(check, false);
        assert_eq!(commission, 500);
    }
}

mod test_calculate_instant_unstake_commission {
    use jito_steward::constants::COMMISSION_MAX;

    use super::*;

    #[test]
    fn test_normal() {
        let validator = create_validator_history(&[5; 5], &[1, 2, 3, 4, 5], &[1000; 5], &[0; 5]);
        let threshold = 4;

        let (check, commission) = calculate_instant_unstake_commission(&validator, threshold);

        assert_eq!(check, true);
        assert_eq!(commission, 5);
    }

    #[test]
    fn test_edge_cases() {
        // Commission at threshold
        let validator = create_validator_history(&[5; 5], &[1, 2, 3, 4, 5], &[1000; 5], &[0; 5]);
        let threshold = 5;

        let (check, commission) = calculate_instant_unstake_commission(&validator, threshold);

        assert_eq!(check, false);
        assert_eq!(commission, 5);

        // No commission data
        let validator = create_validator_history(&[5; 5], &[u8::MAX; 5], &[1000; 5], &[0; 5]);
        let threshold = 5;

        let (check, commission) = calculate_instant_unstake_commission(&validator, threshold);

        assert_eq!(check, true);
        assert_eq!(commission, COMMISSION_MAX);

        // Only one epoch of data
        let validator = create_validator_history(
            &[u16::MAX, u16::MAX, u16::MAX, u16::MAX, 5],
            &[u8::MAX, u8::MAX, u8::MAX, u8::MAX, 3],
            &[u32::MAX, u32::MAX, u32::MAX, u32::MAX, 1000],
            &[u8::MAX, u8::MAX, u8::MAX, u8::MAX, 1],
        );
        let threshold = 5;

        let (check, commission) = calculate_instant_unstake_commission(&validator, threshold);

        assert_eq!(check, false);
        assert_eq!(commission, 3);
    }
}

mod test_calculate_instant_unstake_blacklist {
    use jito_steward::constants::MAX_VALIDATORS;

    use super::*;

    #[test]
    fn test_normal() {
        let mut config = create_config(300, 8, 10);
        config.validator_history_blacklist.set(5, true).unwrap();

        let result = calculate_instant_unstake_blacklist(&config, 5).unwrap();
        assert_eq!(result, true);

        let result = calculate_instant_unstake_blacklist(&config, 6).unwrap();
        assert_eq!(result, false);
    }

    #[test]
    fn test_edge_cases() {
        let mut config = create_config(300, 8, 10);

        // Index out of bounds
        let result = calculate_instant_unstake_blacklist(&config, MAX_VALIDATORS as u32);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), false);

        // Empty blacklist
        let result = calculate_instant_unstake_blacklist(&config, 0).unwrap();
        assert_eq!(result, false);

        // All blacklisted
        for i in 0..MAX_VALIDATORS {
            config.validator_history_blacklist.set(i, true).unwrap();
        }
        let result = calculate_instant_unstake_blacklist(&config, 0).unwrap();
        assert_eq!(result, true);
    }
}
