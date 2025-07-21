use anchor_lang::error::Error;
use validator_history::constants::TVC_MULTIPLIER;
use validator_history::errors::ValidatorHistoryError;
use validator_history::state::CircBuf;
use validator_history::{
    constants::MAX_STAKE_BUFFER_VALIDATORS, Config, ValidatorHistoryEntry, ValidatorStake,
    ValidatorStakeBuffer,
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
        counter: MAX_STAKE_BUFFER_VALIDATORS as u32,
        ..Default::default()
    };
    buffer.insert(&config, entry).unwrap();
    assert_eq!(buffer.length(), 1);
    assert_eq!(buffer.get_by_index(0).unwrap(), entry);
}

#[test]
fn test_validator_stake_buffer_insert_partially_full_ordered() {
    let mut buffer = ValidatorStakeBuffer::default();
    let config = Config {
        counter: MAX_STAKE_BUFFER_VALIDATORS as u32,
        ..Default::default()
    };
    buffer.insert(&config, ValidatorStake::new(1, 100)).unwrap();
    buffer.insert(&config, ValidatorStake::new(2, 200)).unwrap();
    buffer.insert(&config, ValidatorStake::new(3, 300)).unwrap();
    assert_eq!(buffer.length(), 3);
    assert_eq!(buffer.get_by_index(0).unwrap().stake_amount, 300);
    assert_eq!(buffer.get_by_index(1).unwrap().stake_amount, 200);
    assert_eq!(buffer.get_by_index(2).unwrap().stake_amount, 100);
}

#[test]
fn test_validator_stake_buffer_insert_unordered_in_middle() {
    let mut buffer = ValidatorStakeBuffer::default();
    let config = Config {
        counter: MAX_STAKE_BUFFER_VALIDATORS as u32,
        ..Default::default()
    };
    buffer.insert(&config, ValidatorStake::new(1, 100)).unwrap();
    buffer.insert(&config, ValidatorStake::new(2, 300)).unwrap();
    buffer.insert(&config, ValidatorStake::new(3, 200)).unwrap();
    assert_eq!(buffer.length(), 3);
    assert_eq!(buffer.get_by_index(0).unwrap().stake_amount, 300);
    assert_eq!(buffer.get_by_index(1).unwrap().stake_amount, 200);
    assert_eq!(buffer.get_by_index(2).unwrap().stake_amount, 100);
}

#[test]
fn test_validator_stake_buffer_insert_unordered_at_start() {
    let mut buffer = ValidatorStakeBuffer::default();
    let config = Config {
        counter: MAX_STAKE_BUFFER_VALIDATORS as u32,
        ..Default::default()
    };
    buffer.insert(&config, ValidatorStake::new(1, 100)).unwrap();
    buffer.insert(&config, ValidatorStake::new(2, 300)).unwrap();
    buffer.insert(&config, ValidatorStake::new(3, 50)).unwrap();
    assert_eq!(buffer.length(), 3);
    assert_eq!(buffer.get_by_index(0).unwrap().stake_amount, 300);
    assert_eq!(buffer.get_by_index(1).unwrap().stake_amount, 100);
    assert_eq!(buffer.get_by_index(2).unwrap().stake_amount, 50);
}

