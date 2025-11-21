use jito_steward::{
    bitmask::BitMask,
    constants::{MAX_VALIDATORS, SORTED_INDEX_DEFAULT},
    directed_delegation::{decrease_stake_calculation, increase_stake_calculation, RebalanceType},
    state::directed_stake::{DirectedStakeMeta, DirectedStakeTarget},
    Delegation, StewardStateEnum, StewardStateV2 as StewardState,
};
use solana_sdk::pubkey::Pubkey;

/// Helper function to create a mock StewardState for testing
fn create_mock_steward_state(num_pool_validators: u16) -> StewardState {
    StewardState {
        state_tag: StewardStateEnum::ComputeScores,
        validator_lamport_balances: [0; MAX_VALIDATORS],
        scores: [0; MAX_VALIDATORS],
        sorted_score_indices: [SORTED_INDEX_DEFAULT; MAX_VALIDATORS],
        raw_scores: [0; MAX_VALIDATORS],
        sorted_raw_score_indices: [SORTED_INDEX_DEFAULT; MAX_VALIDATORS],
        delegations: [Delegation::default(); MAX_VALIDATORS],
        instant_unstake: BitMask::default(),
        progress: BitMask::default(),
        validators_to_remove: BitMask::default(),
        validators_for_immediate_removal: BitMask::default(),
        start_computing_scores_slot: 0,
        current_epoch: 800,
        next_cycle_epoch: 10,
        num_pool_validators: num_pool_validators.into(),
        scoring_unstake_total: 0,
        instant_unstake_total: 0,
        stake_deposit_unstake_total: 0,
        validators_added: 0,
        status_flags: 0,
        _padding0: [0; 2],
    }
}

/// Helper function to create a mock DirectedStakeMeta for testing
fn create_mock_directed_stake_meta(
    targets: Vec<(Pubkey, u64, u64)>, // (vote_pubkey, target_lamports, staked_lamports)
) -> DirectedStakeMeta {
    let mut meta = DirectedStakeMeta {
        total_stake_targets: 0,
        directed_unstake_total: 0,
        padding0: [0; 63],
        is_initialized: jito_steward::utils::U8Bool::from(true),
        directed_stake_lamports: [0; MAX_VALIDATORS],
        targets: [DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: 0,
            total_staked_lamports: 0,
            target_last_updated_epoch: 0,
            staked_last_updated_epoch: 0,
            _padding0: [0; 32],
        }; MAX_VALIDATORS],
    };

    for (i, (vote_pubkey, target_lamports, staked_lamports)) in targets.iter().enumerate() {
        if i < 2048 {
            meta.targets[i] = DirectedStakeTarget {
                vote_pubkey: *vote_pubkey,
                total_target_lamports: *target_lamports,
                total_staked_lamports: *staked_lamports,
                target_last_updated_epoch: 0,
                staked_last_updated_epoch: 0,
                _padding0: [0; 32],
            };
        }
    }

    // Set total_stake_targets to the actual number of targets provided
    meta.total_stake_targets = targets.len().min(2048) as u64;

    meta
}

#[test]
fn test_increase_stake_calculation_basic() {
    let state = create_mock_steward_state(3);
    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    let validator3 = Pubkey::new_unique();

    let directed_stake_meta = create_mock_directed_stake_meta(vec![
        (validator1, 1_000_000, 500_000),   // Needs 500k more
        (validator2, 2_000_000, 1_000_000), // Needs 1M more
        (validator3, 1_500_000, 1_500_000), // Already at target
    ]);

    // Test increasing stake for validator1
    let result = increase_stake_calculation(
        &state,
        &directed_stake_meta,
        0,
        500_000,
        1_200_000,
        false,
        0,
        0,
    );

    let validator1_proportion_bps = 3333;
    let expected_amount = (1_200_000 * validator1_proportion_bps) / 10_000;
    assert!(result.is_ok());
    match result.unwrap() {
        RebalanceType::Increase(amount) => {
            assert!(amount == expected_amount);
        }
        _ => panic!("Expected Increase variant"),
    }
}

#[test]
fn test_increase_stake_undirected_cap_reached() {
    let state = create_mock_steward_state(3);
    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    let validator3 = Pubkey::new_unique();

    let directed_stake_meta = create_mock_directed_stake_meta(vec![
        (validator1, 1_000_000, 500_000),   // Needs 500k more
        (validator2, 2_000_000, 1_000_000), // Needs 1M more
        (validator3, 1_500_000, 1_500_000), // Already at target
    ]);

    // Test increasing stake for validator1
    let result = increase_stake_calculation(
        &state,
        &directed_stake_meta,
        0,
        500_000,
        2_000_000,
        true,
        0,
        0,
    );

    assert!(result.is_ok());
    match result.unwrap() {
        RebalanceType::None => {}
        _ => panic!("Expected None variant"),
    }
}

