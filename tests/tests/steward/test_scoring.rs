use jito_steward::{score::*, Config, LargeBitMask, Parameters};
use solana_sdk::pubkey::Pubkey;
use validator_history::{CircBuf, ValidatorHistory};

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
        tip_router_upload_auth_epoch_cutoff: 0.into(),
        _padding: [0; 1021],
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

        // All commissions below threshold, and epoch is first instance of max commission
        let window = [Some(100), Some(200), Some(250), Some(250)];
        let (score, max_commission, max_epoch, running_jito) =
            calculate_mev_commission(&window, 3, 300).unwrap();
        assert_eq!(score, 1.0);
        assert_eq!(max_commission, 250);
        assert_eq!(max_epoch, 2);
        assert_eq!(running_jito, 1.0);

        // Window with Nones
        let window = [None, None, None];
        let (score, max_commission, max_epoch, running_jito) =
            calculate_mev_commission(&window, 2, 300).unwrap();
        assert_eq!(score, 0.0);
        assert_eq!(max_commission, 10000);
        assert_eq!(max_epoch, 2);
        assert_eq!(running_jito, 0.0);
    }

    #[test]
    fn test_edge_cases() {
        // Empty window
        let window: [Option<u16>; 0] = [];
        let (score, max_commission, max_epoch, running_jito) =
            calculate_mev_commission(&window, 0, 300).unwrap();
        assert_eq!(score, 0.0);
        assert_eq!(max_commission, 10000);
        assert_eq!(max_epoch, 0);
        assert_eq!(running_jito, 0.0);

        // Test Arithmetic error
        let window = [Some(0), Some(0), Some(0)];
        let result = calculate_mev_commission(&window, 0, 300);
        assert!(result.is_err());
    }
}

mod test_calculate_epoch_credits {
    use jito_steward::constants::EPOCH_DEFAULT;
    use validator_history::constants::TVC_MULTIPLIER;

    use super::*;

    #[test]
    fn test_normal() {
        let epoch_credits = [
            Some(800 * TVC_MULTIPLIER),
            Some(800 * TVC_MULTIPLIER),
            Some(800 * TVC_MULTIPLIER),
        ];
        let total_blocks = [Some(1000), Some(1000), Some(1000)];
        let epoch_start = 0;
        let threshold = 0.9;

        let (ratio, delinquency_score, delinquency_ratio, delinquency_epoch) =
            calculate_epoch_credits(&epoch_credits, &total_blocks, epoch_start, threshold).unwrap();

        assert_eq!(ratio, 0.8);
        assert_eq!(delinquency_score, 0.0);
        assert_eq!(delinquency_ratio, 0.8);
        assert_eq!(delinquency_epoch, 0);
    }

