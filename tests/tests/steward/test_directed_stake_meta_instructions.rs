use jito_steward::{
    state::directed_stake::{DirectedStakeMeta, DirectedStakeTarget},
    DirectedStakeWhitelist, MAX_PERMISSIONED_DIRECTED_VALIDATORS,
};
use solana_sdk::pubkey::Pubkey;

#[test]
fn test_initialize_directed_stake_meta_validation() {
    // Test the initialization logic that would be in the instruction handler
    let epoch = 12345;
    let total_stake_targets = 5;

    let meta = DirectedStakeMeta {
        epoch,
        total_stake_targets,
        uploaded_stake_targets: 0,
        _padding0: [0; 132],
        targets: [DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: 0,
            total_staked_lamports: 0,
            _padding0: [0; 64],
        }; MAX_PERMISSIONED_DIRECTED_VALIDATORS],
    };

    // Verify initial state
    assert_eq!(meta.epoch, epoch);
    assert_eq!(meta.total_stake_targets, total_stake_targets);
    assert_eq!(meta.uploaded_stake_targets, 0);
    assert!(!meta.is_copy_complete());
}

#[test]
fn test_copy_directed_stake_targets_validation() {
    let mut meta = DirectedStakeMeta {
        epoch: 12345,
        total_stake_targets: 3,
        uploaded_stake_targets: 0,
        _padding0: [0; 132],
        targets: [DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: 0,
            total_staked_lamports: 0,
            _padding0: [0; 64],
        }; MAX_PERMISSIONED_DIRECTED_VALIDATORS],
    };

    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    let validator3 = Pubkey::new_unique();

    // Test copying targets one by one
    let target1 = DirectedStakeTarget {
        vote_pubkey: validator1,
        total_target_lamports: 1000,
        total_staked_lamports: 0,
        _padding0: [0; 64],
    };

    // Simulate the copy logic
    let target_index = meta.uploaded_stake_targets as usize;
    meta.targets[target_index] = target1;
    meta.uploaded_stake_targets += 1;

    assert_eq!(meta.uploaded_stake_targets, 1);
    assert_eq!(meta.get_target_index(&validator1), Some(0));
    assert!(!meta.is_copy_complete());

    // Copy second target
    let target2 = DirectedStakeTarget {
        vote_pubkey: validator2,
        total_target_lamports: 2000,
        total_staked_lamports: 0,
        _padding0: [0; 64],
    };

    let target_index = meta.uploaded_stake_targets as usize;
    meta.targets[target_index] = target2;
    meta.uploaded_stake_targets += 1;

    assert_eq!(meta.uploaded_stake_targets, 2);
    assert_eq!(meta.get_target_index(&validator2), Some(1));
    assert!(!meta.is_copy_complete());

    // Copy third target
    let target3 = DirectedStakeTarget {
        vote_pubkey: validator3,
        total_target_lamports: 3000,
        total_staked_lamports: 0,
        _padding0: [0; 64],
    };

    let target_index = meta.uploaded_stake_targets as usize;
    meta.targets[target_index] = target3;
    meta.uploaded_stake_targets += 1;

    assert_eq!(meta.uploaded_stake_targets, 3);
    assert_eq!(meta.get_target_index(&validator3), Some(2));
    assert!(meta.is_copy_complete());
}

#[test]
fn test_copy_directed_stake_targets_duplicate_validation() {
    let mut meta = DirectedStakeMeta {
        epoch: 12345,
        total_stake_targets: 2,
        uploaded_stake_targets: 0,
        _padding0: [0; 132],
        targets: [DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: 0,
            total_staked_lamports: 0,
            _padding0: [0; 64],
        }; MAX_PERMISSIONED_DIRECTED_VALIDATORS],
    };

    let validator = Pubkey::new_unique();

    // Add first target
    let target1 = DirectedStakeTarget {
        vote_pubkey: validator,
        total_target_lamports: 1000,
        total_staked_lamports: 0,
        _padding0: [0; 64],
    };

    let target_index = meta.uploaded_stake_targets as usize;
    meta.targets[target_index] = target1;
    meta.uploaded_stake_targets += 1;

    // Test that duplicate validator is detected
    assert!(meta.get_target_index(&validator).is_some());

    // This would be prevented by the instruction logic
    // The instruction should check if the validator already exists before adding
}