#[test]
fn test_increase_stake_calculation_no_increase_needed() {
    let state = create_mock_steward_state(1);
    let validator1 = Pubkey::new_unique();

    let directed_stake_meta = create_mock_directed_stake_meta(vec![
        (validator1, 1_000_000, 1_000_000), // Already at target
    ]);

    let result = increase_stake_calculation(
        &state,
        &directed_stake_meta,
        0,         // validator1 index
        1_000_000, // current_lamports (already at target)
        1_000_000, // reserve_lamports
        false,
        0,
        0,
    );

    assert!(result.is_ok());
    match result.unwrap() {
        RebalanceType::None => {
            // Correct - no increase needed
        }
        _ => panic!("Expected None variant"),
    }
}

#[test]
fn test_increase_stake_calculation_index_out_of_bounds() {
    let state = create_mock_steward_state(2);
    let validator1 = Pubkey::new_unique();

    let directed_stake_meta =
        create_mock_directed_stake_meta(vec![(validator1, 1_000_000, 500_000)]);

    let result = increase_stake_calculation(
        &state,
        &directed_stake_meta,
        5, // Should be out of bounds
        500_000,
        5_000_000,
        false,
        0,
        0,
    );

    assert!(result.is_err());
}

#[test]
fn test_increase_stake_calculation_zero_reserve() {
    let state = create_mock_steward_state(1);
    let validator1 = Pubkey::new_unique();

    let directed_stake_meta = create_mock_directed_stake_meta(vec![
        (validator1, 1_000_000, 500_000), // Needs 500k more
    ]);

    let result =
        increase_stake_calculation(&state, &directed_stake_meta, 0, 500_000, 0, false, 0, 0);

    assert!(result.is_ok());
    match result.unwrap() {
        RebalanceType::Increase(amount) => {
            assert_eq!(amount, 0); // Should be 0 with no reserve
        }
        _ => panic!("Expected Increase variant"),
    }
}

#[test]
fn test_decrease_stake_calculation_basic() {
    let state = create_mock_steward_state(3);
    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    let validator3 = Pubkey::new_unique();

    let directed_stake_meta = create_mock_directed_stake_meta(vec![
        (validator1, 500_000, 1_000_000),   // Has 500k more than target
        (validator2, 1_000_000, 2_000_000), // Has 1M more than target
        (validator3, 1_500_000, 1_500_000), // At target
    ]);

    // Test decreasing stake for validator1
    let result = decrease_stake_calculation(
        &state,
        &directed_stake_meta,
        0,
        1_000_000,
        1_000_000_000_000,
        0,
        0,
    );

    assert!(result.is_ok());
    match result.unwrap() {
        RebalanceType::Decrease(components) => {
            assert!(components.directed_unstake_lamports > 0);
            assert!(components.directed_unstake_lamports <= 1_000_000);
        }
        _ => panic!("Expected Decrease variant"),
    }
}

#[test]
fn test_decrease_stake_calculation_no_decrease_needed() {
    let state = create_mock_steward_state(1);
    let validator1 = Pubkey::new_unique();

    let directed_stake_meta = create_mock_directed_stake_meta(vec![
        (validator1, 1_000_000, 1_000_000), // At target
    ]);

    let result = decrease_stake_calculation(
        &state,
        &directed_stake_meta,
        0,
        1_000_000, // at target
        1_000_000_000_000,
        0,
        1_000_000,
    );

    assert!(result.is_ok());
    match result.unwrap() {
        RebalanceType::None => {
            // Correct - no decrease needed
        }
        _ => panic!("Expected None variant"),
    }
}

#[test]
fn test_decrease_stake_calculation_index_out_of_bounds() {
    let state = create_mock_steward_state(2);
    let validator1 = Pubkey::new_unique();

    let directed_stake_meta =
        create_mock_directed_stake_meta(vec![(validator1, 1_000_000, 1_500_000)]);

    let result = decrease_stake_calculation(
        &state,
        &directed_stake_meta,
        5, // Should be out of bounds
        1_500_000,
        1_000_000_000_000,
        0,
        0,
    );

    assert!(result.is_err());
}

#[test]
fn test_decrease_stake_calculation_zero_cap() {
    let state = create_mock_steward_state(1);
    let validator1 = Pubkey::new_unique();

    let directed_stake_meta = create_mock_directed_stake_meta(vec![
        (validator1, 500_000_000, 1_000_000_000), // Has 500m more than target
    ]);

    let result =
        decrease_stake_calculation(&state, &directed_stake_meta, 0, 1_000_000_000, 0, 0, 0);

    assert!(result.is_ok());
    match result.unwrap() {
        RebalanceType::Decrease(components) => {
            assert_eq!(components.directed_unstake_lamports, 0);
        }
        _ => panic!("Expected Decrease variant"),
    }
}

