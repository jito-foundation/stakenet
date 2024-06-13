use std::mem::size_of;

use anchor_lang::prelude::*;
use borsh::BorshSerialize;
use type_layout::TypeLayout;

use crate::{bitmask::BitMask, parameters::Parameters, utils::U8Bool, StewardState};

/// Config is a user-provided keypair.
/// This is so there can be multiple configs per stake pool, and one party can't
/// squat a config address for another party's stake pool.
#[account(zero_copy)]
#[derive(BorshSerialize, TypeLayout)]
pub struct Config {
    /// SPL Stake Pool address that this program is managing
    pub stake_pool: Pubkey,

    /// Authority for pool stewardship, can execute SPL Staker commands and adjust Delegation parameters
    pub authority: Pubkey,

    /// Bitmask representing index of validators that are not allowed delegation
    pub blacklist: BitMask,

    /// Parameters for scoring, delegation, and state machine
    pub parameters: Parameters,

    /// Padding for future governance parameters
    pub _padding: [u8; 1023],

    /// Halts any state machine progress
    pub paused: U8Bool,
}

impl Config {
    pub const SIZE: usize = 8 + size_of::<Self>();
    pub const SEED: &'static [u8] = b"config";

    pub fn is_paused(&self) -> bool {
        self.paused.into()
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused.into();
    }
}

// PDA that is used to sign instructions for the stake pool.
// The pool's "staker" account needs to be assigned to this address,
// and it has authority over adding validators, removing validators, and delegating stake.
#[account]
pub struct Staker {
    pub bump: u8,
}
impl Staker {
    pub const SIZE: usize = 8 + size_of::<Self>();
    pub const SEED: &'static [u8] = b"staker";

    pub fn get_address(config: &Pubkey) -> Pubkey {
        let (pubkey, _) =
            Pubkey::find_program_address(&[Self::SEED, config.as_ref()], &crate::id());
        pubkey
    }
}

// static_assertions::const_assert_eq!(StewardStateAccount::SIZE, 162584);

#[derive(BorshSerialize)]
#[account(zero_copy)]
pub struct StewardStateAccount {
    pub state: StewardState,
    pub is_initialized: U8Bool,
    pub bump: u8,
    pub _padding: [u8; 6],
}

impl StewardStateAccount {
    pub const SIZE: usize = 8 + size_of::<Self>();
    pub const SEED: &'static [u8] = b"steward_state";
    pub const IS_INITIALIZED_BYTE_POSITION: usize = Self::SIZE - 8;
}
