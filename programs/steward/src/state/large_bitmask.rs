/*
    This file is largely copied over from bitmask.rs
    This is because making a generic bitmask struct either didn't play well with
    zero-copy, or it added too much overhead to a struct meant for performance.
*/

use anchor_lang::{prelude::Result, zero_copy};
use borsh::{BorshDeserialize, BorshSerialize};

use crate::errors::StewardError;

//We are allocating at this size to handle future growth of ValidatorHistory accounts, at 2800 in June 2024
const LARGE_BITMASK_INDEXES: usize = 20_000;

#[allow(clippy::integer_division)]
const LARGE_BITMASK: usize = (LARGE_BITMASK_INDEXES + 64 - 1) / 64; // ceil(LARGE_BITMASK_INDEXES / 64)

/// Data structure used to efficiently pack a binary array, primarily used to store all validators.
/// Each validator has an index (its index in the spl_stake_pool::ValidatorList), corresponding to a bit in the bitmask.
/// When an operation is executed on a validator, the bit corresponding to that validator's index is set to 1.
/// When all bits are 1, the operation is complete.
#[derive(BorshSerialize, BorshDeserialize)]
#[zero_copy]
pub struct LargeBitMask {
    pub values: [u64; LARGE_BITMASK],
}

impl Default for LargeBitMask {
    fn default() -> Self {
        Self {
            values: [0; LARGE_BITMASK],
        }
    }
}

impl LargeBitMask {
    #[allow(clippy::integer_division)]
    pub fn set(&mut self, index: usize, value: bool) -> Result<()> {
        if index >= LARGE_BITMASK_INDEXES {
            return Err(StewardError::BitmaskOutOfBounds.into());
        }
        let word = index / 64;
        let bit = index % 64;
        if value {
            self.values[word] |= 1 << bit;
        } else {
            self.values[word] &= !(1 << bit);
        }
        Ok(())
    }

    #[allow(clippy::integer_division)]
    pub fn get(&self, index: usize) -> Result<bool> {
        if index >= LARGE_BITMASK_INDEXES {
            return Err(StewardError::BitmaskOutOfBounds.into());
        }
        let word = index / 64;
        let bit = index % 64;
        Ok((self.values[word] >> bit) & 1 == 1)
    }

    /// Unsafe version of get, which does not check if the index is out of bounds.
    #[inline]
    #[allow(clippy::integer_division, clippy::arithmetic_side_effects)]
    pub const fn get_unsafe(&self, index: usize) -> bool {
        let word = index / 64;
        let bit = index % 64;
        (self.values[word] >> bit) & 1 == 1
    }

    pub fn reset(&mut self) {
        self.values = [0; LARGE_BITMASK];
    }

    pub fn is_empty(&self) -> bool {
        self.values.iter().all(|&x| x == 0)
    }

    pub fn count(&self) -> usize {
        self.values.iter().map(|x| x.count_ones() as usize).sum()
    }

    #[allow(clippy::integer_division)]
    pub fn is_complete(&self, num_validators: u64) -> Result<bool> {
        let num_validators = num_validators as usize;
        if num_validators > LARGE_BITMASK_INDEXES {
            return Err(StewardError::BitmaskOutOfBounds.into());
        }
        let full_words = num_validators / 64;
        if !self.values[0..full_words].iter().all(|&x| x == u64::MAX) {
            return Ok(false);
        }
        let remainder = num_validators % 64;
        if remainder > 0 {
            let mask: u64 = (1u64 << remainder)
                .checked_sub(1)
                .ok_or(StewardError::ArithmeticError)?;
            if self.values[full_words] & mask != mask {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
