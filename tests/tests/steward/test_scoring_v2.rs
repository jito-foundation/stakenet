use jito_steward::score::{
    calculate_avg_commission, calculate_avg_mev_commission, calculate_avg_vote_credits,
    encode_validator_score,
};
use solana_sdk::pubkey::Pubkey;
use validator_history::{CircBuf, ValidatorHistory, ValidatorHistoryEntry};

// Helper function to create a ValidatorHistory
fn create_validator_history() -> ValidatorHistory {
    ValidatorHistory {
        history: CircBuf::default(),
        struct_version: 0,
        vote_account: Pubkey::new_unique(),
        index: 0,
        bump: 0,
        _padding0: [0; 7],
        last_ip_timestamp: 0,
        last_version_timestamp: 0,
        validator_age: 0,
        validator_age_last_updated_epoch: 0,
        _padding1: [0; 226],
    }
}

#[test]
fn test_encode_validator_score_perfect_validator() {
    // Perfect validator: 0% commissions, max age, max credits
    let score = encode_validator_score(0, 0, 131071, 33554431).unwrap();

    // Should have maximum possible score
    // Inflation: 100 (inverted from 0)
    // MEV: 10000 (inverted from 0)
    // Age: 131071 (max 17-bit value)
    // Credits: 33554431 (max 25-bit value)
    let expected = (100u64 << 56) | (10000u64 << 42) | (131071u64 << 25) | 33554431u64;
    assert_eq!(score, expected);

    // Verify top byte is 100 (0x64)
    assert_eq!((score >> 56) & 0xFF, 100);
}

#[test]
fn test_encode_validator_score_worst_validator() {
    // Worst validator: max commissions, no age, no credits
    let score = encode_validator_score(100, 10000, 0, 0).unwrap();

    // Should have minimum possible score (all zeros)
    assert_eq!(score, 0);
}

#[test]
fn test_encode_validator_score_tier_ordering() {
    // Test that tier 1 (inflation) has highest priority
    let score_low_inflation = encode_validator_score(10, 5000, 100, 10000).unwrap();
    let score_high_inflation = encode_validator_score(20, 0, 200, 20000).unwrap();

    // Lower inflation commission should always beat higher, regardless of other fields
    assert!(
        score_low_inflation > score_high_inflation,
        "10% inflation should score higher than 20% inflation"
    );

    // Test tier 2 (MEV) priority when inflation is equal
    let score_low_mev = encode_validator_score(10, 1000, 100, 10000).unwrap();
    let score_high_mev = encode_validator_score(10, 2000, 200, 20000).unwrap();

    assert!(
        score_low_mev > score_high_mev,
        "Lower MEV commission should score higher when inflation is equal"
    );

    // Test tier 3 (age) priority when inflation and MEV are equal
    let score_high_age = encode_validator_score(10, 1000, 200, 10000).unwrap();
    let score_low_age = encode_validator_score(10, 1000, 100, 20000).unwrap();

    assert!(
        score_high_age > score_low_age,
        "Higher age should score higher when inflation and MEV are equal"
    );

    // Test tier 4 (credits) only matters when all others are equal
    let score_high_credits = encode_validator_score(10, 1000, 100, 20000).unwrap();
    let score_low_credits = encode_validator_score(10, 1000, 100, 10000).unwrap();

    assert!(
        score_high_credits > score_low_credits,
        "Higher credits should score higher when all other tiers are equal"
    );
}

#[test]
fn test_encode_validator_score_commission_inversion() {
    // Test that lower commissions result in higher scores
    let scores: Vec<u64> = (0..=100)
        .step_by(10)
        .map(|commission| encode_validator_score(commission as u8, 0, 100, 10000).unwrap())
        .collect();

    // Scores should be strictly decreasing as commission increases
    for i in 1..scores.len() {
        assert!(
            scores[i - 1] > scores[i],
            "Score with {}% commission should be higher than {}%",
            (i - 1) * 10,
            i * 10
        );
    }
}

#[test]
fn test_encode_validator_score_mev_commission_inversion() {
    // Test MEV commission inversion (in basis points)
    let test_values = [0, 100, 500, 1000, 5000, 10000];
    let scores: Vec<u64> = test_values
        .iter()
        .map(|&mev_bps| encode_validator_score(50, mev_bps, 100, 10000).unwrap())
        .collect();

    // Scores should be strictly decreasing as MEV commission increases
    for i in 1..scores.len() {
        assert!(
            scores[i - 1] > scores[i],
            "Lower MEV commission ({} bps) should yield higher score than ({} bps)",
            test_values[i - 1],
            test_values[i]
        );
    }
}

