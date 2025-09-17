use jito_steward::{
    state::directed_stake::{DirectedStakePreference, DirectedStakeRecordType},
    utils::U8Bool,
    DirectedStakeTicket, DirectedStakeWhitelist, MAX_PERMISSIONED_DIRECTED_STAKERS,
    MAX_PERMISSIONED_DIRECTED_VALIDATORS, MAX_PREFERENCES_PER_TICKET,
};
use solana_sdk::pubkey::Pubkey;

#[test]
fn test_directed_stake_record_type_serialization() {
    let validator_type = DirectedStakeRecordType::Validator;
    let user_type = DirectedStakeRecordType::User;
    let protocol_type = DirectedStakeRecordType::Protocol;

    assert_eq!(validator_type, DirectedStakeRecordType::Validator);
    assert_eq!(user_type, DirectedStakeRecordType::User);
    assert_eq!(protocol_type, DirectedStakeRecordType::Protocol);
    assert_ne!(validator_type, user_type);
    assert_ne!(validator_type, protocol_type);
    assert_ne!(user_type, protocol_type);
}

#[test]
fn test_directed_stake_ticket_initialization() {
    let update_authority = Pubkey::new_unique();
    let close_authority = Pubkey::new_unique();

    let ticket = DirectedStakeTicket {
        num_preferences: 0,
        staker_preferences: [DirectedStakePreference {
            vote_pubkey: Pubkey::default(),
            stake_share_bps: 0,
            _padding0: [0; 94],
        }; MAX_PREFERENCES_PER_TICKET],
        ticket_update_authority: update_authority,
        ticket_close_authority: close_authority,
        ticket_holder_is_protocol: U8Bool::from(false),
        _padding0: [0; 125],
    };

    assert_eq!(ticket.num_preferences, 0);
    assert_eq!(ticket.ticket_update_authority, update_authority);
    assert_eq!(ticket.ticket_close_authority, close_authority);
    assert_eq!(ticket.ticket_holder_is_protocol, U8Bool::from(false));

    assert!(ticket.preferences_valid());

    let allocations = ticket.get_allocations(1000);
    assert_eq!(allocations.len(), 0);
}

#[test]
fn test_directed_stake_ticket_with_preferences() {
    let update_authority = Pubkey::new_unique();
    let close_authority = Pubkey::new_unique();
    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();

    let mut ticket = DirectedStakeTicket {
        num_preferences: 2,
        staker_preferences: [DirectedStakePreference {
            vote_pubkey: validator1,
            stake_share_bps: 4000,
            _padding0: [0; 94],
        }; MAX_PREFERENCES_PER_TICKET],
        ticket_update_authority: update_authority,
        ticket_close_authority: close_authority,
        ticket_holder_is_protocol: U8Bool::from(true),
        _padding0: [0; 125],
    };

    ticket.staker_preferences[1] = DirectedStakePreference {
        vote_pubkey: validator2,
        stake_share_bps: 6000,
        _padding0: [0; 94],
    };

    assert!(ticket.preferences_valid());

    let allocations = ticket.get_allocations(10000);
    assert_eq!(allocations.len(), 2);
    assert_eq!(allocations[0], (validator1, 4000));
    assert_eq!(allocations[1], (validator2, 6000));
}

#[test]
fn test_directed_stake_ticket_invalid_preferences() {
    let update_authority = Pubkey::new_unique();
    let close_authority = Pubkey::new_unique();
    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();

    let mut ticket = DirectedStakeTicket {
        num_preferences: 2,
        staker_preferences: [DirectedStakePreference {
            vote_pubkey: validator1,
            stake_share_bps: 6000, // 60%
            _padding0: [0; 94],
        }; MAX_PREFERENCES_PER_TICKET],
        ticket_update_authority: update_authority,
        ticket_close_authority: close_authority,
        ticket_holder_is_protocol: U8Bool::from(false),
        _padding0: [0; 125],
    };

    ticket.staker_preferences[1] = DirectedStakePreference {
        vote_pubkey: validator2,
        stake_share_bps: 6000, // 60% + 60% = 120% > 100%
        _padding0: [0; 94],
    };

    assert!(!ticket.preferences_valid());
}

