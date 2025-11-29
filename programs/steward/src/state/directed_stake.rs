use std::mem::size_of;

use crate::constants::MAX_VALIDATORS;
use crate::errors::StewardError::{
    AlreadyPermissioned, DirectedStakeValidatorListFull, StakerNotInWhitelist,
    ValidatorNotInWhitelist,
};
use crate::utils::U8Bool;
use anchor_lang::prelude::*;
use borsh::{BorshDeserialize, BorshSerialize};

pub const MAX_PERMISSIONED_DIRECTED_STAKERS: usize = 2048;
pub const MAX_PREFERENCES_PER_TICKET: usize = 8;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq)]
pub enum DirectedStakeRecordType {
    Validator,
    User,
    Protocol,
}

#[derive(BorshSerialize, Debug)]
#[account(zero_copy)]
pub struct DirectedStakeMeta {
    // u64 for alignment, max permissioned validators is much smaller
    pub total_stake_targets: u64,
    pub directed_unstake_total: u64,
    pub padding0: [u8; 63],
    pub is_initialized: U8Bool,
    pub targets: [DirectedStakeTarget; MAX_VALIDATORS],
    // Total staked lamports indexed by validator list index
    pub directed_stake_lamports: [u64; MAX_VALIDATORS],
    pub directed_stake_meta_indices: [u64; MAX_VALIDATORS],
}

impl Default for DirectedStakeMeta {
    fn default() -> Self {
        Self {
            total_stake_targets: 0,
            directed_unstake_total: 0,
            padding0: [0; 63],
            is_initialized: U8Bool::from(true),
            targets: [DirectedStakeTarget::default(); MAX_VALIDATORS],
            directed_stake_lamports: [0; MAX_VALIDATORS],
            directed_stake_meta_indices: [u64::MAX; MAX_VALIDATORS],
        }
    }
}

#[allow(dead_code)]
impl DirectedStakeMeta {
    pub const SIZE: usize = 8 + size_of::<Self>();
    pub const SEED: &'static [u8] = b"meta";
    // Byte position of is_initialized field: 8 (discriminator) + 8*5 (u64 fields) + 62 (padding0) = 110
    pub const IS_INITIALIZED_BYTE_POSITION: usize = 8 + 8 * 5 + 63;

    /// Get the index of a particular validator in the targets array
    pub fn get_target_index(&self, vote_pubkey: &Pubkey) -> Option<usize> {
        for (index, target) in self.targets.iter().enumerate() {
            if &target.vote_pubkey == vote_pubkey {
                return Some(index);
            }
        }
        None
    }

    /// Get the target lamports for a particular validator
    pub fn get_target_lamports(&self, vote_pubkey: &Pubkey) -> Option<u64> {
        self.get_target_index(vote_pubkey)
            .map(|index| self.targets[index].total_target_lamports)
    }

    /// Get the total staked lamports for a particular validator
    pub fn get_total_staked_lamports(&self, vote_pubkey: &Pubkey) -> Option<u64> {
        self.get_target_index(vote_pubkey)
            .map(|index| self.targets[index].total_staked_lamports)
    }

    /// Add to the total staked lamports for a particular validator
    pub fn add_to_total_staked_lamports(&mut self, vote_pubkey_index: usize, lamports: u64) {
        let before = self.targets[vote_pubkey_index].total_staked_lamports;
        let after = before.saturating_add(lamports);
        msg!(
            "add_to_total_staked_lamports[index={}, before={}, +{}, after={}]",
            vote_pubkey_index,
            before,
            lamports,
            after
        );
        self.targets[vote_pubkey_index].total_staked_lamports = after;
    }

    /// Subtract from the total staked lamports for a particular validator
    pub fn subtract_from_total_staked_lamports(&mut self, vote_pubkey_index: usize, lamports: u64) {
        let before = self.targets[vote_pubkey_index].total_staked_lamports;
        let after = before.saturating_sub(lamports);
        msg!(
            "subtract_from_total_staked_lamports[index={}, before={}, -{}, after={}]",
            vote_pubkey_index,
            before,
            lamports,
            after
        );
        self.targets[vote_pubkey_index].total_staked_lamports = after;
    }

    pub fn update_staked_last_updated_epoch(&mut self, vote_pubkey_index: usize, epoch: u64) {
        self.targets[vote_pubkey_index].staked_last_updated_epoch = epoch;
    }