    #[test]
    fn test_edge_cases() {
        // Delinquency detected
        let epoch_credits = [
            Some(700 * TVC_MULTIPLIER),
            Some(800 * TVC_MULTIPLIER),
            Some(850 * TVC_MULTIPLIER),
        ];
        let total_blocks = [Some(1000), Some(1000), Some(1000)];
        let (_ratio, delinquency_score, delinquency_ratio, delinquency_epoch) =
            calculate_epoch_credits(&epoch_credits, &total_blocks, 0, 0.9).unwrap();
        assert_eq!(delinquency_score, 0.0);
        assert_eq!(delinquency_ratio, 0.7);
        assert_eq!(delinquency_epoch, 0);

        // Missing data
        let epoch_credits = [None, Some(800 * TVC_MULTIPLIER), Some(900 * TVC_MULTIPLIER)];
        let total_blocks = [Some(1000), None, Some(1000)];
        let (ratio, delinquency_score, delinquency_ratio, delinquency_epoch) =
            calculate_epoch_credits(&epoch_credits, &total_blocks, 0, 0.9).unwrap();
        assert_eq!(ratio, 1700. / 3000.);
        assert_eq!(delinquency_score, 0.0);
        assert_eq!(delinquency_ratio, 0.0);
        assert_eq!(delinquency_epoch, 0);

        // No delinquent epochs
        let epoch_credits = [
            Some(800 * TVC_MULTIPLIER),
            Some(900 * TVC_MULTIPLIER),
            Some(1000 * TVC_MULTIPLIER),
        ];
        let total_blocks = [Some(1000), Some(1000), Some(1000)];
        let (ratio, delinquency_score, delinquency_ratio, delinquency_epoch) =
            calculate_epoch_credits(&epoch_credits, &total_blocks, 0, 0.7).unwrap();
        assert_eq!(ratio, 0.9);
        assert_eq!(delinquency_score, 1.0);
        assert_eq!(delinquency_ratio, 1.0);
        assert_eq!(delinquency_epoch, EPOCH_DEFAULT);

        // Empty windows
        let epoch_credits: [Option<u32>; 0] = [];
        let total_blocks: [Option<u32>; 0] = [];
        let result = calculate_epoch_credits(&epoch_credits, &total_blocks, 0, 0.9);
        assert!(result.is_err());

        // Test Arithmetic error
        let epoch_credits = [Some(TVC_MULTIPLIER), Some(0)];
        let total_blocks = [Some(1), Some(1)];
        let result = calculate_epoch_credits(&epoch_credits, &total_blocks, u16::MAX, 0.9);
        assert!(result.is_err());

        // Test all blocks none error
        let epoch_credits = [Some(1), Some(1)];
        let total_blocks = [None, None];
        let result = calculate_epoch_credits(&epoch_credits, &total_blocks, u16::MAX, 0.9);
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

        // Commission above threshold
        let commission_window = [Some(5), Some(10), Some(6)];
        let (score, max_commission, max_epoch) =
            calculate_commission(&commission_window, 2, 8).unwrap();
        assert_eq!(score, 0.0);
        assert_eq!(max_commission, 10);
        assert_eq!(max_epoch, 1);
    }

    #[test]
    fn test_edge_cases() {
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

        // Test Arithmetic error
        let commission_window = [Some(0), Some(0), Some(0)];
        let result = calculate_commission(&commission_window, 0, 8);
        assert!(result.is_err());
    }
}

mod test_calculate_historical_commission {
    use jito_steward::constants::VALIDATOR_HISTORY_FIRST_RELIABLE_EPOCH;

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
    }

    #[test]
    fn test_edge_cases() {
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

        // Test all commissions none
        let validator = create_validator_history(&[100; 10], &[u8::MAX; 10], &[1000; 10], &[0; 10]);
        let (score, max_comission, max_epoch) =
            calculate_historical_commission(&validator, 1, 8).unwrap();
        assert_eq!(score, 1.0);
        assert_eq!(max_comission, 0);
        assert_eq!(max_epoch, VALIDATOR_HISTORY_FIRST_RELIABLE_EPOCH as u16);
    }
}

mod test_calculate_superminority {
    use jito_steward::constants::EPOCH_DEFAULT;

    use super::*;

    #[test]
    fn test_normal() {
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
        assert_eq!(epoch, EPOCH_DEFAULT);

        // Superminority with missed uploads after epoch 3
        let validator = create_validator_history(
            &[100; 6],
            &[5; 6],
            &[u32::MAX; 6],
            &[0, 0, 0, 1, u8::MAX, u8::MAX],
        );
        let current_epoch = 5;
        let commission_range = 4;
        let (score, epoch) =
            calculate_superminority(&validator, current_epoch, commission_range).unwrap();
        assert_eq!(score, 0.0);
        assert_eq!(epoch, 3);

        // Superminority with missed uploads after epoch 3
        let validator = create_validator_history(
            &[100; 6],
            &[5; 6],
            &[u32::MAX; 6],
            &[0, 0, 0, 0, u8::MAX, u8::MAX],
        );
        let current_epoch = 5;
        let commission_range = 4;
        let (score, epoch) =
            calculate_superminority(&validator, current_epoch, commission_range).unwrap();
        assert_eq!(score, 1.0);
        assert_eq!(epoch, EPOCH_DEFAULT);
    }