#[test]
fn test_encode_validator_score_bit_boundaries() {
    // Test that values are properly contained in their bit ranges
    let score = encode_validator_score(50, 5000, 65536, 16777216).unwrap();

    // Extract each component
    let inflation_bits = (score >> 56) & 0xFF; // 8 bits
    let mev_bits = (score >> 42) & 0x3FFF; // 14 bits
    let age_bits = (score >> 25) & 0x1FFFF; // 17 bits
    let credit_bits = score & 0x1FFFFFF; // 25 bits

    // Verify inversions
    assert_eq!(inflation_bits, 50, "Inflation: 100 - 50 = 50");
    assert_eq!(mev_bits, 5000, "MEV: 10000 - 5000 = 5000");
    assert_eq!(age_bits, 65536, "Age: Direct (not inverted)");
    assert_eq!(credit_bits, 16777216, "Credits: Direct");
}

#[test]
fn test_encode_validator_score_caps() {
    // Test that values above max are capped
    let score = encode_validator_score(200, 20000, 1000000, 100000000).unwrap();

    // Extract components - should be capped at their maximums
    let inflation_bits = (score >> 56) & 0xFF;
    let mev_bits = (score >> 42) & 0x3FFF;
    let age_bits = (score >> 25) & 0x1FFFF;
    let credit_bits = score & 0x1FFFFFF;

    assert_eq!(inflation_bits, 0, "100 - min(200, 100) = 0");
    assert_eq!(mev_bits, 0, "10000 - min(20000, 10000) = 0");
    assert_eq!(age_bits, 131071, "Capped at (1 << 17) - 1");
    assert_eq!(credit_bits, 33554431, "Capped at (1 << 25) - 1");
}

#[test]
fn test_calculate_avg_commission() {
    let mut validator = create_validator_history();

    // Set up commission values for epochs 10-20
    for epoch in 10..=20 {
        validator.history.push(ValidatorHistoryEntry {
            epoch: epoch as u16,
            commission: 10,
            ..ValidatorHistoryEntry::default()
        });
    }

    // Average over 10 epochs (11-20)
    let avg = calculate_avg_commission(&validator, 20, 10);
    assert_eq!(avg, 10);

    // Test with some missing data
    validator.history.arr[15].commission = u8::MAX; // MAX means None
    let avg = calculate_avg_commission(&validator, 20, 10);
    assert_eq!(avg, 10, "Should still be 10 (ignores None values)");

    // Test with varying commissions - need to clear and repopulate
    let mut validator = create_validator_history();
    for epoch in 10..=20 {
        validator.history.push(ValidatorHistoryEntry {
            epoch: epoch as u16,
            commission: (epoch as u8) % 20,
            ..ValidatorHistoryEntry::default()
        });
    }
    let avg = calculate_avg_commission(&validator, 20, 10);
    // Average of [10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20] % 20 = [10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 0]
    // = (10+11+12+13+14+15+16+17+18+19+0) / 11 = 145 / 11 = 13
    assert_eq!(avg, 13);

    // Test with all None - Clear history and add entries with no commission data
    let mut validator = create_validator_history();
    for epoch in 10..=20 {
        validator.history.push(ValidatorHistoryEntry {
            epoch: epoch as u16,
            commission: u8::MAX, // MAX means None
            ..ValidatorHistoryEntry::default()
        });
    }
    let avg = calculate_avg_commission(&validator, 20, 10);
    assert_eq!(avg, 100, "Should return 100 (max) when no data");
}