    pub fn all_targets_rebalanced_for_epoch(&self, epoch: u64) -> bool {
        for target in self.targets.iter() {
            if target.vote_pubkey == Pubkey::default() {
                continue;
            }
            if target.staked_last_updated_epoch != epoch {
                return false;
            }
        }
        true
    }

    pub fn total_staked_lamports(&self) -> u64 {
        let mut total: u64 = 0;
        for target in self.targets.iter() {
            total = total.saturating_add(target.total_staked_lamports);
        }
        msg!("DirectedStakeMeta::total_staked_lamports total = {}", total);
        total
    }
}

#[derive(BorshSerialize, Debug, Default)]
#[account(zero_copy)]
pub struct DirectedStakeTarget {
    /// Validator vote pubkey
    pub vote_pubkey: Pubkey,
    /// Total directed stake target for this validator
    pub total_target_lamports: u64,
    /// Total directed stake already applied to this validator
    pub total_staked_lamports: u64,
    /// Last updated epoch for target lamports
    pub target_last_updated_epoch: u64,
    /// Last updated epoch for staked lamports
    pub staked_last_updated_epoch: u64,
    // Alignment compliant reserve space for future use
    pub _padding0: [u8; 32],
}

#[derive(BorshSerialize, BorshDeserialize)]
#[account(zero_copy)]
pub struct DirectedStakePreference {
    /// Validator vote pubkey
    pub vote_pubkey: Pubkey,
    /// Percentage of directed stake allocated towards this validator
    pub stake_share_bps: u16,
    pub _padding0: [u8; 94],
}

impl DirectedStakePreference {
    pub fn new(vote_pubkey: Pubkey, stake_share_bps: u16) -> Self {
        Self {
            vote_pubkey,
            stake_share_bps,
            _padding0: [0; 94],
        }
    }

    pub fn empty() -> Self {
        Self {
            vote_pubkey: Pubkey::default(),
            stake_share_bps: 0,
            _padding0: [0; 94],
        }
    }

    pub fn get_allocation(&self, total_lamports: u64) -> u128 {
        let product = (total_lamports as u128).saturating_mul(self.stake_share_bps as u128);
        let allocation = product.saturating_div(10_000);
        msg!(
            "DirectedStakePreference::get_allocation total_lamports={}, stake_share_bps={}, product={}, allocation={}",
            total_lamports,
            self.stake_share_bps,
            product,
            allocation
        );
        allocation
    }
}

#[derive(BorshSerialize, BorshDeserialize)]
#[account(zero_copy)]
pub struct DirectedStakeTicket {
    pub num_preferences: u16,
    /// The sum of staker preferences must be less than or equal to 10_000 bps
    pub staker_preferences: [DirectedStakePreference; MAX_PREFERENCES_PER_TICKET],
    /// Authority that can update the ticket preferences and close the ticket
    pub ticket_update_authority: Pubkey,
    /// Is the ticket holder a protocol vs. an individual pubkey
    pub ticket_holder_is_protocol: U8Bool,
    // 15 bytes required for alignment
    // + 110 bytes reserved for future use
    pub _padding0: [u8; 125],
}

impl DirectedStakeTicket {
    pub const SIZE: usize = 8 + size_of::<Self>();
    pub const SEED: &'static [u8] = b"ticket";

    pub fn new(
        ticket_update_authority: Pubkey,
        ticket_holder_is_protocol: U8Bool,
        staker_preferences: &[DirectedStakePreference],
    ) -> Self {
        let mut staker_preferences_arr =
            [DirectedStakePreference::empty(); MAX_PREFERENCES_PER_TICKET];
        for (i, preference) in staker_preferences.iter().enumerate() {
            if i < MAX_PREFERENCES_PER_TICKET {
                staker_preferences_arr[i] = *preference;
            }
        }
        Self {
            num_preferences: staker_preferences.len() as u16,
            staker_preferences: staker_preferences_arr,
            ticket_update_authority,
            ticket_holder_is_protocol,
            _padding0: [0; 125],
        }
    }

    // Total allocated bps must be calculated as u32 to avoid overflow
    pub fn preferences_valid(&self) -> bool {
        let total_bps: u32 = self
            .staker_preferences
            .iter()
            .take(self.num_preferences as usize)
            .map(|pref| pref.stake_share_bps as u32)
            .sum();
        msg!(
            "DirectedStakeTicket::preferences_valid total_bps = {} (<= 10000 is valid)",
            total_bps
        );
        total_bps <= 10_000
    }