    #[test]
    fn test_edge_cases() {
        // Empty history
        let validator = create_validator_history(&[], &[], &[], &[]);
        let result = calculate_superminority(&validator, 0, 10);
        assert!(result.is_err());

        // Arithmetic error
        let validator = create_validator_history(&[100; 10], &[5; 10], &[u32::MAX; 10], &[0; 10]);
        let result = calculate_superminority(&validator, 0, 1);
        assert!(result.is_err());

        // History with None values
        let mut validator = create_validator_history(&[100; 10], &[5; 10], &[1000; 10], &[0; 10]);
        validator
            .history
            .push(validator_history::ValidatorHistoryEntry::default());
        let (score, epoch) = calculate_superminority(&validator, 10, 10).unwrap();
        assert_eq!(score, 1.0);
        assert_eq!(epoch, EPOCH_DEFAULT);
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
    }
}

mod test_calculate_merkle_root_authoirty {
    use validator_history::{MerkleRootUploadAuthority, ValidatorHistoryEntry};

    use super::*;

    #[test]
    fn test_normal() {
        let mut validator = create_validator_history(
            &[100; 10],
            &[5; 10],
            &[1000; 10],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        );
        let mut config = create_config(300, 8, 10);
        config.tip_router_upload_auth_epoch_cutoff = 800.into();
        let mut current_epoch = 3;

        // When using MerkleRootUploadAuthority::Other it should be a 0 score always
        validator.history.push(ValidatorHistoryEntry {
            merkle_root_upload_authority: MerkleRootUploadAuthority::Other,
            ..Default::default()
        });
        let score = calculate_merkle_root_authoirty(&validator, &config, current_epoch).unwrap();
        assert_eq!(score, 0.0);

        // MerkleRootUploadAuthority::OldJitoLabs returns score of 1 **prior** to config switch
        validator.history.push(ValidatorHistoryEntry {
            merkle_root_upload_authority: MerkleRootUploadAuthority::OldJitoLabs,
            ..Default::default()
        });
        let score = calculate_merkle_root_authoirty(&validator, &config, current_epoch).unwrap();
        assert_eq!(score, 1.0);
        // MerkleRootUploadAuthority::TipRouter returns score of 1 always
        validator.history.push(ValidatorHistoryEntry {
            merkle_root_upload_authority: MerkleRootUploadAuthority::TipRouter,
            ..Default::default()
        });
        let score = calculate_merkle_root_authoirty(&validator, &config, current_epoch).unwrap();
        assert_eq!(score, 1.0);

        // Test after TipRouter only config switch
        current_epoch = 800;
        // When using MerkleRootUploadAuthority::Other it should be a 0 score always
        validator.history.push(ValidatorHistoryEntry {
            merkle_root_upload_authority: MerkleRootUploadAuthority::Other,
            ..Default::default()
        });
        let score = calculate_merkle_root_authoirty(&validator, &config, current_epoch).unwrap();
        assert_eq!(score, 0.0);

        // MerkleRootUploadAuthority::OldJitoLabs returns score of 1 **prior** to config switch
        validator.history.push(ValidatorHistoryEntry {
            merkle_root_upload_authority: MerkleRootUploadAuthority::OldJitoLabs,
            ..Default::default()
        });
        let score = calculate_merkle_root_authoirty(&validator, &config, current_epoch).unwrap();
        assert_eq!(score, 0.0);

        // MerkleRootUploadAuthority::TipRouter returns score of 1 always
        validator.history.push(ValidatorHistoryEntry {
            merkle_root_upload_authority: MerkleRootUploadAuthority::TipRouter,
            ..Default::default()
        });
        let score = calculate_merkle_root_authoirty(&validator, &config, current_epoch).unwrap();
        assert_eq!(score, 1.0);
    }

    #[test]
    fn test_edge_cases() {
        // Empty history
        let validator = create_validator_history(&[], &[], &[], &[]);
        let mut config = create_config(300, 8, 10);
        config.tip_router_upload_auth_epoch_cutoff = 800.into();
        let current_epoch = 800;
        let score = calculate_merkle_root_authoirty(&validator, &config, current_epoch).unwrap();
        assert_eq!(score, 0.0);
    }
}