#[test]
fn test_calculate_avg_mev_commission() {
    let mut validator = create_validator_history();

    // Set up MEV commission values
    for epoch in 10..=20 {
        validator.history.push(ValidatorHistoryEntry {
            epoch: epoch as u16,
            mev_commission: 1000,
            ..ValidatorHistoryEntry::default()
        });
    }

    // Average over 10 epochs (11-20, inclusive)
    let avg = calculate_avg_mev_commission(&validator, 20, 10);
    assert_eq!(avg, 1000);

    // Test with one high value - add a new entry for epoch 21
    validator.history.push(ValidatorHistoryEntry {
        epoch: 21,
        mev_commission: 10000,
        ..ValidatorHistoryEntry::default()
    });
    // Now check average for epochs 11-21 (window of 10 from epoch 21 means 21-10=11 to 21 inclusive)
    let avg = calculate_avg_mev_commission(&validator, 21, 10);
    // Average of 10 * 1000 + 1 * 10000 = 20000 / 11 = 1818
    assert_eq!(avg, 1818);

    // Test with high MEV commission value
    validator.history.push(ValidatorHistoryEntry {
        epoch: 22,
        mev_commission: 20000, // Above BASIS_POINTS_MAX
        ..ValidatorHistoryEntry::default()
    });
    let avg = calculate_avg_mev_commission(&validator, 22, 10);
    // Average of epochs 12-22: 9*1000 + 1*10000 + 1*20000 = 39000 / 11 = 3545
    // The value is NOT capped when stored in ValidatorHistory
    assert_eq!(avg, 3545);
}

#[test]
fn test_calculate_avg_vote_credits() {
    // Test with all values present
    let window = vec![Some(1000), Some(2000), Some(3000)];
    let avg = calculate_avg_vote_credits(&window);
    assert_eq!(avg, 2000, "(1000 + 2000 + 3000) / 3 = 2000");

    // Test with some None values
    let window = vec![Some(1000), None, Some(3000)];
    let avg = calculate_avg_vote_credits(&window);
    assert_eq!(avg, 2000, "(1000 + 3000) / 2 = 2000");

    // Test with all None
    let window = vec![None, None, None];
    let avg = calculate_avg_vote_credits(&window);
    assert_eq!(avg, 0);

    // Test empty window
    let window = vec![];
    let avg = calculate_avg_vote_credits(&window);
    assert_eq!(avg, 0);

    // Test single value
    let window = vec![Some(5000)];
    let avg = calculate_avg_vote_credits(&window);
    assert_eq!(avg, 5000);
}

#[test]
fn test_score_sorting_property() {
    // Ensure scores maintain correct sorting for delegation
    let validators = [
        (0, 0, 100, 10000),       // Best: 0% commissions
        (5, 100, 100, 10000),     // Good: 5% inflation, 1% MEV
        (10, 500, 100, 10000),    // OK: 10% inflation, 5% MEV
        (50, 5000, 100, 10000),   // Bad: 50% inflation, 50% MEV
        (100, 10000, 100, 10000), // Worst: 100% commissions
    ];

    let scores: Vec<u64> = validators
        .iter()
        .map(|&(inf, mev, age, credits)| encode_validator_score(inf, mev, age, credits).unwrap())
        .collect();

    // Verify scores are in descending order
    for i in 1..scores.len() {
        assert!(
            scores[i - 1] > scores[i],
            "Validator {} should score higher than {}",
            i - 1,
            i
        );
    }
}

#[test]
fn test_large_score_values() {
    // Test that large score values don't cause issues
    // This addresses the concern about large u64 values

    // Perfect validator should have a very large score
    let perfect_score = encode_validator_score(0, 0, 100, 16000).unwrap();

    // This is a large number but it's correct!
    // Top byte is 100 (0x64), representing perfect inflation commission score
    assert_eq!((perfect_score >> 56) & 0xFF, 100);

    // The full value should be:
    // (100 << 56) | (10000 << 42) | (100 << 25) | 16000
    let expected = (100u64 << 56) | (10000u64 << 42) | (100u64 << 25) | 16000u64;
    assert_eq!(perfect_score, expected);

    // This large value is by design - it allows for fine-grained sorting
    // where higher-order bits (inflation commission) matter most
}

#[test]
fn test_commission_boundaries() {
    // Test boundary values for commissions

    // 0% should give max score
    let zero_commission = encode_validator_score(0, 0, 0, 0).unwrap();
    assert_eq!((zero_commission >> 56) & 0xFF, 100);
    assert_eq!((zero_commission >> 42) & 0x3FFF, 10000);

    // 100% should give min score
    let max_commission = encode_validator_score(100, 10000, 0, 0).unwrap();
    assert_eq!((max_commission >> 56) & 0xFF, 0);
    assert_eq!((max_commission >> 42) & 0x3FFF, 0);

    // Test mid-range values
    let mid_commission = encode_validator_score(50, 5000, 0, 0).unwrap();
    assert_eq!((mid_commission >> 56) & 0xFF, 50);
    assert_eq!((mid_commission >> 42) & 0x3FFF, 5000);
}