    // This is intended to be called off-chain while computing directed stake meta
    pub fn get_allocations(&self, total_lamports: u64) -> Vec<(Pubkey, u128)> {
        let mut allocations: Vec<(Pubkey, u128)> = Vec::new();
        for pref in self
            .staker_preferences
            .iter()
            .take(self.num_preferences as usize)
        {
            let lamports: u128 = (total_lamports as u128)
                .saturating_mul(pref.stake_share_bps as u128)
                .saturating_div(10_000);
            if lamports > 0 {
                msg!(
                    "DirectedStakeTicket::get_allocations vote_pubkey={}, stake_share_bps={}, total_lamports={}, allocation_lamports={}",
                    pref.vote_pubkey,
                    pref.stake_share_bps,
                    total_lamports,
                    lamports
                );
                allocations.push((pref.vote_pubkey, lamports));
            }
        }
        allocations
    }
}

#[derive(BorshSerialize, BorshDeserialize)]
#[account(zero_copy)]
pub struct DirectedStakeWhitelist {
    pub permissioned_user_stakers: [Pubkey; MAX_PERMISSIONED_DIRECTED_STAKERS],
    pub permissioned_protocol_stakers: [Pubkey; MAX_PERMISSIONED_DIRECTED_STAKERS],
    pub permissioned_validators: [Pubkey; MAX_VALIDATORS],
    pub total_permissioned_user_stakers: u16,
    pub total_permissioned_protocol_stakers: u16,
    pub total_permissioned_validators: u16,
    // 249 bytes reserved for future use (1 byte used for is_initialized)
    pub _padding0: [u8; 249],
    pub is_initialized: U8Bool,
}

impl DirectedStakeWhitelist {
    pub const SIZE: usize = 8 + size_of::<Self>();
    pub const SEED: &'static [u8] = b"whitelist";
    // Byte position of is_initialized field: 8 (discriminator) + 32*2048*2 (user + protocol stakers) + 32*2048 (validators) + 2*3 (u16 fields) + 248 (padding0) = 196866
    pub const IS_INITIALIZED_BYTE_POSITION: usize = 8
        + 32 * MAX_PERMISSIONED_DIRECTED_STAKERS
        + 32 * MAX_PERMISSIONED_DIRECTED_STAKERS
        + 32 * MAX_VALIDATORS
        + 2
        + 2
        + 2
        + 249;

    pub fn is_user_staker_permissioned(&self, staker: &Pubkey) -> bool {
        self.permissioned_user_stakers
            .iter()
            .take(self.total_permissioned_user_stakers as usize)
            .any(|pk| pk == staker)
    }

    pub fn is_protocol_staker_permissioned(&self, staker: &Pubkey) -> bool {
        self.permissioned_protocol_stakers
            .iter()
            .take(self.total_permissioned_protocol_stakers as usize)
            .any(|pk| pk == staker)
    }

    pub fn is_staker_permissioned(&self, staker: &Pubkey) -> bool {
        self.is_user_staker_permissioned(staker) || self.is_protocol_staker_permissioned(staker)
    }

    pub fn is_validator_permissioned(&self, validator: &Pubkey) -> bool {
        self.permissioned_validators
            .iter()
            .take(self.total_permissioned_validators as usize)
            .any(|pk| pk == validator)
    }

    pub fn can_add_user_staker(&self) -> bool {
        (self.total_permissioned_user_stakers as usize) < MAX_PERMISSIONED_DIRECTED_STAKERS
    }

    pub fn can_add_protocol_staker(&self) -> bool {
        (self.total_permissioned_protocol_stakers as usize) < MAX_PERMISSIONED_DIRECTED_STAKERS
    }

    pub fn can_add_staker(&self) -> bool {
        self.can_add_user_staker() || self.can_add_protocol_staker()
    }

    pub fn can_add_validator(&self) -> bool {
        (self.total_permissioned_validators as usize) < MAX_VALIDATORS
    }

    pub fn add_user_staker(&mut self, staker: Pubkey) -> Result<()> {
        if !self.can_add_user_staker() {
            return Err(error!(AlreadyPermissioned));
        }
        if self.is_staker_permissioned(&staker) {
            return Err(error!(AlreadyPermissioned));
        }
        self.permissioned_user_stakers[self.total_permissioned_user_stakers as usize] = staker;
        self.total_permissioned_user_stakers += 1;
        Ok(())
    }