mod test_calculate_instant_unstake_delinquency {
    use validator_history::constants::TVC_MULTIPLIER;

    use super::*;

    #[test]
    fn test_normal() {
        let total_blocks_latest = 1000;
        let cluster_history_slot_index = 1000;
        let epoch_credits_latest = 900 * TVC_MULTIPLIER;
        let validator_history_slot_index = 1000;
        let threshold = 0.8;

        let result = calculate_instant_unstake_delinquency(
            total_blocks_latest,
            cluster_history_slot_index,
            epoch_credits_latest,
            validator_history_slot_index,
            threshold,
        )
        .unwrap();

        assert!(!result);

        // Delinquency detected
        let result = calculate_instant_unstake_delinquency(1000, 1000, 700, 1000, 0.8).unwrap();
        assert!(result);
    }

    #[test]
    fn test_edge_cases() {
        let total_blocks_latest = 0;
        let cluster_history_slot_index = 1000;
        let epoch_credits_latest = 900 * TVC_MULTIPLIER;
        let validator_history_slot_index = 1000;
        let threshold = 0.8;

        // Zero blocks produced
        let result = calculate_instant_unstake_delinquency(
            total_blocks_latest,
            cluster_history_slot_index,
            epoch_credits_latest,
            validator_history_slot_index,
            threshold,
        )
        .unwrap();
        assert!(!result);

        // Zero slots
        let total_blocks_latest = 1000;
        let cluster_history_slot_index = 0;
        let result = calculate_instant_unstake_delinquency(
            total_blocks_latest,
            cluster_history_slot_index,
            epoch_credits_latest,
            validator_history_slot_index,
            threshold,
        );
        assert!(result.is_err());
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

        assert!(check);
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

        assert!(!check);
        assert_eq!(commission, 200);

        // No MEV commission data
        let validator = create_validator_history(&[u16::MAX], &[5], &[1000], &[0]);
        let (check, commission) =
            calculate_instant_unstake_mev_commission(&validator, 0, threshold);

        assert!(!check);
        assert_eq!(commission, 0);

        // Only one epoch of data
        let validator = create_validator_history(
            &[u16::MAX, 400],
            &[u8::MAX, 5],
            &[u32::MAX, 1000],
            &[u8::MAX, 0],
        );
        let (check, commission) =
            calculate_instant_unstake_mev_commission(&validator, 1, threshold);

        assert!(check);
        assert_eq!(commission, 400);

        // Threshold at exactly the highest commission
        let validator =
            create_validator_history(&[100, 200, 300, 400, 500], &[5; 5], &[1000; 5], &[0; 5]);
        let threshold = 500;

        let (check, commission) =
            calculate_instant_unstake_mev_commission(&validator, 4, threshold);

        assert!(!check);
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

        assert!(check);
        assert_eq!(commission, 5);
    }

    #[test]
    fn test_edge_cases() {
        // Commission at threshold
        let validator = create_validator_history(&[5; 5], &[1, 2, 3, 4, 5], &[1000; 5], &[0; 5]);
        let threshold = 5;

        let (check, commission) = calculate_instant_unstake_commission(&validator, threshold);

        assert!(!check);
        assert_eq!(commission, 5);

        // No commission data
        let validator = create_validator_history(&[5; 5], &[u8::MAX; 5], &[1000; 5], &[0; 5]);
        let threshold = 5;

        let (check, commission) = calculate_instant_unstake_commission(&validator, threshold);

        assert!(check);
        assert_eq!(commission, COMMISSION_MAX);

        // Only one epoch of data
        let validator = create_validator_history(
            &[u16::MAX, 5],
            &[u8::MAX, 3],
            &[u32::MAX, 1000],
            &[u8::MAX, 1],
        );
        let threshold = 5;

        let (check, commission) = calculate_instant_unstake_commission(&validator, threshold);

        assert!(!check);
        assert_eq!(commission, 3);
    }
}

mod test_calculate_instant_unstake_blacklist {

    use super::*;