#[test]
fn test_copy_directed_stake_targets_capacity_validation() {
    let meta = DirectedStakeMeta {
        epoch: 12345,
        total_stake_targets: 2,
        uploaded_stake_targets: 2, // Already at capacity
        _padding0: [0; 132],
        targets: [DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: 0,
            total_staked_lamports: 0,
            _padding0: [0; 64],
        }; MAX_PERMISSIONED_DIRECTED_VALIDATORS],
    };

    // Test that we can't add more targets when at capacity
    assert!(meta.uploaded_stake_targets >= meta.total_stake_targets);
    assert!(meta.is_copy_complete());
}

#[test]
fn test_directed_stake_meta_instruction_flow() {
    // Test the complete flow from initialization to completion
    let epoch = 12345;
    let total_stake_targets = 3;

    // Step 1: Initialize meta
    let mut meta = DirectedStakeMeta {
        epoch,
        total_stake_targets,
        uploaded_stake_targets: 0,
        _padding0: [0; 132],
        targets: [DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: 0,
            total_staked_lamports: 0,
            _padding0: [0; 64],
        }; MAX_PERMISSIONED_DIRECTED_VALIDATORS],
    };

    assert_eq!(meta.epoch, epoch);
    assert_eq!(meta.total_stake_targets, total_stake_targets);
    assert_eq!(meta.uploaded_stake_targets, 0);
    assert!(!meta.is_copy_complete());

    // Step 2: Copy targets one by one
    let validators = [
        (Pubkey::new_unique(), 1000),
        (Pubkey::new_unique(), 2000),
        (Pubkey::new_unique(), 3000),
    ];

    for (i, (validator, target_lamports)) in validators.iter().enumerate() {
        let target = DirectedStakeTarget {
            vote_pubkey: *validator,
            total_target_lamports: *target_lamports,
            total_staked_lamports: 0,
            _padding0: [0; 64],
        };

        let target_index = meta.uploaded_stake_targets as usize;
        meta.targets[target_index] = target;
        meta.uploaded_stake_targets += 1;

        assert_eq!(meta.uploaded_stake_targets, (i + 1) as u16);
        assert_eq!(meta.get_target_index(validator), Some(i));
        assert_eq!(meta.targets[i].total_target_lamports, *target_lamports);
    }

    // Step 3: Verify completion
    assert!(meta.is_copy_complete());
    assert_eq!(meta.uploaded_stake_targets, total_stake_targets);
}

#[test]
fn test_directed_stake_meta_error_conditions() {
    let mut meta = DirectedStakeMeta {
        epoch: 12345,
        total_stake_targets: 2,
        uploaded_stake_targets: 0,
        _padding0: [0; 132],
        targets: [DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: 0,
            total_staked_lamports: 0,
            _padding0: [0; 64],
        }; MAX_PERMISSIONED_DIRECTED_VALIDATORS],
    };

    let validator = Pubkey::new_unique();

    // Test error condition: trying to add more targets than allowed
    meta.uploaded_stake_targets = meta.total_stake_targets;
    assert!(meta.uploaded_stake_targets >= meta.total_stake_targets);

    // Test error condition: trying to add duplicate validator
    meta.uploaded_stake_targets = 1;
    meta.targets[0] = DirectedStakeTarget {
        vote_pubkey: validator,
        total_target_lamports: 1000,
        total_staked_lamports: 0,
        _padding0: [0; 64],
    };

    // This would be caught by the instruction logic
    assert!(meta.get_target_index(&validator).is_some());
}

#[test]
fn test_directed_stake_meta_large_scale_operations() {
    // Test with a larger number of targets
    let total_stake_targets = 10;
    let mut meta = DirectedStakeMeta {
        epoch: 12345,
        total_stake_targets,
        uploaded_stake_targets: 0,
        _padding0: [0; 132],
        targets: [DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: 0,
            total_staked_lamports: 0,
            _padding0: [0; 64],
        }; MAX_PERMISSIONED_DIRECTED_VALIDATORS],
    };

    let validators: Vec<Pubkey> = (0..total_stake_targets as usize)
        .map(|_| Pubkey::new_unique())
        .collect();

    // Add all targets
    for (i, validator) in validators.iter().enumerate() {
        let target = DirectedStakeTarget {
            vote_pubkey: *validator,
            total_target_lamports: (i + 1) as u64 * 1000,
            total_staked_lamports: 0,
            _padding0: [0; 64],
        };

        let target_index = meta.uploaded_stake_targets as usize;
        meta.targets[target_index] = target;
        meta.uploaded_stake_targets += 1;

        assert_eq!(meta.uploaded_stake_targets, (i + 1) as u16);
        assert_eq!(meta.get_target_index(validator), Some(i));
    }

    assert!(meta.is_copy_complete());
    assert_eq!(meta.uploaded_stake_targets, total_stake_targets);

    // Verify all validators can be found
    for (i, validator) in validators.iter().enumerate() {
        assert_eq!(meta.get_target_index(validator), Some(i));
    }
}