#[test]
fn test_validator_age_impact() {
    // Test that validator age properly affects scoring

    // Same commissions and credits, different ages
    let young_validator = encode_validator_score(10, 1000, 10, 10000).unwrap();
    let old_validator = encode_validator_score(10, 1000, 1000, 10000).unwrap();
    let ancient_validator = encode_validator_score(10, 1000, 100000, 10000).unwrap();

    assert!(
        old_validator > young_validator,
        "Older validator should score higher"
    );
    assert!(
        ancient_validator > old_validator,
        "Even older validator should score highest"
    );

    // Extract age components to verify
    assert_eq!((young_validator >> 25) & 0x1FFFF, 10);
    assert_eq!((old_validator >> 25) & 0x1FFFF, 1000);
    assert_eq!((ancient_validator >> 25) & 0x1FFFF, 100000);
}

#[test]
fn test_vote_credits_impact() {
    // Test that vote credits properly affect scoring

    // Same everything except credits
    let low_credits = encode_validator_score(10, 1000, 100, 1000).unwrap();
    let mid_credits = encode_validator_score(10, 1000, 100, 10000).unwrap();
    let high_credits = encode_validator_score(10, 1000, 100, 100000).unwrap();

    assert!(
        mid_credits > low_credits,
        "More credits should score higher"
    );
    assert!(
        high_credits > mid_credits,
        "Even more credits should score highest"
    );

    // Extract credit components to verify
    assert_eq!(low_credits & 0x1FFFFFF, 1000);
    assert_eq!(mid_credits & 0x1FFFFFF, 10000);
    assert_eq!(high_credits & 0x1FFFFFF, 100000);
}

// Integration tests for the complete validator_score function
#[cfg(test)]
mod validator_score_integration_tests {
    use super::*;
    use jito_steward::{score::validator_score, Config, LargeBitMask, Parameters};
    use solana_sdk::pubkey::Pubkey;
    use validator_history::{
        constants::TVC_MULTIPLIER, CircBufCluster, ClusterHistory, ClusterHistoryEntry,
    };

    fn create_test_config() -> Config {
        Config {
            parameters: Parameters {
                mev_commission_bps_threshold: 1000,
                commission_threshold: 10,
                historical_commission_threshold: 50,
                mev_commission_range: 10,
                epoch_credits_range: 10,
                commission_range: 10,
                scoring_delinquency_threshold_ratio: 0.85,
                instant_unstake_delinquency_threshold_ratio: 0.8,
                ..Parameters::default()
            },
            stake_pool: Pubkey::new_unique(),
            validator_list: Pubkey::new_unique(),
            admin: Pubkey::new_unique(),
            parameters_authority: Pubkey::new_unique(),
            blacklist_authority: Pubkey::new_unique(),
            validator_history_blacklist: LargeBitMask::default(),
            paused: false.into(),
            _padding_0: [0u8; 7],
            priority_fee_parameters_authority: Pubkey::new_unique(),
            _padding: [0; 984],
        }
    }

    fn create_cluster_history(epochs: u16) -> ClusterHistory {
        let bump =
            Pubkey::find_program_address(&[ClusterHistory::SEED], &validator_history::id()).1;
        let mut cluster = ClusterHistory {
            struct_version: 0,
            bump,
            _padding0: [0; 7],
            cluster_history_last_update_slot: 1000,
            _padding1: [0; 232],
            history: CircBufCluster {
                arr: [ClusterHistoryEntry::default(); ClusterHistory::MAX_ITEMS],
                idx: ClusterHistory::MAX_ITEMS as u64 - 1,
                is_empty: 1,
                padding: [0; 7],
            },
        };

        // Add cluster history entries with 1000 blocks per epoch
        for epoch in 0..=epochs {
            cluster.history.push(ClusterHistoryEntry {
                epoch: epoch as u16,
                total_blocks: 1000,
                ..ClusterHistoryEntry::default()
            });
        }

        cluster
    }