    #[test]
    fn test_normal() {
        let mut config = create_config(300, 8, 10);
        config.validator_history_blacklist.set(5, true).unwrap();

        let result = calculate_instant_unstake_blacklist(&config, 5).unwrap();
        assert!(result);

        let result = calculate_instant_unstake_blacklist(&config, 6).unwrap();
        assert!(!result);
    }

    /* single line fn, no edge cases */
}

mod test_calculate_instant_unstake_merkle_root_upload_auth {
    use validator_history::{MerkleRootUploadAuthority, ValidatorHistoryEntry};

    use super::*;

    #[test]
    fn test_normal() {
        let mut validator = create_validator_history(
            &[100; 10],
            &[5; 10],
            &[1000; 10],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        );
        let mut config = create_config(300, 8, 10);
        config.tip_router_upload_auth_epoch_cutoff = 800.into();
        let mut current_epoch = 3;

        // When using MerkleRootUploadAuthority::Other it should be a 0 score always
        validator.history.push(ValidatorHistoryEntry {
            merkle_root_upload_authority: MerkleRootUploadAuthority::Other,
            ..Default::default()
        });
        let is_instant_unstake =
            calculate_instant_unstake_merkle_root_upload_auth(&validator, &config, current_epoch)
                .unwrap();
        assert!(is_instant_unstake);

        // MerkleRootUploadAuthority::OldJitoLabs should not instant unstake prior to config switch
        validator.history.push(ValidatorHistoryEntry {
            merkle_root_upload_authority: MerkleRootUploadAuthority::OldJitoLabs,
            ..Default::default()
        });
        let is_instant_unstake =
            calculate_instant_unstake_merkle_root_upload_auth(&validator, &config, current_epoch)
                .unwrap();
        assert!(!is_instant_unstake);
        // MerkleRootUploadAuthority::TipRouter should never instant unstake
        validator.history.push(ValidatorHistoryEntry {
            merkle_root_upload_authority: MerkleRootUploadAuthority::TipRouter,
            ..Default::default()
        });
        let is_instant_unstake =
            calculate_instant_unstake_merkle_root_upload_auth(&validator, &config, current_epoch)
                .unwrap();
        assert!(!is_instant_unstake);

        // Test after TipRouter only config switch
        current_epoch = 800;
        // When using MerkleRootUploadAuthority::Other should instant unstake
        validator.history.push(ValidatorHistoryEntry {
            merkle_root_upload_authority: MerkleRootUploadAuthority::Other,
            ..Default::default()
        });
        let is_instant_unstake =
            calculate_instant_unstake_merkle_root_upload_auth(&validator, &config, current_epoch)
                .unwrap();
        assert!(is_instant_unstake);

        // MerkleRootUploadAuthority::OldJitoLabs should instant unstake **after** config switch
        validator.history.push(ValidatorHistoryEntry {
            merkle_root_upload_authority: MerkleRootUploadAuthority::OldJitoLabs,
            ..Default::default()
        });
        let is_instant_unstake =
            calculate_instant_unstake_merkle_root_upload_auth(&validator, &config, current_epoch)
                .unwrap();
        assert!(is_instant_unstake);

        // MerkleRootUploadAuthority::TipRouter should never instant unstake
        validator.history.push(ValidatorHistoryEntry {
            merkle_root_upload_authority: MerkleRootUploadAuthority::TipRouter,
            ..Default::default()
        });
        let is_instant_unstake =
            calculate_instant_unstake_merkle_root_upload_auth(&validator, &config, current_epoch)
                .unwrap();
        assert!(!is_instant_unstake);
    }

    #[test]
    fn test_edge_cases() {
        // Empty history
        let validator = create_validator_history(&[], &[], &[], &[]);
        let mut config = create_config(300, 8, 10);
        config.tip_router_upload_auth_epoch_cutoff = 800.into();
        let current_epoch = 800;
        let is_instant_unstake =
            calculate_instant_unstake_merkle_root_upload_auth(&validator, &config, current_epoch)
                .unwrap();
        assert!(is_instant_unstake);
    }
}