#[test]
fn test_directed_stake_whitelist_initialization() {
    let whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    assert_eq!(whitelist.total_permissioned_user_stakers, 0);
    assert_eq!(whitelist.total_permissioned_protocol_stakers, 0);
    assert_eq!(whitelist.total_permissioned_validators, 0);
    assert!(whitelist.can_add_staker());
    assert!(whitelist.can_add_validator());
}

#[test]
fn test_directed_stake_whitelist_add_operations() {
    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    let user_staker = Pubkey::new_unique();
    let protocol_staker = Pubkey::new_unique();
    let validator = Pubkey::new_unique();

    let result = whitelist.add_user_staker(user_staker);
    assert!(result.is_ok());
    assert_eq!(whitelist.total_permissioned_user_stakers, 1);
    assert!(whitelist.is_user_staker_permissioned(&user_staker));
    assert!(whitelist.is_staker_permissioned(&user_staker));

    let result = whitelist.add_protocol_staker(protocol_staker);
    assert!(result.is_ok());
    assert_eq!(whitelist.total_permissioned_protocol_stakers, 1);
    assert!(whitelist.is_protocol_staker_permissioned(&protocol_staker));
    assert!(whitelist.is_staker_permissioned(&protocol_staker));

    let result = whitelist.add_validator(validator);
    assert!(result.is_ok());
    assert_eq!(whitelist.total_permissioned_validators, 1);
    assert!(whitelist.is_validator_permissioned(&validator));

    let result = whitelist.add_user_staker(user_staker);
    assert!(result.is_err());

    let result = whitelist.add_protocol_staker(protocol_staker);
    assert!(result.is_err());

    let result = whitelist.add_validator(validator);
    assert!(result.is_err());
}

#[test]
fn test_directed_stake_whitelist_remove_operations() {
    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    let user_staker = Pubkey::new_unique();
    let protocol_staker = Pubkey::new_unique();
    let validator = Pubkey::new_unique();

    whitelist.add_user_staker(user_staker).unwrap();
    whitelist.add_protocol_staker(protocol_staker).unwrap();
    whitelist.add_validator(validator).unwrap();

    let result = whitelist.remove_user_staker(&user_staker);
    assert!(result.is_ok());
    assert_eq!(whitelist.total_permissioned_user_stakers, 0);
    assert!(!whitelist.is_user_staker_permissioned(&user_staker));
    assert!(!whitelist.is_staker_permissioned(&user_staker));

    let result = whitelist.remove_protocol_staker(&protocol_staker);
    assert!(result.is_ok());
    assert_eq!(whitelist.total_permissioned_protocol_stakers, 0);
    assert!(!whitelist.is_protocol_staker_permissioned(&protocol_staker));
    assert!(!whitelist.is_staker_permissioned(&protocol_staker));

    let result = whitelist.remove_validator(&validator);
    assert!(result.is_ok());
    assert_eq!(whitelist.total_permissioned_validators, 0);
    assert!(!whitelist.is_validator_permissioned(&validator));

    let result = whitelist.remove_user_staker(&user_staker);
    assert!(result.is_err());

    let result = whitelist.remove_protocol_staker(&protocol_staker);
    assert!(result.is_err());

    let result = whitelist.remove_validator(&validator);
    assert!(result.is_err());
}

#[test]
fn test_directed_stake_whitelist_edge_cases() {
    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    let non_existent = Pubkey::new_unique();
    let result = whitelist.remove_staker(&non_existent);
    assert!(result.is_err());

    let result = whitelist.remove_validator(&non_existent);
    assert!(result.is_err());

    assert!(!whitelist.is_staker_permissioned(&non_existent));
    assert!(!whitelist.is_validator_permissioned(&non_existent));
}

