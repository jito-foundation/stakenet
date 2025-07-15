use anchor_lang::error::Error;
use validator_history::constants::TVC_MULTIPLIER;
use validator_history::errors::ValidatorHistoryError;
use validator_history::state::CircBuf;
use validator_history::{
    constants::MAX_VALIDATORS, Config, ValidatorHistoryEntry, ValidatorStake, ValidatorStakeBuffer,
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
    let entry = ValidatorStake::new(1, 100);
    let config = Config {
        counter: MAX_VALIDATORS as u32,
        ..Default::default()
    };
    {
        let mut insert = buffer.insert_builder(&config);
        insert(entry).unwrap();
    }
    assert_eq!(buffer.length(), 1);
    assert_eq!(buffer.get_by_index(0).unwrap(), entry);
}

#[test]
fn test_validator_stake_buffer_insert_partially_full_ordered() {
    let mut buffer = ValidatorStakeBuffer::default();
    let config = Config {
        counter: MAX_VALIDATORS as u32,
        ..Default::default()
    };
    {
        let mut insert = buffer.insert_builder(&config);
        insert(ValidatorStake::new(1, 100)).unwrap();
        insert(ValidatorStake::new(2, 200)).unwrap();
        insert(ValidatorStake::new(3, 300)).unwrap();
    }
    assert_eq!(buffer.length(), 3);
    assert_eq!(buffer.get_by_index(0).unwrap().stake_amount, 100);
    assert_eq!(buffer.get_by_index(1).unwrap().stake_amount, 200);
    assert_eq!(buffer.get_by_index(2).unwrap().stake_amount, 300);
}

#[test]
fn test_validator_stake_buffer_insert_unordered_in_middle() {
    let mut buffer = ValidatorStakeBuffer::default();
    let config = Config {
        counter: MAX_VALIDATORS as u32,
        ..Default::default()
    };
    {
        let mut insert = buffer.insert_builder(&config);
        insert(ValidatorStake::new(1, 100)).unwrap();
        insert(ValidatorStake::new(2, 300)).unwrap();
        insert(ValidatorStake::new(3, 200)).unwrap();
    }
    assert_eq!(buffer.length(), 3);
    assert_eq!(buffer.get_by_index(0).unwrap().stake_amount, 100);
    assert_eq!(buffer.get_by_index(1).unwrap().stake_amount, 200);
    assert_eq!(buffer.get_by_index(2).unwrap().stake_amount, 300);
}

#[test]
fn test_validator_stake_buffer_insert_unordered_at_start() {
    let mut buffer = ValidatorStakeBuffer::default();
    let config = Config {
        counter: MAX_VALIDATORS as u32,
        ..Default::default()
    };
    {
        let mut insert = buffer.insert_builder(&config);
        insert(ValidatorStake::new(1, 100)).unwrap();
        insert(ValidatorStake::new(2, 300)).unwrap();
        insert(ValidatorStake::new(3, 50)).unwrap();
    }
    assert_eq!(buffer.length(), 3);
    assert_eq!(buffer.get_by_index(0).unwrap().stake_amount, 50);
    assert_eq!(buffer.get_by_index(1).unwrap().stake_amount, 100);
    assert_eq!(buffer.get_by_index(2).unwrap().stake_amount, 300);
}

#[test]
fn test_validator_stake_buffer_insert_partially_full_unordered() {
    let mut buffer = ValidatorStakeBuffer::default();
    let config = Config {
        counter: MAX_VALIDATORS as u32,
        ..Default::default()
    };
    {
        let mut insert = buffer.insert_builder(&config);
        insert(ValidatorStake::new(1, 300)).unwrap();
        insert(ValidatorStake::new(2, 100)).unwrap();
        insert(ValidatorStake::new(3, 200)).unwrap();
    }
    assert_eq!(buffer.length(), 3);
    assert_eq!(buffer.get_by_index(0).unwrap().stake_amount, 100);
    assert_eq!(buffer.get_by_index(1).unwrap().stake_amount, 200);
    assert_eq!(buffer.get_by_index(2).unwrap().stake_amount, 300);
}

