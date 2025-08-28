use crate::utils::U8Bool;
use anchor_lang::prelude::*;
use borsh::BorshSerialize;

pub const MAX_PERMISSIONED_DIRECTED_VALIDATORS: usize = 2048;
pub const MAX_PREFERENCES_PER_TICKET: usize = 128;

#[derive(BorshSerialize)]
#[account(zero_copy)]
struct DirectedStakeMeta {
    epoch: u64,
    total_stake_targets: u16,
    uploaded_stake_targets: u16,
    // 4 bytes required for alignment
    // + 128 bytes reserved for future use
    _padding0: [u8; 132],
    targets: [DirectedStakeTarget; MAX_PERMISSIONED_DIRECTED_VALIDATORS],
}

#[derive(BorshSerialize)]
#[account(zero_copy)]
struct DirectedStakeTarget {
    vote_pubkey: Pubkey,
    total_target_lamports: u128,
    total_applied_lamports: u128,
    // Alignment compliant reserve space for future use
    _padding0: [u8; 64],
}

#[derive(BorshSerialize)]
#[account(zero_copy)]
pub struct DirectedStakePreference {
    pub vote_pubkey: Pubkey,
    /// Percentage of directed stake allocated towards this validator
    pub stake_share_bps: u16,
    pub _padding0: [u8; 94],
}

#[derive(BorshSerialize)]
#[account(zero_copy)]
pub struct DirectedStakeTicket {
    pub directed_stake_type: [DirectedStakePreference; MAX_PREFERENCES_PER_TICKET],
    pub ticket_update_authority: Pubkey,
    pub ticket_close_authority: Pubkey,
    pub active_directed_stake_lamports: u128,
    /// Is the ticket holder a protocol vs. an individual pubkey
    pub ticket_holder_is_protocol: U8Bool,
    // 15 bytes required for alignment
    // + 112 bytes reserved for future use
    pub _padding0: [u8; 127],
}
