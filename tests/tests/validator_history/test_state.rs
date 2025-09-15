use anchor_lang::prelude::*;
use validator_history::constants::TVC_MULTIPLIER;
use validator_history::state::{CircBuf, ValidatorHistory};
use validator_history::ValidatorHistoryEntry;

const MAX_ITEMS: usize = 512;
#[test]
fn test_normalized_epoch_credits_latest() {
    let mut circ_buf = CircBuf {
        idx: 4,
        is_empty: 0,
        padding: [0; 7],
        arr: [ValidatorHistoryEntry::default(); MAX_ITEMS],
    };

    for i in 0..5 {
        circ_buf.arr[i] = ValidatorHistoryEntry {
            epoch_credits: 1000,
            epoch: i as u16,
            ..ValidatorHistoryEntry::default()
        };
    }

    // Test normalizing an epoch before tvc activation
    assert_eq!(
        circ_buf.epoch_credits_latest_normalized(3, 4),
        Some(1000 * TVC_MULTIPLIER)
    );

    // Test normalizing an epoch after tvc activation
    assert_eq!(circ_buf.epoch_credits_latest_normalized(3, 3), Some(1000));
}

#[test]
fn test_epoch_credits_range_normalized() {
    let mut circ_buf = CircBuf {
        idx: 4,
        is_empty: 0,
        padding: [0; 7],
        arr: [ValidatorHistoryEntry::default(); MAX_ITEMS],
    };

    for i in 0..5 {
        circ_buf.arr[i] = ValidatorHistoryEntry {
            epoch_credits: 1000,
            epoch: i as u16,
            ..ValidatorHistoryEntry::default()
        };
    }

    // Test normalizing epochs before tvc activation
    assert_eq!(
        circ_buf.epoch_credits_range_normalized(0, 4, 5),
        vec![
            Some(1000 * TVC_MULTIPLIER),
            Some(1000 * TVC_MULTIPLIER),
            Some(1000 * TVC_MULTIPLIER),
            Some(1000 * TVC_MULTIPLIER),
            Some(1000 * TVC_MULTIPLIER)
        ]
    );

    // Test normalizing epochs with tvc activation in middle of range
    assert_eq!(
        circ_buf.epoch_credits_range_normalized(0, 4, 2),
        vec![
            Some(1000 * TVC_MULTIPLIER),
            Some(1000 * TVC_MULTIPLIER),
            Some(1000),
            Some(1000),
            Some(1000)
        ]
    );
}