#[test]
fn test_validator_stake_buffer_finalized_error() {
    let mut buffer = ValidatorStakeBuffer::default();
    let max_len = 10;
    let config = Config {
        counter: max_len,
        ..Default::default()
    };

    // Fill the buffer to max_len
    {
        let mut insert = buffer.insert_builder(&config);
        for i in 0..max_len {
            insert(ValidatorStake::new(i, i as u64 + 100)).unwrap();
        }
    }
    assert_eq!(buffer.length(), max_len);
    assert!(buffer.is_finalized());

    // Attempt to insert into a full buffer
    let new_entry = ValidatorStake::new(9999, 50);
    let mut insert = buffer.insert_builder(&config);
    let result = insert(new_entry);
    drop(insert);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        Error::from(ValidatorHistoryError::StakeBufferFinalized)
    );

    // The buffer should remain unchanged in length and finalized state
    assert_eq!(buffer.length(), max_len);
    assert!(buffer.is_finalized());
    // Verify that the first element is still the smallest of the *original* set
    assert_eq!(buffer.get_by_index(0).unwrap().stake_amount, 100);
}

#[test]
fn test_validator_stake_buffer_finalized_with_monotonically_increasing_config() {
    let mut buffer = ValidatorStakeBuffer::default();
    let initial_max_len = 5;
    let initial_config = Config {
        counter: initial_max_len,
        ..Default::default()
    };

    // Fill the buffer to initial_max_len, which should finalize it
    {
        let mut insert = buffer.insert_builder(&initial_config);
        for i in 0..initial_max_len {
            insert(ValidatorStake::new(i, i as u64 + 100)).unwrap();
        }
    }
    assert_eq!(buffer.length(), initial_max_len);
    assert!(buffer.is_finalized());

    // Create a new config with a monotonically incremented counter
    let new_max_len = initial_max_len + 5;
    let new_config = Config {
        counter: new_max_len,
        ..Default::default()
    };

    // Attempt to insert into the same buffer using a new insert_builder with the new config
    let new_entry = ValidatorStake::new(9999, 50);
    let mut insert_with_new_config = buffer.insert_builder(&new_config);
    let result = insert_with_new_config(new_entry);
    drop(insert_with_new_config);

    // The insertion should still fail because the buffer was finalized with the previous config
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        Error::from(ValidatorHistoryError::StakeBufferFinalized)
    );

    // The buffer should remain unchanged in length and finalized state
    assert_eq!(buffer.length(), initial_max_len);
    assert!(buffer.is_finalized());
    // Verify that the first element is still the smallest of the *original* set
    assert_eq!(buffer.get_by_index(0).unwrap().stake_amount, 100);
}

#[test]
fn test_validator_stake_buffer_insert_with_zero_max_len() {
    let mut buffer = ValidatorStakeBuffer::default();
    let config = Config {
        counter: 0,
        ..Default::default()
    };
    let entry = ValidatorStake::new(1, 100);

    // Attempt to insert with config.counter set to zero (violates validation)
    let mut insert = buffer.insert_builder(&config);
    let result = insert(entry);
    drop(insert);

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        Error::from(ValidatorHistoryError::ConfigCounterFloor)
    );
    // The buffer should remain unchanged
    assert_eq!(buffer.length(), 0);
    assert!(!buffer.is_finalized());
}

#[test]
fn test_validator_stake_buffer_insert_with_counter_greater_than_max_validators() {
    let mut buffer = ValidatorStakeBuffer::default();
    let config = Config {
        counter: (MAX_VALIDATORS + 1) as u32,
        ..Default::default()
    };
    let entry = ValidatorStake::new(1, 100);

    // Attempt to insert with counter greater than max validators
    let mut insert = buffer.insert_builder(&config);
    let result = insert(entry);
    drop(insert);

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        Error::from(ValidatorHistoryError::ConfigCounterCeiling)
    );
    // The buffer should remain unchanged
    assert_eq!(buffer.length(), 0);
    assert!(!buffer.is_finalized());
}
