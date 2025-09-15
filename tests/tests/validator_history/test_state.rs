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
fn test_validator_age_current_epoch_pending() {
    // Test that current epoch with 0 credits remains "pending" and can be counted later
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

    // Add epochs 100, 101 with credits
    validator_history.history.push(ValidatorHistoryEntry {
        epoch: 100,
        epoch_credits: 1000,
        ..ValidatorHistoryEntry::default()
    });
    validator_history.history.push(ValidatorHistoryEntry {
        epoch: 101,
        epoch_credits: 1000,
        ..ValidatorHistoryEntry::default()
    });

    // Initialize at epoch 101
    validator_history.update_validator_age(101).unwrap();
    assert_eq!(validator_history.validator_age, 2);
    assert_eq!(validator_history.validator_age_last_updated_epoch, 101);

    // Add epoch 102 with 0 credits initially (simulating early in the epoch)
    validator_history.history.push(ValidatorHistoryEntry {
        epoch: 102,
        epoch_credits: 0,
        ..ValidatorHistoryEntry::default()
    });

    // Update at epoch 102 with 0 credits - should not advance checkpoint to 102
    validator_history.update_validator_age(102).unwrap();
    assert_eq!(validator_history.validator_age, 2); // No new epochs counted
    assert_eq!(validator_history.validator_age_last_updated_epoch, 101); // Checkpoint stays at 101

    // Simulate epoch 102 gaining credits later
    // Find and update the epoch 102 entry
    for entry in validator_history.history.arr.iter_mut() {
        if entry.epoch == 102 {
            entry.epoch_credits = 1500;
            break;
        }
    }

    // Update again at epoch 102 - now it should count
    validator_history.update_validator_age(102).unwrap();
    assert_eq!(validator_history.validator_age, 3); // Now epoch 102 is counted
    assert_eq!(validator_history.validator_age_last_updated_epoch, 102); // Checkpoint advances

    // Verify idempotency - calling again shouldn't change anything
    validator_history.update_validator_age(102).unwrap();
    assert_eq!(validator_history.validator_age, 3);
    assert_eq!(validator_history.validator_age_last_updated_epoch, 102);
}

#[test]
fn test_validator_age_multiple_pending_epochs() {
    // Test handling multiple epochs that start with 0 credits
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

    // Initialize
    validator_history.update_validator_age(100).unwrap();
    assert_eq!(validator_history.validator_age, 1);
    assert_eq!(validator_history.validator_age_last_updated_epoch, 100);

    // Add epochs 101, 102, 103 all with 0 credits initially
    for epoch in 101..=103 {
        validator_history.history.push(ValidatorHistoryEntry {
            epoch,
            epoch_credits: 0,
            ..ValidatorHistoryEntry::default()
        });
    }

    // Update at epoch 103 - no new epochs should be counted
    // Checkpoint advances to 102 (epoch-1) to mark epochs 101-102 as processed
    // Epoch 103 remains pending since it's the current epoch with 0 credits
    validator_history.update_validator_age(103).unwrap();
    assert_eq!(validator_history.validator_age, 1);
    assert_eq!(validator_history.validator_age_last_updated_epoch, 102);

    // Epoch 101 gains credits (but this is after we've already processed it)
    for entry in validator_history.history.arr.iter_mut() {
        if entry.epoch == 101 {
            entry.epoch_credits = 500;
            break;
        }
    }

    // Update - epoch 101 won't be counted because it's already been processed
    // Checkpoint stays at 102
    validator_history.update_validator_age(103).unwrap();
    assert_eq!(validator_history.validator_age, 1); // Still 1, epoch 101 was already processed
    assert_eq!(validator_history.validator_age_last_updated_epoch, 102);

    // Epoch 103 gains credits (skipping 102)
    for entry in validator_history.history.arr.iter_mut() {
        if entry.epoch == 103 {
            entry.epoch_credits = 800;
            break;
        }
    }

    // Update - should count epoch 103 now (total: 100 and 103)
    validator_history.update_validator_age(103).unwrap();
    assert_eq!(validator_history.validator_age, 2); // epochs 100 and 103
    assert_eq!(validator_history.validator_age_last_updated_epoch, 103);

    // Finally epoch 102 gains credits (but it's already been processed)
    for entry in validator_history.history.arr.iter_mut() {
        if entry.epoch == 102 {
            entry.epoch_credits = 600;
            break;
        }
    }

    // Move to epoch 104 to finalize everything
    validator_history.history.push(ValidatorHistoryEntry {
        epoch: 104,
        epoch_credits: 1000,
        ..ValidatorHistoryEntry::default()
    });

    validator_history.update_validator_age(104).unwrap();
    // Should have counted epochs 100, 103, 104 (epochs 101 and 102 were processed with 0 credits)
    assert_eq!(validator_history.validator_age, 3);
    assert_eq!(validator_history.validator_age_last_updated_epoch, 104);
}
