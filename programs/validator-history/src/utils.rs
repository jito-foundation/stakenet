use std::collections::HashMap;

use anchor_lang::{
    prelude::{AccountInfo, Pubkey, Result},
    require,
    solana_program::native_token::lamports_to_sol,
};

use crate::{errors::ValidatorHistoryError, ValidatorHistoryEntry};

pub fn cast_epoch(epoch: u64) -> Result<u16> {
    require!(
        epoch < (u16::MAX as u64),
        ValidatorHistoryError::EpochTooLarge
    );
    Ok(epoch as u16)
}

/// Largest value stored in `ValidatorHistoryEntry::epoch_credits`.
///
/// `u32::MAX` means unset to every reader, so values saturate one below it. Tower credits never
/// get close, but Alpenglow reward lamports do, and landing on `u32::MAX` would make a validator
/// read as not voting (delinquent, and flagged for instant unstake).
pub const MAX_EPOCH_CREDITS: u32 = u32::MAX - 1;

/// Credits earned per epoch, derived from a vote account's raw `epoch_credits`.
///
/// Values are uncapped, so they must be capped at `MAX_EPOCH_CREDITS` before being stored in
/// `ValidatorHistoryEntry::epoch_credits`.
///
/// Epoch credits
/// 0. epoch
/// 1. epoch cumulative votes
/// 2. prev epoch cumulative votes
pub fn epoch_credits_map(epoch_credits: &[(u64, u64, u64)]) -> Result<HashMap<u16, u64>> {
    let mut credits_by_epoch: HashMap<u16, u64> = HashMap::with_capacity(epoch_credits.len());
    for (epoch, cur, prev) in epoch_credits.iter() {
        if *epoch >= u16::MAX as u64 {
            continue;
        }
        let credits = cur
            .checked_sub(*prev)
            .ok_or(ValidatorHistoryError::InvalidEpochCredits)?;
        credits_by_epoch
            .entry(*epoch as u16)
            .and_modify(|entry| *entry = entry.saturating_add(credits))
            .or_insert(credits);
    }
    Ok(credits_by_epoch)
}

pub fn get_min_epoch(
    epoch_credits: &[(
        u64, /* epoch */
        u64, /* epoch cumulative votes */
        u64, /* prev epoch cumulative votes */
    )],
) -> Result<u16> {
    epoch_credits
        .iter()
        .filter_map(|(epoch, _, _)| {
            if *epoch < u16::MAX as u64 {
                Some(*epoch as u16)
            } else {
                None
            }
        })
        .min()
        .ok_or_else(|| ValidatorHistoryError::InvalidEpochCredits.into())
}

pub fn get_max_epoch(
    epoch_credits: &[(
        u64, /* epoch */
        u64, /* epoch cumulative votes */
        u64, /* prev epoch cumulative votes */
    )],
) -> Result<u16> {
    epoch_credits
        .iter()
        .filter_map(|(epoch, _, _)| {
            if *epoch < u16::MAX as u64 {
                Some(*epoch as u16)
            } else {
                None
            }
        })
        .max()
        .ok_or_else(|| ValidatorHistoryError::InvalidEpochCredits.into())
}

pub fn cast_epoch_start_timestamp(start_timestamp: i64) -> u64 {
    start_timestamp.try_into().unwrap()
}

pub fn fixed_point_sol(lamports: u64) -> u32 {
    // convert to sol
    let mut sol = lamports_to_sol(lamports);
    // truncate to 2 decimal points by rounding up, technically we can combine this line and the next
    sol = f64::round(sol * 100.0) / 100.0;
    // return a 4byte unsigned fixed point number with a 1/100 scaling factor
    // this will internally represent a max value of 42949672.95 SOL
    (sol * 100.0) as u32
}

pub fn get_vote_account(validator_history_account_info: &AccountInfo) -> Pubkey {
    let pubkey_bytes = &validator_history_account_info.data.borrow()[8..32 + 8];
    let mut data = [0; 32];
    data.copy_from_slice(pubkey_bytes);
    Pubkey::from(data)
}

/// Finds the position to insert a new entry with the given epoch, where the epoch is greater than the previous entry and less than the next entry.
/// Assumes entries are in sorted order (according to CircBuf ordering), and there are no duplicate epochs.
pub fn find_insert_position(
    arr: &[ValidatorHistoryEntry],
    idx: usize,
    epoch: u16,
) -> Option<usize> {
    let len = arr.len();
    if len == 0 {
        return None;
    }

    let insert_pos =
        if idx != len - 1 && arr[idx + 1].epoch == ValidatorHistoryEntry::default().epoch {
            // If the circ buf still has default values in it, we do a normal binary search without factoring for wraparound.
            let len = idx + 1;
            let mut left = 0;
            let mut right = len;
            while left < right {
                let mid = (left + right) / 2;
                match arr[mid].epoch.cmp(&epoch) {
                    std::cmp::Ordering::Equal => return None,
                    std::cmp::Ordering::Less => left = mid + 1,
                    std::cmp::Ordering::Greater => right = mid,
                }
            }
            left % arr.len()
        } else {
            // Binary search with wraparound
            let mut left = 0;
            let mut right = len;
            while left < right {
                let mid = (left + right) / 2;
                // idx + 1 is the index of the smallest epoch in the array
                let mid_idx = ((idx + 1) + mid) % len;
                match arr[mid_idx].epoch.cmp(&epoch) {
                    std::cmp::Ordering::Equal => return None,
                    std::cmp::Ordering::Less => left = mid + 1,
                    std::cmp::Ordering::Greater => right = mid,
                }
            }
            ((idx + 1) + left) % len
        };
    if arr[insert_pos].epoch == epoch {
        return None;
    }
    Some(insert_pos)
}