#[test]
fn test_directed_stake_ticket_edge_cases() {
    let update_authority = Pubkey::new_unique();
    let close_authority = Pubkey::new_unique();

    let mut ticket = DirectedStakeTicket {
        num_preferences: 0,
        staker_preferences: [DirectedStakePreference {
            vote_pubkey: Pubkey::default(),
            stake_share_bps: 0,
            _padding0: [0; 94],
        }; MAX_PREFERENCES_PER_TICKET],
        ticket_update_authority: update_authority,
        ticket_close_authority: close_authority,
        ticket_holder_is_protocol: U8Bool::from(false),
        _padding0: [0; 125],
    };

    let allocations = ticket.get_allocations(0);
    assert_eq!(allocations.len(), 0);

    ticket.num_preferences = 1;
    ticket.staker_preferences[0] = DirectedStakePreference {
        vote_pubkey: Pubkey::new_unique(),
        stake_share_bps: 10000, // 100%
        _padding0: [0; 94],
    };

    assert!(ticket.preferences_valid());
    let allocations = ticket.get_allocations(1000);
    assert_eq!(allocations.len(), 1);
    assert_eq!(allocations[0].1, 1000);

    ticket.staker_preferences[0].stake_share_bps = 0;
    let allocations = ticket.get_allocations(1000);
    assert_eq!(allocations.len(), 0);
}

#[test]
fn test_directed_stake_whitelist_validation_logic() {
    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    let staker = Pubkey::new_unique();
    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    let non_whitelisted_validator = Pubkey::new_unique();

    // Add staker and validators to whitelist
    whitelist.add_staker(staker).unwrap();
    whitelist.add_validator(validator1).unwrap();
    whitelist.add_validator(validator2).unwrap();

    // Test staker validation
    assert!(whitelist.is_staker_permissioned(&staker));
    assert!(!whitelist.is_staker_permissioned(&Pubkey::new_unique()));

    // Test validator validation
    assert!(whitelist.is_validator_permissioned(&validator1));
    assert!(whitelist.is_validator_permissioned(&validator2));
    assert!(!whitelist.is_validator_permissioned(&non_whitelisted_validator));

    // Test preferences validation scenario
    let valid_preferences = vec![
        DirectedStakePreference {
            vote_pubkey: validator1,
            stake_share_bps: 5000,
            _padding0: [0; 94],
        },
        DirectedStakePreference {
            vote_pubkey: validator2,
            stake_share_bps: 5000,
            _padding0: [0; 94],
        },
    ];

    // All validators in preferences should be whitelisted
    for preference in &valid_preferences {
        assert!(whitelist.is_validator_permissioned(&preference.vote_pubkey));
    }

    // Test invalid preferences with non-whitelisted validator
    let invalid_preferences = vec![
        DirectedStakePreference {
            vote_pubkey: validator1,
            stake_share_bps: 5000,
            _padding0: [0; 94],
        },
        DirectedStakePreference {
            vote_pubkey: non_whitelisted_validator, // This should fail validation
            stake_share_bps: 5000,
            _padding0: [0; 94],
        },
    ];

    // At least one validator should not be whitelisted
    let has_non_whitelisted = invalid_preferences
        .iter()
        .any(|pref| !whitelist.is_validator_permissioned(&pref.vote_pubkey));
    assert!(has_non_whitelisted);
}