#[test]
fn test_increase_stake_calculation_proportional_distribution() {
    let state = create_mock_steward_state(3);
    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    let validator3 = Pubkey::new_unique();

    let directed_stake_meta = create_mock_directed_stake_meta(vec![
        (validator1, 1_000_000, 500_000), // Needs 500k (33.33% of total delta)
        (validator2, 2_000_000, 1_000_000), // Needs 1M (66.67% of total delta)
        (validator3, 1_500_000, 1_500_000), // At target
    ]);

    let reserve_lamports = 1_500_000; // 1.5M reserve

    // Test validator1 (should get 33.33% of reserve)
    let result1 = increase_stake_calculation(
        &state,
        &directed_stake_meta,
        0,
        500_000,
        reserve_lamports,
        false,
        0,
        0,
    );

    let validator1_proportion_bps = 3333;
    let expected_amount = (reserve_lamports * validator1_proportion_bps) / 10_000;
    assert!(result1.is_ok());
    match result1.unwrap() {
        RebalanceType::Increase(amount1) => {
            assert!(amount1 == expected_amount);
        }
        _ => panic!("Expected Increase variant"),
    }

    let validator2_proportion_bps = 6666;
    let expected_amount = (reserve_lamports * validator2_proportion_bps) / 10_000;
    let result2 = increase_stake_calculation(
        &state,
        &directed_stake_meta,
        1,
        1_000_000,
        reserve_lamports,
        false,
        0,
        0,
    );

    assert!(result2.is_ok());
    match result2.unwrap() {
        RebalanceType::Increase(amount2) => {
            // Should be approximately 66.66% of 1.5M = 1M
            assert!(amount2 == expected_amount);
        }
        _ => panic!("Expected Increase variant"),
    }
}

#[test]
fn test_decrease_stake_directed_stake_lamports_tracking() {
    let state = create_mock_steward_state(5);
    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    let validator3 = Pubkey::new_unique();
    let validator4 = Pubkey::new_unique();
    let validator5 = Pubkey::new_unique();

    let directed_stake_meta = create_mock_directed_stake_meta(vec![
        (validator1, 500_000, 1_000_000), // Has 500k excess (25% of total excess)
        (validator2, 1_000_000, 1_500_000), // Has 500k excess (25% of total excess)
        (validator3, 500_000, 1_000_000), // Has 500k excess (25% of total excess)
        (validator4, 1_000_000, 1_500_000), // Has 500k excess (25% of total excess)
        (validator5, 1_500_000, 1_500_000), // At target
    ]);

    let result = decrease_stake_calculation(
        &state,
        &directed_stake_meta,
        0,
        1_000_000,
        1_000_000_000_000,
        0,
        0,
    );

    assert!(result.is_ok());
    match result.unwrap() {
        RebalanceType::Decrease(components1) => {
            assert!(components1.directed_unstake_lamports == 500_000);
        }
        _ => panic!("Expected Decrease variant"),
    }

    let result = decrease_stake_calculation(
        &state,
        &directed_stake_meta,
        1,
        2_000_000,
        1_000_000_000_000,
        0,
        0,
    );

    assert!(result.is_ok());
    match result.unwrap() {
        RebalanceType::Decrease(components2) => {
            assert!(components2.directed_unstake_lamports == 1_000_000);
        }
        _ => panic!("Expected Decrease variant"),
    }
}

#[test]
fn test_decrease_stake_directed_stake_lamports_with_cap() {
    let state = create_mock_steward_state(5);
    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    let validator3 = Pubkey::new_unique();
    let validator4 = Pubkey::new_unique();
    let validator5 = Pubkey::new_unique();

    let directed_stake_meta = create_mock_directed_stake_meta(vec![
        (validator1, 500_000, 1_000_000), // Has 500k excess (25% of total excess)
        (validator2, 1_000_000, 1_500_000), // Has 500k excess (25% of total excess)
        (validator3, 500_000, 1_000_000), // Has 500k excess (25% of total excess)
        (validator4, 1_000_000, 1_500_000), // Has 500k excess (25% of total excess)
        (validator5, 1_500_000, 1_500_000), // At target
    ]);

    let result =
        decrease_stake_calculation(&state, &directed_stake_meta, 0, 1_000_000, 1_000_000, 0, 0);

    assert!(result.is_ok());
    match result.unwrap() {
        RebalanceType::Decrease(components1) => {
            assert!(components1.directed_unstake_lamports == 250_000);
        }
        _ => panic!("Expected Decrease variant"),
    }

    let result =
        decrease_stake_calculation(&state, &directed_stake_meta, 1, 2_000_000, 1_000_000, 0, 0);

    assert!(result.is_ok());
    match result.unwrap() {
        RebalanceType::Decrease(components2) => {
            assert!(components2.directed_unstake_lamports == 500_000);
        }
        _ => panic!("Expected Decrease variant"),
    }
}

#[test]
fn test_edge_case_zero_values() {
    let state = create_mock_steward_state(1);
    let validator1 = Pubkey::new_unique();

    let directed_stake_meta = create_mock_directed_stake_meta(vec![
        (validator1, 0, 0), // Zero target and staked
    ]);

    // Test increase with zero values
    let result =
        increase_stake_calculation(&state, &directed_stake_meta, 0, 0, 0, false, 1_000_000, 0);

    assert!(result.is_ok());
    match result.unwrap() {
        RebalanceType::None => {
            // Should be None when delta is 0
        }
        _ => panic!("Expected None variant"),
    }
}
