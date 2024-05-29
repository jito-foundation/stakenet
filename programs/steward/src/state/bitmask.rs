use anchor_lang::{prelude::Result, zero_copy};
use borsh::{BorshDeserialize, BorshSerialize};

use crate::{constants::MAX_VALIDATORS, errors::StewardError};

#[allow(clippy::integer_division)]
const BITMASK_SIZE: usize = (MAX_VALIDATORS + 64 - 1) / 64; // ceil(MAX_VALIDATORS / 64)

/// Data structure used to efficiently pack a binary array, primarily used to store all validators.
/// Each validator has an index (its index in the spl_stake_pool::ValidatorList), corresponding to a bit in the bitmask.
/// When an operation is executed on a validator, the bit corresponding to that validator's index is set to 1.
/// When all bits are 1, the operation is complete.
#[derive(BorshSerialize, BorshDeserialize)]
#[zero_copy]
pub struct BitMask {
    pub values: [u64; BITMASK_SIZE],
}

impl Default for BitMask {
    fn default() -> Self {
        Self {
            values: [0; BITMASK_SIZE],
        }
    }
}

impl BitMask {
    #[allow(clippy::integer_division)]
    pub fn set(&mut self, index: usize, value: bool) -> Result<()> {
        if index >= MAX_VALIDATORS {
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
        if index >= MAX_VALIDATORS {
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
        self.values = [0; BITMASK_SIZE];
    }

    pub fn is_empty(&self) -> bool {
        self.values.iter().all(|&x| x == 0)
    }

    pub fn count(&self) -> usize {
        self.values.iter().map(|x| x.count_ones() as usize).sum()
    }

    #[allow(clippy::integer_division)]
    pub fn is_complete(&self, num_validators: usize) -> Result<bool> {
        if num_validators > MAX_VALIDATORS {
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
