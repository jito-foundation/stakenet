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
    let epoch_u16: u16 = (epoch % u16::MAX as u64).try_into().unwrap();
    Ok(epoch_u16)
}

pub fn get_min_epoch(
    epoch_credits: &[(
        u64, /* epoch */
        u64, /* epoch cumulative votes */
        u64, /* prev epoch cumulative votes */
    )],
) -> Result<u16> {
    cast_epoch(
        epoch_credits
            .iter()
            .min_by_key(|(epoch, _, _)| *epoch)
            .ok_or(ValidatorHistoryError::InvalidEpochCredits)?
            .0,
    )
}

pub fn get_max_epoch(
    epoch_credits: &[(
        u64, /* epoch */
        u64, /* epoch cumulative votes */
        u64, /* prev epoch cumulative votes */
    )],
) -> Result<u16> {
    cast_epoch(
        epoch_credits
            .iter()
            .max_by_key(|(epoch, _, _)| *epoch)
            .ok_or(ValidatorHistoryError::InvalidEpochCredits)?
            .0,
    )
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
    use super::*;

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