#[test]
fn test_permissioned_stakers_validation() {
    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    let staker1 = Pubkey::new_unique();
    let staker2 = Pubkey::new_unique();
    let staker3 = Pubkey::new_unique();
    let non_permissioned_staker = Pubkey::new_unique();

    // Test initial state - no stakers should be permissioned
    assert!(!whitelist.is_staker_permissioned(&staker1));
    assert!(!whitelist.is_staker_permissioned(&staker2));
    assert!(!whitelist.is_staker_permissioned(&non_permissioned_staker));

    // Add stakers to whitelist (as user stakers)
    assert!(whitelist.add_user_staker(staker1).is_ok());
    assert!(whitelist.add_user_staker(staker2).is_ok());
    assert!(whitelist.add_user_staker(staker3).is_ok());

    // Verify stakers are now permissioned
    assert!(whitelist.is_staker_permissioned(&staker1));
    assert!(whitelist.is_staker_permissioned(&staker2));
    assert!(whitelist.is_staker_permissioned(&staker3));
    assert!(!whitelist.is_staker_permissioned(&non_permissioned_staker));

    // Test can_add_staker method
    assert!(whitelist.can_add_staker());
    assert_eq!(whitelist.total_permissioned_user_stakers, 3);

    // Test duplicate addition should fail
    assert!(whitelist.add_user_staker(staker1).is_err());
    assert_eq!(whitelist.total_permissioned_user_stakers, 3); // Should remain unchanged

    // Test removing a staker
    assert!(whitelist.remove_user_staker(&staker2).is_ok());
    assert!(!whitelist.is_staker_permissioned(&staker2));
    assert!(whitelist.is_staker_permissioned(&staker1));
    assert!(whitelist.is_staker_permissioned(&staker3));
    assert_eq!(whitelist.total_permissioned_user_stakers, 2);

    // Test removing non-existent staker should fail
    assert!(whitelist
        .remove_user_staker(&non_permissioned_staker)
        .is_err());
    assert_eq!(whitelist.total_permissioned_user_stakers, 2); // Should remain unchanged
}

#[test]
fn test_permissioned_validators_validation() {
    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    let validator3 = Pubkey::new_unique();
    let non_permissioned_validator = Pubkey::new_unique();

    // Test initial state - no validators should be permissioned
    assert!(!whitelist.is_validator_permissioned(&validator1));
    assert!(!whitelist.is_validator_permissioned(&validator2));
    assert!(!whitelist.is_validator_permissioned(&non_permissioned_validator));

    // Add validators to whitelist
    assert!(whitelist.add_validator(validator1).is_ok());
    assert!(whitelist.add_validator(validator2).is_ok());
    assert!(whitelist.add_validator(validator3).is_ok());

    // Verify validators are now permissioned
    assert!(whitelist.is_validator_permissioned(&validator1));
    assert!(whitelist.is_validator_permissioned(&validator2));
    assert!(whitelist.is_validator_permissioned(&validator3));
    assert!(!whitelist.is_validator_permissioned(&non_permissioned_validator));

    // Test can_add_validator method
    assert!(whitelist.can_add_validator());
    assert_eq!(whitelist.total_permissioned_validators, 3);

    // Test duplicate addition should fail
    assert!(whitelist.add_validator(validator1).is_err());
    assert_eq!(whitelist.total_permissioned_validators, 3); // Should remain unchanged

    // Test removing a validator
    assert!(whitelist.remove_validator(&validator2).is_ok());
    assert!(!whitelist.is_validator_permissioned(&validator2));
    assert!(whitelist.is_validator_permissioned(&validator1));
    assert!(whitelist.is_validator_permissioned(&validator3));
    assert_eq!(whitelist.total_permissioned_validators, 2);

    // Test removing non-existent validator should fail
    assert!(whitelist
        .remove_validator(&non_permissioned_validator)
        .is_err());
    assert_eq!(whitelist.total_permissioned_validators, 2); // Should remain unchanged
}

#[test]
fn test_ticket_authorization_scenarios() {
    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    let permissioned_staker = Pubkey::new_unique();
    let non_permissioned_staker = Pubkey::new_unique();
    let permissioned_validator = Pubkey::new_unique();
    let non_permissioned_validator = Pubkey::new_unique();
    let update_authority = Pubkey::new_unique();
    let close_authority = Pubkey::new_unique();

    // Setup whitelist
    whitelist.add_staker(permissioned_staker).unwrap();
    whitelist.add_validator(permissioned_validator).unwrap();

    // Test ticket initialization authorization
    // Permissioned staker should be able to initialize ticket
    assert!(whitelist.is_staker_permissioned(&permissioned_staker));

    // Non-permissioned staker should not be able to initialize ticket
    assert!(!whitelist.is_staker_permissioned(&non_permissioned_staker));

    // Test ticket update authorization scenarios
    let _ticket = DirectedStakeTicket {
        num_preferences: 0,
        staker_preferences: [DirectedStakePreference::empty(); MAX_PREFERENCES_PER_TICKET],
        ticket_update_authority: update_authority,
        ticket_close_authority: close_authority,
        ticket_holder_is_protocol: U8Bool::from(false),
        _padding0: [0; 125],
    };

    // Test valid preferences (all validators whitelisted)
    let valid_preferences = vec![DirectedStakePreference {
        vote_pubkey: permissioned_validator,
        stake_share_bps: 10000,
        _padding0: [0; 94],
    }];

    // All validators in preferences should be whitelisted
    for preference in &valid_preferences {
        assert!(whitelist.is_validator_permissioned(&preference.vote_pubkey));
    }

    // Test invalid preferences (non-whitelisted validator)
    let invalid_preferences = [DirectedStakePreference {
        vote_pubkey: non_permissioned_validator,
        stake_share_bps: 10000,
        _padding0: [0; 94],
    }];

    // At least one validator should not be whitelisted
    let has_non_whitelisted = invalid_preferences
        .iter()
        .any(|pref| !whitelist.is_validator_permissioned(&pref.vote_pubkey));
    assert!(has_non_whitelisted);
}