#[test]
fn test_validator_stake_buffer_insert_partially_full_unordered() {
    let mut buffer = ValidatorStakeBuffer::default();
    let config = Config {
        counter: MAX_STAKE_BUFFER_VALIDATORS as u32,
        ..Default::default()
    };
    buffer.insert(&config, ValidatorStake::new(1, 300)).unwrap();
    buffer.insert(&config, ValidatorStake::new(2, 100)).unwrap();
    buffer.insert(&config, ValidatorStake::new(3, 200)).unwrap();
    assert_eq!(buffer.length(), 3);
    assert_eq!(buffer.get_by_index(0).unwrap().stake_amount, 300);
    assert_eq!(buffer.get_by_index(1).unwrap().stake_amount, 200);
    assert_eq!(buffer.get_by_index(2).unwrap().stake_amount, 100);
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
    for i in 0..max_len {
        buffer
            .insert(&config, ValidatorStake::new(i, i as u64 + 100))
            .unwrap();
    }
    assert_eq!(buffer.length(), max_len);
    assert!(buffer.is_finalized());

    // Attempt to insert into a full buffer
    let new_entry = ValidatorStake::new(9999, 50);
    let result = buffer.insert(&config, new_entry);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        Error::from(ValidatorHistoryError::StakeBufferFinalized)
    );

    // The buffer should remain unchanged in length and finalized state
    assert_eq!(buffer.length(), max_len);
    assert!(buffer.is_finalized());
    // Verify that the first element is the largest (descending sort)
    assert_eq!(
        buffer.get_by_index(0).unwrap().stake_amount,
        100 + (max_len as u64 - 1)
    );
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
    for i in 0..initial_max_len {
        buffer
            .insert(&initial_config, ValidatorStake::new(i, i as u64 + 100))
            .unwrap();
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
    let result = buffer.insert(&new_config, new_entry);

    // The insertion should still fail because the buffer was finalized with the previous config
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        Error::from(ValidatorHistoryError::StakeBufferFinalized)
    );

    // The buffer should remain unchanged in length and finalized state
    assert_eq!(buffer.length(), initial_max_len);
    assert!(buffer.is_finalized());
    // Verify that the first element is the largest (descending sort)
    assert_eq!(
        buffer.get_by_index(0).unwrap().stake_amount,
        100 + (initial_max_len as u64 - 1)
    );
}

#[test]
fn test_validator_stake_buffer_insert_with_zero_max_len() {
    let mut buffer = ValidatorStakeBuffer::default();
    let config = Config {
        counter: 0,
        ..Default::default()
    };
    let entry = ValidatorStake::new(1, 100);
    let result = buffer.insert(&config, entry);

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
        counter: (MAX_STAKE_BUFFER_VALIDATORS + 1) as u32,
        ..Default::default()
    };
    let entry = ValidatorStake::new(1, 100);
    let result = buffer.insert(&config, entry);

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        Error::from(ValidatorHistoryError::ConfigCounterCeiling)
    );
    // The buffer should remain unchanged
    assert_eq!(buffer.length(), 0);
    assert!(!buffer.is_finalized());
}

#[test]
fn test_get_by_id_zero_total_stake() {
    let mut buffer = ValidatorStakeBuffer::default();
    let config = Config {
        counter: MAX_STAKE_BUFFER_VALIDATORS as u32,
        ..Default::default()
    };
    for i in 0..150 {
        buffer.insert(&config, ValidatorStake::new(i, 0)).unwrap();
    }
    assert_eq!(buffer.length(), 150);
    assert_eq!(buffer.total_stake(), 0);

    let result = buffer.get_by_id(0);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        Error::from(ValidatorHistoryError::StakeBufferEmpty)
    );

    let result = buffer.get_by_id(75);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        Error::from(ValidatorHistoryError::StakeBufferEmpty)
    );
}