#[cfg(test)]
mod tests {
    use validator_history_vote_state::AG_MIGRATION_EPOCH_CREDIT;

    use super::*;

    /// What a vote account looks like mid alpenglow migration: epoch 71 is split into a tower era
    /// entry and an alpenglow era entry, separated by the marker.
    fn migrating_epoch_credits() -> Vec<(u64, u64, u64)> {
        vec![
            (70, 9, 6),
            (71, 20, 9),
            AG_MIGRATION_EPOCH_CREDIT,
            (71, 35, 20),
            (72, 50, 35),
        ]
    }

    #[test]
    fn test_epoch_credits_map_handles_migration() {
        let map = epoch_credits_map(&migrating_epoch_credits()).unwrap();

        assert_eq!(map.len(), 3);
        assert_eq!(map[&70], 3);

        assert_eq!(map[&71], 11 + 15);
        assert_eq!(map[&72], 15);
    }

    #[test]
    fn test_epoch_credits_map_rejects_decreasing_credits() {
        assert!(epoch_credits_map(&[(70, 6, 9)]).is_err());
    }

    #[test]
    fn test_epoch_credits_map_does_not_cap_alpenglow_lamports() {
        // 300 SOL of vote rewards, well past what `ValidatorHistoryEntry::epoch_credits` can hold
        let lamports = 300_000_000_000;
        let map = epoch_credits_map(&[(72, 1_000 + lamports, 1_000)]).unwrap();

        assert_eq!(map[&72], lamports);
        assert!(map[&72] > u64::from(MAX_EPOCH_CREDITS));
    }

    #[test]
    fn test_min_max_epoch_skip_migration_marker() {
        let epoch_credits = migrating_epoch_credits();
        assert_eq!(get_min_epoch(&epoch_credits).unwrap(), 70);
        assert_eq!(get_max_epoch(&epoch_credits).unwrap(), 72);

        assert!(get_min_epoch(&[AG_MIGRATION_EPOCH_CREDIT]).is_err());
        assert!(get_max_epoch(&[]).is_err());
    }

    #[test]
    fn test_fixed_point_sol() {
        assert_eq!(fixed_point_sol(1_000_000_000), 100);
        assert_eq!(fixed_point_sol(4_294_967_295_000_000_000), 4294967295);

        assert_eq!(fixed_point_sol(429_496_729_600_000_000), 4294967295)
    }

    #[test]
    fn test_find_insert_position() {
        // Test empty
        let arr = vec![];
        assert_eq!(find_insert_position(&arr, 0, 5), None);

        // Test single element
        let arr = vec![ValidatorHistoryEntry {
            epoch: 10,
            ..Default::default()
        }];
        assert_eq!(find_insert_position(&arr, 0, 5), Some(0));
        assert_eq!(find_insert_position(&arr, 0, 15), Some(0));

        // Test multiple elements
        let arr = vec![
            ValidatorHistoryEntry {
                epoch: 5,
                ..Default::default()
            },
            ValidatorHistoryEntry {
                epoch: 10,
                ..Default::default()
            },
            ValidatorHistoryEntry {
                epoch: 15,
                ..Default::default()
            },
            ValidatorHistoryEntry {
                epoch: 20,
                ..Default::default()
            },
            ValidatorHistoryEntry::default(),
            ValidatorHistoryEntry::default(),
            ValidatorHistoryEntry::default(),
        ];

        let idx = 3;
        assert_eq!(find_insert_position(&arr, idx, 0), Some(0));
        assert_eq!(find_insert_position(&arr, idx, 12), Some(2));
        assert_eq!(find_insert_position(&arr, idx, 25), Some(4));

        // Test wraparound
        let arr = vec![
            ValidatorHistoryEntry {
                epoch: 15,
                ..Default::default()
            },
            ValidatorHistoryEntry {
                epoch: 20,
                ..Default::default()
            },
            ValidatorHistoryEntry {
                epoch: 25,
                ..Default::default()
            },
            ValidatorHistoryEntry {
                epoch: 5,
                ..Default::default()
            },
            ValidatorHistoryEntry {
                epoch: 10,
                ..Default::default()
            },
        ];

        let idx = 2;
        assert_eq!(find_insert_position(&arr, idx, 0), Some(3));
        assert_eq!(find_insert_position(&arr, idx, 12), Some(0));
        assert_eq!(find_insert_position(&arr, idx, 17), Some(1));
        assert_eq!(find_insert_position(&arr, idx, 22), Some(2));

        // Test duplicate
        assert_eq!(find_insert_position(&arr, idx, 10), None);
    }
}