#[test]
fn test_whitelist_capacity_limits() {
    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    // Test that we can add up to the maximum number of user stakers
    for i in 0..MAX_PERMISSIONED_DIRECTED_STAKERS {
        let staker = Pubkey::new_unique();
        assert!(whitelist.add_user_staker(staker).is_ok());
        assert_eq!(whitelist.total_permissioned_user_stakers, (i + 1) as u16);
    }

    // Test that we can add up to the maximum number of validators
    for i in 0..MAX_PERMISSIONED_DIRECTED_VALIDATORS {
        let validator = Pubkey::new_unique();
        assert!(whitelist.add_validator(validator).is_ok());
        assert_eq!(whitelist.total_permissioned_validators, (i + 1) as u16);
    }

    // Test that we can't add more user stakers beyond the limit
    let extra_staker = Pubkey::new_unique();
    assert!(whitelist.add_user_staker(extra_staker).is_err());
    assert_eq!(
        whitelist.total_permissioned_user_stakers,
        MAX_PERMISSIONED_DIRECTED_STAKERS as u16
    );

    // Test that we can't add more validators beyond the limit
    let extra_validator = Pubkey::new_unique();
    assert!(whitelist.add_validator(extra_validator).is_err());
    assert_eq!(
        whitelist.total_permissioned_validators,
        MAX_PERMISSIONED_DIRECTED_VALIDATORS as u16
    );

    // Test can_add methods return false when at capacity
    assert!(!whitelist.can_add_user_staker());
    assert!(!whitelist.can_add_validator());
    // Note: can_add_staker() returns true if either user or protocol stakers can be added
    // Since we only filled user stakers, protocol stakers can still be added
    assert!(whitelist.can_add_staker());
}

#[test]
fn test_whitelist_removal_and_readdition() {
    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    let staker = Pubkey::new_unique();
    let validator = Pubkey::new_unique();

    // Add staker and validator
    whitelist.add_user_staker(staker).unwrap();
    whitelist.add_validator(validator).unwrap();

    assert!(whitelist.is_staker_permissioned(&staker));
    assert!(whitelist.is_validator_permissioned(&validator));
    assert_eq!(whitelist.total_permissioned_user_stakers, 1);
    assert_eq!(whitelist.total_permissioned_validators, 1);

    // Remove them
    whitelist.remove_user_staker(&staker).unwrap();
    whitelist.remove_validator(&validator).unwrap();

    assert!(!whitelist.is_staker_permissioned(&staker));
    assert!(!whitelist.is_validator_permissioned(&validator));
    assert_eq!(whitelist.total_permissioned_user_stakers, 0);
    assert_eq!(whitelist.total_permissioned_validators, 0);

    // Add them back
    whitelist.add_user_staker(staker).unwrap();
    whitelist.add_validator(validator).unwrap();

    assert!(whitelist.is_staker_permissioned(&staker));
    assert!(whitelist.is_validator_permissioned(&validator));
    assert_eq!(whitelist.total_permissioned_user_stakers, 1);
    assert_eq!(whitelist.total_permissioned_validators, 1);
}