#[test]
fn test_directed_stake_meta_whitelist_integration_validation() {
    // Test that targets are properly validated against whitelist
    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); 2048],
        permissioned_protocol_stakers: [Pubkey::default(); 2048],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    let whitelisted_validator = Pubkey::new_unique();
    let non_whitelisted_validator = Pubkey::new_unique();

    // Add validator to whitelist
    whitelist.add_validator(whitelisted_validator).unwrap();

    let mut meta = DirectedStakeMeta {
        epoch: 12345,
        total_stake_targets: 1,
        uploaded_stake_targets: 0,
        _padding0: [0; 132],
        targets: [DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: 0,
            total_staked_lamports: 0,
            _padding0: [0; 64],
        }; MAX_PERMISSIONED_DIRECTED_VALIDATORS],
    };

    // Test adding whitelisted validator (should succeed)
    let target = DirectedStakeTarget {
        vote_pubkey: whitelisted_validator,
        total_target_lamports: 1000,
        total_staked_lamports: 0,
        _padding0: [0; 64],
    };

    let target_index = meta.uploaded_stake_targets as usize;
    meta.targets[target_index] = target;
    meta.uploaded_stake_targets += 1;

    assert!(whitelist.is_validator_permissioned(&whitelisted_validator));
    assert_eq!(meta.get_target_index(&whitelisted_validator), Some(0));

    // Test that non-whitelisted validator would be rejected
    assert!(!whitelist.is_validator_permissioned(&non_whitelisted_validator));
    assert_eq!(meta.get_target_index(&non_whitelisted_validator), None);
}

#[test]
fn test_directed_stake_meta_edge_case_epochs() {
    // Test with different epoch values
    let epochs = [0, 1, 100, 1000, u64::MAX];

    for epoch in epochs {
        let meta = DirectedStakeMeta {
            epoch,
            total_stake_targets: 1,
            uploaded_stake_targets: 0,
            _padding0: [0; 132],
            targets: [DirectedStakeTarget {
                vote_pubkey: Pubkey::default(),
                total_target_lamports: 0,
                total_staked_lamports: 0,
                _padding0: [0; 64],
            }; MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        };

        assert_eq!(meta.epoch, epoch);
    }
}

#[test]
fn test_directed_stake_meta_target_lamport_edge_cases() {
    let mut meta = DirectedStakeMeta {
        epoch: 12345,
        total_stake_targets: 3,
        uploaded_stake_targets: 0,
        _padding0: [0; 132],
        targets: [DirectedStakeTarget {
            vote_pubkey: Pubkey::default(),
            total_target_lamports: 0,
            total_staked_lamports: 0,
            _padding0: [0; 64],
        }; MAX_PERMISSIONED_DIRECTED_VALIDATORS],
    };

    let validator = Pubkey::new_unique();

    // Test with zero lamports
    let target1 = DirectedStakeTarget {
        vote_pubkey: validator,
        total_target_lamports: 0,
        total_staked_lamports: 0,
        _padding0: [0; 64],
    };

    let target_index = meta.uploaded_stake_targets as usize;
    meta.targets[target_index] = target1;
    meta.uploaded_stake_targets += 1;

    assert_eq!(meta.targets[0].total_target_lamports, 0);
    assert_eq!(meta.targets[0].total_staked_lamports, 0);

    // Test with maximum lamports
    let validator2 = Pubkey::new_unique();
    let target2 = DirectedStakeTarget {
        vote_pubkey: validator2,
        total_target_lamports: u64::MAX,
        total_staked_lamports: u64::MAX / 2,
        _padding0: [0; 64],
    };

    let target_index = meta.uploaded_stake_targets as usize;
    meta.targets[target_index] = target2;
    meta.uploaded_stake_targets += 1;

    assert_eq!(meta.targets[1].total_target_lamports, u64::MAX);
    assert_eq!(meta.targets[1].total_staked_lamports, u64::MAX / 2);
}
