use crate::errors::StewardError::{
    AlreadyPermissioned, DirectedStakeStakerListFull, DirectedStakeValidatorListFull,
};
use crate::utils::U8Bool;
use anchor_lang::prelude::*;
use borsh::BorshSerialize;

pub const MAX_PERMISSIONED_DIRECTED_STAKERS: usize = 2048;
pub const MAX_PERMISSIONED_DIRECTED_VALIDATORS: usize = 2048;
pub const MAX_PREFERENCES_PER_TICKET: usize = 128;

#[derive(BorshSerialize)]
#[account(zero_copy)]
struct DirectedStakeMeta {
    /// Epoch for which the target calculations were made for
    pub epoch: u64,
    /// Total number of stake target calculations to be uploaded
    pub total_stake_targets: u16,
    /// Number of stake target calculations uploaded so far
    pub uploaded_stake_targets: u16,
    // 4 bytes required for alignment
    // + 128 bytes reserved for future use
    _padding0: [u8; 132],
    /// Source of truth for directed stake targets for the epoch when
    /// total_stake_targets == uploaded_stake_targets
    pub targets: [DirectedStakeTarget; MAX_PERMISSIONED_DIRECTED_VALIDATORS],
}

#[allow(dead_code)]
impl DirectedStakeMeta {
    pub const SIZE: usize = 8 + size_of::<Self>();
    pub const SEED: &'static [u8] = b"meta";

    /// Returns true if all stake targets have been copied to on-chain accounts
    pub fn is_copy_complete(&self) -> bool {
        self.total_stake_targets == self.uploaded_stake_targets
    }

    /// Get the index of a particular validator in the targets array
    pub fn get_target_index(&self, vote_pubkey: &Pubkey) -> Option<usize> {
        self.targets
            .iter()
            .position(|target| target.vote_pubkey == *vote_pubkey)
    }
}

#[derive(BorshSerialize)]
#[account(zero_copy)]
struct DirectedStakeTarget {
    /// Validator vote pubkey
    pub vote_pubkey: Pubkey,
    /// Total directed stake target for this validator
    pub total_target_lamports: u128,
    /// Total directed stake already applied to this validator
    pub total_applied_lamports: u128,
    // Alignment compliant reserve space for future use
    _padding0: [u8; 64],
}

#[derive(BorshSerialize)]
#[account(zero_copy)]
pub struct DirectedStakePreference {
    /// Validator vote pubkey
    pub vote_pubkey: Pubkey,
    /// Percentage of directed stake allocated towards this validator
    pub stake_share_bps: u16,
    pub _padding0: [u8; 94],
}

#[derive(BorshSerialize)]
#[account(zero_copy)]
pub struct DirectedStakeTicket {
    pub num_preferences: u16,
    /// The sum of staker preferences must be less than or equal to 10_000 bps
    pub staker_preferences: [DirectedStakePreference; MAX_PREFERENCES_PER_TICKET],
    /// Authority that can update the ticket preferences
    pub ticket_update_authority: Pubkey,
    /// Authority that can close the ticket and withdraw remaining lamports
    pub ticket_close_authority: Pubkey,
    /// Is the ticket holder a protocol vs. an individual pubkey
    pub ticket_holder_is_protocol: U8Bool,
    // 15 bytes required for alignment
    // + 110 bytes reserved for future use
    pub _padding0: [u8; 125],
}

impl DirectedStakeTicket {
    pub const SIZE: usize = 8 + size_of::<Self>();
    pub const SEED: &'static [u8] = b"ticket";

    // Check validity of the preferences at initialization and update time
    pub fn preferences_valid(&self) -> bool {
        let total_bps: u16 = self
            .staker_preferences
            .iter()
            .map(|pref| pref.stake_share_bps)
            .sum();
        total_bps <= 10_000
    }

    // This is intended to be called off-chain while computing directed stake meta
    pub fn get_allocations(&self, total_lamports: u64) -> Vec<(Pubkey, u64)> {
        let mut allocations: Vec<(Pubkey, u64)> = Vec::new();
        let mut allocated_lamports: u64 = 0;
        for pref in self
            .staker_preferences
            .iter()
            .take(self.num_preferences as usize)
        {
            let lamports: u64 = (total_lamports as u128)
                .saturating_mul(pref.stake_share_bps as u128)
                .checked_div(10_000)
                .and_then(|v| v.try_into().ok())
                .unwrap_or(0);
            if lamports > 0 {
                allocations.push((pref.vote_pubkey, lamports));
                allocated_lamports = allocated_lamports.saturating_add(lamports);
            }
        }
        allocations
    }
}

#[derive(BorshSerialize)]
#[account(zero_copy)]
pub struct DirectedStakeWhitelist {
    pub permissioned_stakers: [Pubkey; MAX_PERMISSIONED_DIRECTED_STAKERS],
    pub permissioned_validators: [Pubkey; MAX_PERMISSIONED_DIRECTED_VALIDATORS],
    pub total_permissioned_stakers: u16,
    pub total_permissioned_validators: u16,
    // 256 bytes reserved for future use
    pub _padding0: [u8; 256],
}

impl DirectedStakeWhitelist {
    pub const SIZE: usize = 8 + size_of::<Self>();
    pub const SEED: &'static [u8] = b"whitelist";

    pub fn is_staker_permissioned(&self, staker: &Pubkey) -> bool {
        self.permissioned_stakers
            .iter()
            .take(self.total_permissioned_stakers as usize)
            .any(|pk| pk == staker)
    }

    pub fn is_validator_permissioned(&self, validator: &Pubkey) -> bool {
        self.permissioned_validators
            .iter()
            .take(self.total_permissioned_validators as usize)
            .any(|pk| pk == validator)
    }

    pub fn can_add_staker(&self) -> bool {
        (self.total_permissioned_stakers as usize) < MAX_PERMISSIONED_DIRECTED_STAKERS
    }

    pub fn can_add_validator(&self) -> bool {
        (self.total_permissioned_validators as usize) < MAX_PERMISSIONED_DIRECTED_VALIDATORS
    }

    pub fn add_staker(&mut self, staker: Pubkey) -> Result<()> {
        if !self.can_add_staker() {
            return Err(error!(DirectedStakeStakerListFull));
        }
        if self.is_staker_permissioned(&staker) {
            return Err(error!(AlreadyPermissioned));
        }
        self.permissioned_stakers[self.total_permissioned_stakers as usize] = staker;
        self.total_permissioned_stakers += 1;
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
}
