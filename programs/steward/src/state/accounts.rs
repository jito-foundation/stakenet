use std::mem::size_of;

use anchor_lang::prelude::*;
use borsh::BorshSerialize;
use spl_pod::primitives::PodU16;
use type_layout::TypeLayout;

use crate::{parameters::Parameters, utils::U8Bool, LargeBitMask, StewardState};

/// Config is a user-provided keypair.
/// This is so there can be multiple configs per stake pool, and one party can't
/// squat a config address for another party's stake pool.
#[account(zero_copy)]
#[derive(TypeLayout)]
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

    /// The epoch after which validators must be using TipRouter upload authority for tip
    /// distribution
    pub tip_router_upload_auth_epoch_cutoff: PodU16,

    /// Padding for future governance parameters
    pub _padding: [u8; 1021],
}

impl BorshSerialize for Config {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        self.stake_pool.serialize(writer)?;
        self.validator_list.serialize(writer)?;
        self.admin.serialize(writer)?;
        self.parameters_authority.serialize(writer)?;
        self.blacklist_authority.serialize(writer)?;
        self.validator_history_blacklist.serialize(writer)?;
        self.parameters.serialize(writer)?;
        self.paused.serialize(writer)?;
        let cutoff: u16 = self.tip_router_upload_auth_epoch_cutoff.into();
        cutoff.serialize(writer)?;
        self._padding.serialize(writer)
    }
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