#[test]
fn test_preferences_validation_edge_cases() {
    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    let validator1 = Pubkey::new_unique();
    let validator2 = Pubkey::new_unique();
    let validator3 = Pubkey::new_unique();

    // Add only some validators to whitelist
    whitelist.add_validator(validator1).unwrap();
    whitelist.add_validator(validator2).unwrap();
    // validator3 is not added to whitelist

    // Test mixed valid/invalid preferences
    let mixed_preferences = vec![
        DirectedStakePreference {
            vote_pubkey: validator1, // Valid - whitelisted
            stake_share_bps: 5000,
            _padding0: [0; 94],
        },
        DirectedStakePreference {
            vote_pubkey: validator2, // Valid - whitelisted
            stake_share_bps: 3000,
            _padding0: [0; 94],
        },
        DirectedStakePreference {
            vote_pubkey: validator3, // Invalid - not whitelisted
            stake_share_bps: 2000,
            _padding0: [0; 94],
        },
    ];

    // Check individual validators
    assert!(whitelist.is_validator_permissioned(&validator1));
    assert!(whitelist.is_validator_permissioned(&validator2));
    assert!(!whitelist.is_validator_permissioned(&validator3));

    // Check that mixed preferences contain at least one invalid validator
    let has_invalid = mixed_preferences
        .iter()
        .any(|pref| !whitelist.is_validator_permissioned(&pref.vote_pubkey));
    assert!(has_invalid);

    // Test all valid preferences
    let all_valid_preferences = vec![
        DirectedStakePreference {
            vote_pubkey: validator1,
            stake_share_bps: 6000,
            _padding0: [0; 94],
        },
        DirectedStakePreference {
            vote_pubkey: validator2,
            stake_share_bps: 4000,
            _padding0: [0; 94],
        },
    ];

    // All validators should be whitelisted
    let all_valid = all_valid_preferences
        .iter()
        .all(|pref| whitelist.is_validator_permissioned(&pref.vote_pubkey));
    assert!(all_valid);
}

#[test]
fn test_user_and_protocol_staker_separation() {
    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    let user_staker1 = Pubkey::new_unique();
    let user_staker2 = Pubkey::new_unique();
    let protocol_staker1 = Pubkey::new_unique();
    let protocol_staker2 = Pubkey::new_unique();

    // Add user stakers
    whitelist.add_user_staker(user_staker1).unwrap();
    whitelist.add_user_staker(user_staker2).unwrap();

    // Add protocol stakers
    whitelist.add_protocol_staker(protocol_staker1).unwrap();
    whitelist.add_protocol_staker(protocol_staker2).unwrap();

    // Verify counts
    assert_eq!(whitelist.total_permissioned_user_stakers, 2);
    assert_eq!(whitelist.total_permissioned_protocol_stakers, 2);

    // Verify user staker permissions
    assert!(whitelist.is_user_staker_permissioned(&user_staker1));
    assert!(whitelist.is_user_staker_permissioned(&user_staker2));
    assert!(!whitelist.is_user_staker_permissioned(&protocol_staker1));
    assert!(!whitelist.is_user_staker_permissioned(&protocol_staker2));

    // Verify protocol staker permissions
    assert!(whitelist.is_protocol_staker_permissioned(&protocol_staker1));
    assert!(whitelist.is_protocol_staker_permissioned(&protocol_staker2));
    assert!(!whitelist.is_protocol_staker_permissioned(&user_staker1));
    assert!(!whitelist.is_protocol_staker_permissioned(&user_staker2));

    // Verify general staker permissions (should work for both types)
    assert!(whitelist.is_staker_permissioned(&user_staker1));
    assert!(whitelist.is_staker_permissioned(&user_staker2));
    assert!(whitelist.is_staker_permissioned(&protocol_staker1));
    assert!(whitelist.is_staker_permissioned(&protocol_staker2));

    // Test removal
    whitelist.remove_user_staker(&user_staker1).unwrap();
    assert_eq!(whitelist.total_permissioned_user_stakers, 1);
    assert!(!whitelist.is_user_staker_permissioned(&user_staker1));
    assert!(!whitelist.is_staker_permissioned(&user_staker1));

    whitelist.remove_protocol_staker(&protocol_staker1).unwrap();
    assert_eq!(whitelist.total_permissioned_protocol_stakers, 1);
    assert!(!whitelist.is_protocol_staker_permissioned(&protocol_staker1));
    assert!(!whitelist.is_staker_permissioned(&protocol_staker1));

    // Verify remaining stakers still work
    assert!(whitelist.is_user_staker_permissioned(&user_staker2));
    assert!(whitelist.is_protocol_staker_permissioned(&protocol_staker2));
    assert!(whitelist.is_staker_permissioned(&user_staker2));
    assert!(whitelist.is_staker_permissioned(&protocol_staker2));
}