#[test]
fn test_validator_age_tracking_with_gaps() {
    // Test that validator age correctly counts multiple epochs when there's a gap between updates
    let mut validator_history = ValidatorHistory {
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

    // Add epochs 100, 101, 102 with credits
    for epoch in 100..=102 {
        validator_history.history.push(ValidatorHistoryEntry {
            epoch,
            epoch_credits: 1000,
            ..ValidatorHistoryEntry::default()
        });
    }

    // Initialize validator age at epoch 102
    validator_history.update_validator_age(102).unwrap();
    assert_eq!(validator_history.validator_age, 3);
    assert_eq!(validator_history.validator_age_last_updated_epoch, 102);

    // Add epochs 105, 106, 107 with credits (gap of 2 epochs)
    for epoch in 105..=107 {
        validator_history.history.push(ValidatorHistoryEntry {
            epoch,
            epoch_credits: 1000,
            ..ValidatorHistoryEntry::default()
        });
    }

    // Update at epoch 107 - should count epochs 105, 106, 107
    validator_history.update_validator_age(107).unwrap();
    assert_eq!(validator_history.validator_age, 6); // 3 + 3 new epochs
    assert_eq!(validator_history.validator_age_last_updated_epoch, 107);
}

#[test]
fn test_validator_age_mixed_credits() {
    // Test with some epochs having credits and some not
    let mut validator_history = ValidatorHistory {
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

    // Add epochs with varying credits
    validator_history.history.push(ValidatorHistoryEntry {
        epoch: 100,
        epoch_credits: 1000, // Has credits
        ..ValidatorHistoryEntry::default()
    });
    validator_history.history.push(ValidatorHistoryEntry {
        epoch: 101,
        epoch_credits: 0, // No credits
        ..ValidatorHistoryEntry::default()
    });
    validator_history.history.push(ValidatorHistoryEntry {
        epoch: 102,
        epoch_credits: 500, // Has credits
        ..ValidatorHistoryEntry::default()
    });
    validator_history.history.push(ValidatorHistoryEntry {
        epoch: 103,
        epoch_credits: u32::MAX, // Invalid credits (not set)
        ..ValidatorHistoryEntry::default()
    });
    validator_history.history.push(ValidatorHistoryEntry {
        epoch: 104,
        epoch_credits: 2000, // Has credits
        ..ValidatorHistoryEntry::default()
    });

    // Update at epoch 104
    validator_history.update_validator_age(104).unwrap();
    // Should count epochs 100, 102, 104 (3 epochs with valid credits > 0)
    assert_eq!(validator_history.validator_age, 3);
    assert_eq!(validator_history.validator_age_last_updated_epoch, 104);
}

#[test]
fn test_validator_age_idempotent() {
    // Test that calling update multiple times in same epoch is idempotent
    let mut validator_history = ValidatorHistory {
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

    // Add epoch 100 with credits
    validator_history.history.push(ValidatorHistoryEntry {
        epoch: 100,
        epoch_credits: 1000,
        ..ValidatorHistoryEntry::default()
    });

    // First update
    validator_history.update_validator_age(100).unwrap();
    assert_eq!(validator_history.validator_age, 1);

    // Second update at same epoch - should be no-op
    validator_history.update_validator_age(100).unwrap();
    assert_eq!(validator_history.validator_age, 1);

    // Try updating with an earlier epoch - should also be no-op
    validator_history.update_validator_age(99).unwrap();
    assert_eq!(validator_history.validator_age, 1);
    assert_eq!(validator_history.validator_age_last_updated_epoch, 100);
}

#[test]
fn test_validator_age_wraparound() {
    // Test with circular buffer wraparound
    let mut validator_history = ValidatorHistory {
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

    // Fill buffer to cause wraparound
    for epoch in 0..600u16 {
        validator_history.history.push(ValidatorHistoryEntry {
            epoch,
            epoch_credits: if epoch % 2 == 0 { 1000 } else { 0 },
            ..ValidatorHistoryEntry::default()
        });
    }

    // Update at epoch 599
    validator_history.update_validator_age(599).unwrap();
    // Should count all even epochs from 0 to 598 (300 epochs)
    // But buffer only holds 512 entries, so it wraps around
    // Epochs 88-599 are in buffer, even epochs from 88-598 = 256 epochs
    let even_epochs_in_range = (88..=598).filter(|e| e % 2 == 0).count();
    assert_eq!(validator_history.validator_age, even_epochs_in_range as u32);
}

#[test]
fn test_validator_age_backwards_iteration() {
    // Test that the backwards iteration optimization works correctly
    let mut validator_history = ValidatorHistory {
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

    // Add some epochs with credits
    for epoch in [100, 101, 102, 105, 110].iter() {
        validator_history.history.push(ValidatorHistoryEntry {
            epoch: *epoch,
            epoch_credits: 1000,
            ..ValidatorHistoryEntry::default()
        });
    }

    // Initialize at epoch 102
    validator_history.update_validator_age(102).unwrap();
    // When initializing, it counts all epochs with credits in the buffer
    // which includes epochs 100, 101, 102, 105, 110 (all 5 epochs)
    assert_eq!(validator_history.validator_age, 5); // All 5 epochs in buffer

    // Update at epoch 110 - should be no-op since 110 is already counted
    validator_history.update_validator_age(110).unwrap();
    assert_eq!(validator_history.validator_age, 5); // Still 5, no new epochs
    assert_eq!(validator_history.validator_age_last_updated_epoch, 110);
}