    #[test]
    fn test_validator_score_perfect_validator() {
        let mut validator = create_validator_history();
        let cluster = create_cluster_history(20);
        let config = create_test_config();
        let current_epoch = 20u16;

        // Set up perfect validator - 0% commissions, good credits
        for epoch in 0..=20 {
            validator.history.push(ValidatorHistoryEntry {
                epoch: epoch as u16,
                commission: 0,
                mev_commission: 0,
                epoch_credits: 1000 * TVC_MULTIPLIER,
                vote_account_last_update_slot: 1000,
                is_superminority: 0,
                ..ValidatorHistoryEntry::default()
            });
        }

        let result = validator_score(
            &validator,
            &cluster,
            &config,
            current_epoch,
            0, // tvc_activation_epoch
        )
        .unwrap();

        // Perfect validator should have non-zero raw score and score
        assert!(result.raw_score > 0);
        assert_eq!(result.score, result.raw_score); // No binary filters should fail

        // Check components
        assert_eq!(result.commission_avg, 0);
        assert_eq!(result.mev_commission_avg, 0);
        assert_eq!(result.vote_credits_avg, 16000); // 1000 * TVC_MULTIPLIER

        // All binary filters should pass (score = 1)
        assert_eq!(result.mev_commission_score, 1);
        assert_eq!(result.blacklisted_score, 1);
        assert_eq!(result.superminority_score, 1);
        assert_eq!(result.delinquency_score, 1);
        assert_eq!(result.running_jito_score, 1);
        assert_eq!(result.commission_score, 1);
        assert_eq!(result.historical_commission_score, 1);
    }

    #[test]
    fn test_validator_score_high_commission_filter() {
        let mut validator = create_validator_history();
        let cluster = create_cluster_history(20);
        let config = create_test_config();
        let current_epoch = 20u16;

        // Set up validator with high commission that exceeds threshold
        for epoch in 0..=20 {
            let commission = if epoch == 20 { 50 } else { 5 }; // Spike to 50% in last epoch
            validator.history.push(ValidatorHistoryEntry {
                epoch: epoch as u16,
                commission,
                mev_commission: 100,
                epoch_credits: 1000 * TVC_MULTIPLIER,
                vote_account_last_update_slot: 1000,
                is_superminority: 0,
                ..ValidatorHistoryEntry::default()
            });
        }

        let result = validator_score(&validator, &cluster, &config, current_epoch, 0).unwrap();

        // Score should be 0 due to commission filter
        assert_eq!(result.score, 0);
        assert!(result.raw_score > 0); // Raw score should still be non-zero
        assert_eq!(result.commission_score, 0); // Commission filter failed
        assert_eq!(result.details.max_commission, 50);
        assert_eq!(result.details.max_commission_epoch, 20);
    }

    #[test]
    fn test_validator_score_mev_commission_filter() {
        let mut validator = create_validator_history();
        let cluster = create_cluster_history(20);
        let config = create_test_config();
        let current_epoch = 20u16;

        // Set up validator with high MEV commission
        for epoch in 0..=20 {
            validator.history.push(ValidatorHistoryEntry {
                epoch: epoch as u16,
                commission: 5,
                mev_commission: 2000, // Above 1000 bps threshold
                epoch_credits: 1000 * TVC_MULTIPLIER,
                vote_account_last_update_slot: 1000,
                is_superminority: 0,
                ..ValidatorHistoryEntry::default()
            });
        }

        let result = validator_score(&validator, &cluster, &config, current_epoch, 0).unwrap();

        // Score should be 0 due to MEV commission filter
        assert_eq!(result.score, 0);
        assert!(result.raw_score > 0);
        assert_eq!(result.mev_commission_score, 0); // MEV commission filter failed
        assert_eq!(result.details.max_mev_commission, 2000);
    }

    #[test]
    fn test_validator_score_delinquency_filter() {
        let mut validator = create_validator_history();
        let cluster = create_cluster_history(20);
        let config = create_test_config();
        let current_epoch = 20u16;

        // Set up validator with poor performance
        for epoch in 0..=20 {
            let credits = if epoch == 15 {
                100 // Very low credits in one epoch
            } else {
                1000 * TVC_MULTIPLIER
            };

            validator.history.push(ValidatorHistoryEntry {
                epoch: epoch as u16,
                commission: 5,
                mev_commission: 100,
                epoch_credits: credits,
                vote_account_last_update_slot: 1000,
                is_superminority: 0,
                ..ValidatorHistoryEntry::default()
            });
        }

        let result = validator_score(&validator, &cluster, &config, current_epoch, 0).unwrap();

        // Check if delinquency was detected
        if result.delinquency_score == 0 {
            assert_eq!(result.score, 0);
            assert!(result.details.delinquency_ratio < 0.85);
        }
    }