#[test]
fn test_get_by_id_superminority_calculation() {
    let mut buffer = ValidatorStakeBuffer::default();
    let config = Config {
        counter: MAX_STAKE_BUFFER_VALIDATORS as u32,
        ..Default::default()
    };
    // Total stake: 3 * 100 + 147 * 1 = 300 + 147 = 447
    // superminority_threshold_stake = 447 / 3 = 149
    // cumulative_stake_at_rank_0 = 100
    // cumulative_stake_at_rank_1 = 200 (so rank 1 is superminority threshold)
    for i in 0..150 {
        let stake = if i < 3 { 100 } else { 1 };
        buffer
            .insert(&config, ValidatorStake::new(i, stake))
            .unwrap();
    }
    assert_eq!(buffer.length(), 150);

    // Test validator at rank 0 (stake 100)
    let (_, rank, is_superminority) = buffer.get_by_id(0).unwrap();
    assert_eq!(rank, 0);
    assert!(is_superminority);

    // Test validator at rank 1 (stake 100)
    let (_, rank, is_superminority) = buffer.get_by_id(1).unwrap();
    assert_eq!(rank, 1);
    assert!(is_superminority);

    // Test validator outside superminority (rank 2, stake 100)
    let (_, rank, is_superminority) = buffer.get_by_id(2).unwrap();
    assert_eq!(rank, 2);
    assert!(!is_superminority);

    // Test validator outside superminority (rank 3, stake 1)
    let (_, rank, is_superminority) = buffer.get_by_id(3).unwrap();
    assert_eq!(rank, 3);
    assert!(!is_superminority);

    // Scenario 2: All validators have equal stake
    // Total stake = 150 * 100 = 15000
    // Threshold = 15000 / 3 = 5000
    // cumulative_stake_at_rank_49 = 50 * 100 = 5000
    // cumulative_stake_at_rank_50 = 51 * 100 = 5100 (so rank 50 is threshold)
    let mut buffer = ValidatorStakeBuffer::default();
    for i in 0..150 {
        buffer.insert(&config, ValidatorStake::new(i, 100)).unwrap();
    }

    // Test validator at rank 49
    let (_, rank, is_superminority) = buffer.get_by_id(49).unwrap();
    assert_eq!(rank, 49);
    assert!(is_superminority);

    // Test validator at rank 50
    let (_, rank, is_superminority) = buffer.get_by_id(50).unwrap();
    assert_eq!(rank, 50);
    assert!(is_superminority);

    // Test validator at rank 51
    let (_, rank, is_superminority) = buffer.get_by_id(51).unwrap();
    assert_eq!(rank, 51);
    assert!(!is_superminority);
}

#[test]
fn test_get_by_id_basic_found_rank_and_stake() {
    let mut buffer = ValidatorStakeBuffer::default();
    let config = Config {
        counter: MAX_STAKE_BUFFER_VALIDATORS as u32,
        ..Default::default()
    };
    // Values are stake_amount, validator_id
    // Rank 0: (1000, 0)
    // Rank 1: (999, 1)
    // ...
    // Rank 100: (900, 100)
    //
    // Superminority cutoff is at rank 47 where cum stake is 46_872
    // Total stake is 138_832 ... 1/3 of that is 46_275
    for i in 0..150 {
        buffer
            .insert(&config, ValidatorStake::new(i, 1000 - i as u64))
            .unwrap();
    }
    assert_eq!(buffer.length(), 150);

    // Test finding validator at rank 0
    let (stake, rank, is_superminority) = buffer.get_by_id(0).unwrap();
    assert_eq!(stake, 1000);
    assert_eq!(rank, 0);
    assert!(is_superminority);

    // Test finding validator in the middle (at threshold)
    let (stake, rank, is_superminority) = buffer.get_by_id(47).unwrap();
    assert_eq!(stake, 1000 - 47);
    assert_eq!(rank, 47);
    assert!(is_superminority);

    // Test finding validator in the middle (after threshold)
    let (stake, rank, is_superminority) = buffer.get_by_id(48).unwrap();
    assert_eq!(stake, 1000 - 48);
    assert_eq!(rank, 48);
    assert!(!is_superminority);

    // Test finding validator at the end of inserted range (after threshold)
    let (stake, rank, is_superminority) = buffer.get_by_id(149).unwrap();
    assert_eq!(stake, 1000 - 149);
    assert_eq!(rank, 149);
    assert!(!is_superminority);
}

#[test]
fn test_get_by_id_validator_not_found() {
    let mut buffer = ValidatorStakeBuffer::default();
    let config = Config {
        counter: MAX_STAKE_BUFFER_VALIDATORS as u32,
        ..Default::default()
    };
    for i in 0..150 {
        buffer
            .insert(&config, ValidatorStake::new(i, 1000 - i as u64))
            .unwrap();
    }
    assert_eq!(buffer.length(), 150);

    // Attempt to get a validator_id that does not exist
    let result = buffer.get_by_id(9999);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        Error::from(ValidatorHistoryError::StakeBufferOutOfBounds)
    );
}