#[test]
fn test_user_and_protocol_staker_capacity_limits() {
    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    // Fill up user stakers
    for i in 0..MAX_PERMISSIONED_DIRECTED_STAKERS {
        let staker = Pubkey::new_unique();
        assert!(whitelist.add_user_staker(staker).is_ok());
        assert_eq!(whitelist.total_permissioned_user_stakers, (i + 1) as u16);
    }

    // Fill up protocol stakers
    for i in 0..MAX_PERMISSIONED_DIRECTED_STAKERS {
        let staker = Pubkey::new_unique();
        assert!(whitelist.add_protocol_staker(staker).is_ok());
        assert_eq!(
            whitelist.total_permissioned_protocol_stakers,
            (i + 1) as u16
        );
    }

    // Test that we can't add more user stakers
    let extra_user_staker = Pubkey::new_unique();
    assert!(whitelist.add_user_staker(extra_user_staker).is_err());
    assert_eq!(
        whitelist.total_permissioned_user_stakers,
        MAX_PERMISSIONED_DIRECTED_STAKERS as u16
    );

    // Test that we can't add more protocol stakers
    let extra_protocol_staker = Pubkey::new_unique();
    assert!(whitelist
        .add_protocol_staker(extra_protocol_staker)
        .is_err());
    assert_eq!(
        whitelist.total_permissioned_protocol_stakers,
        MAX_PERMISSIONED_DIRECTED_STAKERS as u16
    );

    // Test can_add methods return false when at capacity
    assert!(!whitelist.can_add_user_staker());
    assert!(!whitelist.can_add_protocol_staker());
    assert!(!whitelist.can_add_staker());
}

#[test]
fn test_cross_category_staker_operations() {
    let mut whitelist = DirectedStakeWhitelist {
        permissioned_user_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_protocol_stakers: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_STAKERS],
        permissioned_validators: [Pubkey::default(); MAX_PERMISSIONED_DIRECTED_VALIDATORS],
        total_permissioned_user_stakers: 0,
        total_permissioned_protocol_stakers: 0,
        total_permissioned_validators: 0,
        _padding0: [0; 250],
    };

    let staker = Pubkey::new_unique();

    // Add as user staker
    whitelist.add_user_staker(staker).unwrap();
    assert!(whitelist.is_user_staker_permissioned(&staker));
    assert!(whitelist.is_staker_permissioned(&staker));

    // Try to add same staker as protocol staker (should fail)
    assert!(whitelist.add_protocol_staker(staker).is_err());

    // Remove from user stakers
    whitelist.remove_user_staker(&staker).unwrap();
    assert!(!whitelist.is_user_staker_permissioned(&staker));
    assert!(!whitelist.is_staker_permissioned(&staker));

    // Now add as protocol staker
    whitelist.add_protocol_staker(staker).unwrap();
    assert!(whitelist.is_protocol_staker_permissioned(&staker));
    assert!(whitelist.is_staker_permissioned(&staker));

    // Try to add same staker as user staker (should fail)
    assert!(whitelist.add_user_staker(staker).is_err());

    // Test generic remove_staker method
    whitelist.remove_staker(&staker).unwrap();
    assert!(!whitelist.is_protocol_staker_permissioned(&staker));
    assert!(!whitelist.is_staker_permissioned(&staker));
}
