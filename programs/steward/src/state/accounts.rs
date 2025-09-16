use std::mem::size_of;

use anchor_lang::prelude::*;
use borsh::BorshSerialize;
use type_layout::TypeLayout;

use crate::{parameters::Parameters, utils::U8Bool, LargeBitMask, StewardState};

/* TODO: const CONFIG_SIZE: usize = size_of::<Config>();
const EXPECTED_SIZE: usize = 4040;
assert!(EXPECTED_SIZE == CONFIG_SIZE);*/

/// Config is a user-provided keypair.
/// This is so there can be multiple configs per stake pool, and one party can't
/// squat a config address for another party's stake pool.
#[account(zero_copy)]
#[derive(BorshSerialize, TypeLayout)]
pub struct Config {
    /// SPL Stake Pool address that this program is managing
    pub stake_pool: Pubkey,

    /// Validator List
    pub validator_list: Pubkey,

    /// Admin
    /// - Update the `parameters_authority`
    /// - Update the `blacklist_authority`
    /// - Can call SPL Passthrough functions
    /// - Can pause/reset the state machine
    pub admin: Pubkey,

    /// Parameters Authority
    /// - Can update steward parameters
    pub parameters_authority: Pubkey,

    /// Blacklist Authority
    /// - Can add to the blacklist
    /// - Can remove from the blacklist
    pub blacklist_authority: Pubkey,

    /// Bitmask representing index of validators that are not allowed delegation
    /// NOTE: This is indexed off of the validator history, NOT the validator list
    pub validator_history_blacklist: LargeBitMask,

    /// Parameters for scoring, delegation, and state machine
    pub parameters: Parameters,

    /// Halts any state machine progress
    pub paused: U8Bool,

    /// Required so that the struct is 8-byte aligned
    /// https://doc.rust-lang.org/reference/type-layout.html#reprc-structs
    pub _padding_0: [u8; 7],

    /// The authority that can update the priority fee configs
    pub priority_fee_parameters_authority: Pubkey,

    /// The authority that can update the DirectedStakeMeta for an epoch
    pub directed_stake_meta_upload_authority: Pubkey,

    /// The authority that can invoke lock_reserve and unlock_reserve instructions
    pub reserve_lock_authority: Pubkey,

    /// The authority that can update the directed stake whitelist
    pub directed_stake_whitelist_authority: Pubkey,

    /// Padding for future governance parameters
    pub _padding: [u8; 888],
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

    /// The maximum the average commission could be.
    pub fn max_avg_commission(&self) -> u16 {
        self.parameters
            .priority_fee_max_commission_bps
            .saturating_add(self.parameters.priority_fee_error_margin_bps)
    }

    pub fn priority_fee_epoch_range(&self, current_epoch: u16) -> (u16, u16) {
        let end_epoch: u16 =
            current_epoch.saturating_sub(self.parameters.priority_fee_lookback_offset.into());
        let start_epoch: u16 =
            end_epoch.saturating_sub(self.parameters.priority_fee_lookback_epochs.into());
        (start_epoch, end_epoch)
    }
}

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

pub fn derive_steward_state_address(steward_config: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[StewardStateAccount::SEED, steward_config.as_ref()],
        &crate::id(),
    )
}
