use anchor_lang::{prelude::*, zero_copy};
use borsh::BorshSerialize;

use crate::{constants::MAX_STAKE_BUFFER_VALIDATORS, errors::ValidatorHistoryError};

#[allow(clippy::manual_div_ceil)]
#[allow(clippy::identity_op)]
#[allow(clippy::integer_division)]
const BITMASK_SIZE: usize = (MAX_STAKE_BUFFER_VALIDATORS + 64 - 1) / 64;

#[derive(BorshSerialize, Debug, PartialEq)]
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
        if index >= MAX_STAKE_BUFFER_VALIDATORS {
            return Err(ValidatorHistoryError::StakeBufferOutOfBounds.into());
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
        if index >= MAX_STAKE_BUFFER_VALIDATORS {
            return Err(ValidatorHistoryError::StakeBufferOutOfBounds.into());
        }
        let word = index / 64;
        let bit = index % 64;
        Ok((self.values[word] >> bit) & 1 == 1)
    }

    pub fn reset(&mut self) {
        self.values = [0; BITMASK_SIZE];
    }
}
