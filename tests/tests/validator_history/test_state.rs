use anchor_lang::error::Error;
use validator_history::constants::TVC_MULTIPLIER;
use validator_history::errors::ValidatorHistoryError;
use validator_history::state::CircBuf;
use validator_history::{
    constants::MAX_VALIDATORS, ValidatorHistoryEntry, ValidatorStake, ValidatorStakeBuffer,
};

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
fn test_validator_stake_buffer_insert_empty_buffer() {
    let mut buffer = ValidatorStakeBuffer::default();
    let entry = ValidatorStake {
        validator_id: 1,
        stake_amount: 100,
    };
    buffer.insert(entry).unwrap();
    assert_eq!(buffer.length, 1);
    assert_eq!(buffer.buffer[0], entry);
}

#[test]
fn test_validator_stake_buffer_insert_partially_full_ordered() {
    let mut buffer = ValidatorStakeBuffer::default();
    buffer
        .insert(ValidatorStake {
            validator_id: 1,
            stake_amount: 100,
        })
        .unwrap();
    buffer
        .insert(ValidatorStake {
            validator_id: 2,
            stake_amount: 200,
        })
        .unwrap();
    buffer
        .insert(ValidatorStake {
            validator_id: 3,
            stake_amount: 300,
        })
        .unwrap();

    assert_eq!(buffer.length, 3);
    assert_eq!(buffer.buffer[0].stake_amount, 100);
    assert_eq!(buffer.buffer[1].stake_amount, 200);
    assert_eq!(buffer.buffer[2].stake_amount, 300);
}

#[test]
fn test_validator_stake_buffer_insert_partially_full_unordered() {
    let mut buffer = ValidatorStakeBuffer::default();
    buffer
        .insert(ValidatorStake {
            validator_id: 1,
            stake_amount: 300,
        })
        .unwrap();
    buffer
        .insert(ValidatorStake {
            validator_id: 2,
            stake_amount: 100,
        })
        .unwrap();
    buffer
        .insert(ValidatorStake {
            validator_id: 3,
            stake_amount: 200,
        })
        .unwrap();

    assert_eq!(buffer.length, 3);
    assert_eq!(buffer.buffer[0].stake_amount, 100);
    assert_eq!(buffer.buffer[1].stake_amount, 200);
    assert_eq!(buffer.buffer[2].stake_amount, 300);
}

#[test]
fn test_validator_stake_buffer_insert_full_expect_error() {
    let mut buffer = ValidatorStakeBuffer::default();
    for i in 0..MAX_VALIDATORS {
        buffer
            .insert(ValidatorStake {
                validator_id: i as u64,
                stake_amount: i as u64 + 100,
            })
            .unwrap();
    }
    assert_eq!(buffer.length, MAX_VALIDATORS as u64);

    // Attempt to insert a value when the buffer is full.
    let new_entry = ValidatorStake {
        validator_id: 9999,
        stake_amount: 50,
    };
    let result = buffer.insert(new_entry);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        Error::from(ValidatorHistoryError::StakeBufferFull)
    );

    // The buffer should remain unchanged
    assert_eq!(buffer.length, MAX_VALIDATORS as u64);
    // Verify that the first element is still the smallest of the *original* set
    assert_eq!(buffer.buffer[0].stake_amount, 100);
}