    pub fn add_protocol_staker(&mut self, staker: Pubkey) -> Result<()> {
        if !self.can_add_protocol_staker() {
            return Err(error!(AlreadyPermissioned));
        }
        if self.is_staker_permissioned(&staker) {
            return Err(error!(AlreadyPermissioned));
        }
        self.permissioned_protocol_stakers[self.total_permissioned_protocol_stakers as usize] =
            staker;
        self.total_permissioned_protocol_stakers += 1;
        Ok(())
    }

    pub fn add_validator(&mut self, validator: Pubkey) -> Result<()> {
        if !self.can_add_validator() {
            return Err(error!(DirectedStakeValidatorListFull));
        }
        if self.is_validator_permissioned(&validator) {
            return Err(error!(DirectedStakeValidatorListFull));
        }
        self.permissioned_validators[self.total_permissioned_validators as usize] = validator;
        self.total_permissioned_validators += 1;
        Ok(())
    }

    pub fn remove_user_staker(&mut self, staker: &Pubkey) -> Result<()> {
        if self.total_permissioned_user_stakers == 0 {
            return Err(error!(StakerNotInWhitelist));
        }

        let mut found_index = None;
        for i in 0..self.total_permissioned_user_stakers as usize {
            if self.permissioned_user_stakers[i] == *staker {
                found_index = Some(i);
                break;
            }
        }

        if let Some(index) = found_index {
            // Shift remaining elements to the left
            for i in index..(self.total_permissioned_user_stakers as usize - 1) {
                self.permissioned_user_stakers[i] = self.permissioned_user_stakers[i + 1];
            }
            // Clear the last element
            self.permissioned_user_stakers[self.total_permissioned_user_stakers as usize - 1] =
                Pubkey::default();
            self.total_permissioned_user_stakers -= 1;
            Ok(())
        } else {
            Err(error!(StakerNotInWhitelist))
        }
    }

    pub fn remove_protocol_staker(&mut self, staker: &Pubkey) -> Result<()> {
        if self.total_permissioned_protocol_stakers == 0 {
            return Err(error!(StakerNotInWhitelist));
        }

        let mut found_index = None;
        for i in 0..self.total_permissioned_protocol_stakers as usize {
            if self.permissioned_protocol_stakers[i] == *staker {
                found_index = Some(i);
                break;
            }
        }

        if let Some(index) = found_index {
            // Shift remaining elements to the left
            for i in index..(self.total_permissioned_protocol_stakers as usize - 1) {
                self.permissioned_protocol_stakers[i] = self.permissioned_protocol_stakers[i + 1];
            }
            // Clear the last element
            self.permissioned_protocol_stakers
                [self.total_permissioned_protocol_stakers as usize - 1] = Pubkey::default();
            self.total_permissioned_protocol_stakers -= 1;
            Ok(())
        } else {
            Err(error!(StakerNotInWhitelist))
        }
    }

    pub fn remove_validator(&mut self, validator: &Pubkey) -> Result<()> {
        if self.total_permissioned_validators == 0 {
            return Err(error!(ValidatorNotInWhitelist));
        }

        let mut found_index = None;
        for i in 0..self.total_permissioned_validators as usize {
            if self.permissioned_validators[i] == *validator {
                found_index = Some(i);
                break;
            }
        }

        if let Some(index) = found_index {
            // Shift remaining elements to the left
            for i in index..(self.total_permissioned_validators as usize - 1) {
                self.permissioned_validators[i] = self.permissioned_validators[i + 1];
            }
            // Clear the last element
            self.permissioned_validators[self.total_permissioned_validators as usize - 1] =
                Pubkey::default();
            self.total_permissioned_validators -= 1;
            Ok(())
        } else {
            Err(error!(ValidatorNotInWhitelist))
        }
    }

    pub fn remove_staker(&mut self, staker: &Pubkey) -> Result<()> {
        // Try to remove from user stakers first, then protocol stakers
        if self.is_user_staker_permissioned(staker) {
            self.remove_user_staker(staker)
        } else if self.is_protocol_staker_permissioned(staker) {
            self.remove_protocol_staker(staker)
        } else {
            Err(error!(StakerNotInWhitelist))
        }
    }
}