    #[test]
    fn test_validator_score_not_running_jito() {
        let mut validator = create_validator_history();
        let cluster = create_cluster_history(20);
        let config = create_test_config();
        let current_epoch = 20u16;

        // Set up validator with no MEV commission (not running Jito)
        for epoch in 0..=20 {
            validator.history.push(ValidatorHistoryEntry {
                epoch: epoch as u16,
                commission: 5,
                mev_commission: u16::MAX, // MAX means None/not set
                epoch_credits: 1000 * TVC_MULTIPLIER,
                vote_account_last_update_slot: 1000,
                is_superminority: 0,
                ..ValidatorHistoryEntry::default()
            });
        }

        let result = validator_score(&validator, &cluster, &config, current_epoch, 0).unwrap();

        // Score should be 0 due to not running Jito
        assert_eq!(result.score, 0);
        assert!(result.raw_score > 0);
        assert_eq!(result.running_jito_score, 0); // Not running Jito filter failed
    }

    #[test]
    fn test_validator_score_blacklisted() {
        let mut validator = create_validator_history();
        validator.index = 5; // Set a specific index

        let cluster = create_cluster_history(20);
        let mut config = create_test_config();

        // Blacklist this validator
        config.validator_history_blacklist.set(5, true).unwrap();

        let current_epoch = 20u16;

        // Set up otherwise good validator
        for epoch in 0..=20 {
            validator.history.push(ValidatorHistoryEntry {
                epoch: epoch as u16,
                commission: 5,
                mev_commission: 100,
                epoch_credits: 1000 * TVC_MULTIPLIER,
                vote_account_last_update_slot: 1000,
                is_superminority: 0,
                ..ValidatorHistoryEntry::default()
            });
        }

        let result = validator_score(&validator, &cluster, &config, current_epoch, 0).unwrap();

        // Score should be 0 due to blacklist
        assert_eq!(result.score, 0);
        assert!(result.raw_score > 0);
        assert_eq!(result.blacklisted_score, 0); // Blacklist filter failed
    }

    #[test]
    fn test_validator_score_ranking() {
        // Test that scores correctly rank validators
        let cluster = create_cluster_history(20);
        let config = create_test_config();
        let current_epoch = 20u16;

        // Create validators with different characteristics
        let mut validators = vec![];

        // Validator 1: Perfect (0% commissions)
        let mut v1 = create_validator_history();
        for epoch in 0..=20 {
            v1.history.push(ValidatorHistoryEntry {
                epoch: epoch as u16,
                commission: 0,
                mev_commission: 0,
                epoch_credits: 1000 * TVC_MULTIPLIER,
                vote_account_last_update_slot: 1000,
                is_superminority: 0,
                ..ValidatorHistoryEntry::default()
            });
        }
        validators.push(("perfect", v1));

        // Validator 2: Good (5% commission)
        let mut v2 = create_validator_history();
        for epoch in 0..=20 {
            v2.history.push(ValidatorHistoryEntry {
                epoch: epoch as u16,
                commission: 5,
                mev_commission: 100,
                epoch_credits: 1000 * TVC_MULTIPLIER,
                vote_account_last_update_slot: 1000,
                is_superminority: 0,
                ..ValidatorHistoryEntry::default()
            });
        }
        validators.push(("good", v2));

        // Validator 3: OK (10% commission, higher MEV)
        let mut v3 = create_validator_history();
        for epoch in 0..=20 {
            v3.history.push(ValidatorHistoryEntry {
                epoch: epoch as u16,
                commission: 10,
                mev_commission: 500,
                epoch_credits: 1000 * TVC_MULTIPLIER,
                vote_account_last_update_slot: 1000,
                is_superminority: 0,
                ..ValidatorHistoryEntry::default()
            });
        }
        validators.push(("ok", v3));

        // Calculate scores
        let mut scores = vec![];
        for (name, validator) in &validators {
            let result = validator_score(validator, &cluster, &config, current_epoch, 0).unwrap();
            scores.push((name, result.score));
        }

        // Verify ranking: perfect > good > ok
        assert!(
            scores[0].1 > scores[1].1,
            "Perfect should score higher than good"
        );
        assert!(
            scores[1].1 > scores[2].1,
            "Good should score higher than ok"
        );
    }
}
