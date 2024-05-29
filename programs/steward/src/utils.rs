use std::ops::{Deref, Not};

use anchor_lang::prelude::*;
use borsh::{BorshDeserialize, BorshSerialize};
use spl_pod::{bytemuck::pod_from_bytes, primitives::PodU64, solana_program::program_pack::Pack};
use spl_stake_pool::{
    big_vec::BigVec,
    state::{ValidatorListHeader, ValidatorStakeInfo},
};

use crate::{errors::StewardError, Config, Delegation};

pub fn get_stake_pool(account: &AccountLoader<Config>) -> Result<Pubkey> {
    let config = account.load()?;
    Ok(config.stake_pool)
}

pub fn get_config_authority(account: &AccountLoader<Config>) -> Result<Pubkey> {
    let config = account.load()?;
    Ok(config.authority)
}

pub fn epoch_progress(clock: &Clock, epoch_schedule: &EpochSchedule) -> Result<f64> {
    let current_epoch = clock.epoch;
    let current_slot = clock.slot;
    let slots_in_epoch = epoch_schedule.slots_per_epoch;
    let slot_index = current_slot
        .checked_sub(epoch_schedule.get_first_slot_in_epoch(current_epoch))
        .ok_or(StewardError::ArithmeticError)?;
    Ok(slot_index as f64 / slots_in_epoch as f64)
}

/// Safely gets the target lamports for a validator based on the delegation and current stake pool lamports.
/// stake_pool_lamports should have all stake accounts' base_lamports removed, since those are immovable.
/// Note: Loses precision up to `denominator` lamports, which is acceptable
pub fn get_target_lamports(delegation: &Delegation, stake_pool_lamports: u64) -> Result<u64> {
    stake_pool_lamports
        .checked_mul(delegation.numerator as u64)
        .and_then(|x| x.checked_div(delegation.denominator as u64))
        .ok_or_else(|| StewardError::ArithmeticError.into())
}

const VEC_SIZE_BYTES: usize = 4;

/// Utility to efficiently extract stake lamports and transient stake from a validator list.
/// Frankenstein of spl_stake_pool::big_vec::BigVec::deserialize_slice
/// and spl_stake_pool::state::ValidatorStakeInfo::active_lamports_greater_than
#[inline(always)]
pub fn stake_lamports_at_validator_list_index(
    validator_list: &BigVec<'_>,
    index: usize,
) -> Result<(u64, bool)> {
    let active_start_index =
        VEC_SIZE_BYTES.saturating_add(index.saturating_mul(ValidatorStakeInfo::LEN));
    let active_end_index = active_start_index
        .checked_add(8)
        .ok_or(StewardError::ArithmeticError)?;
    let transient_start_index = active_end_index;
    let transient_end_index = transient_start_index
        .checked_add(8)
        .ok_or(StewardError::ArithmeticError)?;
    let slice = &validator_list.data[active_start_index..active_end_index];
    let active_stake_lamport_pod = pod_from_bytes::<PodU64>(slice).unwrap();
    let slice = &validator_list.data[transient_start_index..transient_end_index];
    let some_transient_stake = u64::from(*pod_from_bytes::<PodU64>(slice).unwrap()) != 0;
    Ok((u64::from(*active_stake_lamport_pod), some_transient_stake))
}

pub fn get_validator_stake_info_at_index(
    validator_list_account_info: &AccountInfo,
    validator_list_index: usize,
) -> Result<ValidatorStakeInfo> {
    let mut validator_list_data = validator_list_account_info.try_borrow_mut_data()?;
    let (header, mut validator_list) =
        ValidatorListHeader::deserialize_vec(&mut validator_list_data)?;
    require!(
        header.account_type == spl_stake_pool::state::AccountType::ValidatorList,
        StewardError::ValidatorListTypeMismatch
    );
    let validator_stake_info_slice =
        ValidatorListHeader::deserialize_mut_slice(&mut validator_list, validator_list_index, 1)?;
    let validator_stake_info = *validator_stake_info_slice
        .first()
        .ok_or(StewardError::ValidatorNotInList)?;

    Ok(validator_stake_info)
}

/// A boolean type stored as a u8.
#[derive(BorshSerialize, BorshDeserialize, Debug, PartialEq, Eq)]
#[zero_copy]
pub struct U8Bool {
    value: u8,
}

impl U8Bool {
    const fn is_true(self) -> bool {
        self.value != 0
    }
}

impl Not for U8Bool {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self {
            value: (!self.value) & 1,
        }
    }
}

impl From<bool> for U8Bool {
    fn from(val: bool) -> Self {
        Self { value: val as u8 }
    }
}

impl From<U8Bool> for bool {
    fn from(val: U8Bool) -> Self {
        val.is_true()
    }
}

#[derive(Clone)]
pub struct StakePool(spl_stake_pool::state::StakePool);

impl AsRef<spl_stake_pool::state::StakePool> for StakePool {
    fn as_ref(&self) -> &spl_stake_pool::state::StakePool {
        &self.0
    }
}

// This is necessary so we can use "anchor_spl::token::Mint::LEN"
// because rust does not resolve "anchor_spl::token::Mint::LEN" to
// "spl_token::state::Mint::LEN" automatically
impl StakePool {
    pub const LEN: usize = std::mem::size_of::<spl_stake_pool::state::StakePool>();
}

// You don't have to implement the "try_deserialize" function
// from this trait. It delegates to
// "try_deserialize_unchecked" by default which is what we want here
// because non-anchor accounts don't have a discriminator to check
impl anchor_lang::AccountDeserialize for StakePool {
    fn try_deserialize_unchecked(buf: &mut &[u8]) -> Result<Self> {
        Ok(Self(spl_stake_pool::state::StakePool::deserialize(buf)?))
    }
}
// AccountSerialize defaults to a no-op which is what we want here
// because it's a foreign program, so our program does not
// have permission to write to the foreign program's accounts anyway
impl anchor_lang::AccountSerialize for StakePool {}

impl anchor_lang::Owner for StakePool {
    fn owner() -> Pubkey {
        spl_stake_pool::ID
    }
}

// Implement the "std::ops::Deref" trait for better user experience
impl Deref for StakePool {
    type Target = spl_stake_pool::state::StakePool;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// #[cfg(feature = "idl-build")]
// impl anchor_lang::IdlBuild for StakePool {}

#[derive(Clone)]
pub struct ValidatorList(spl_stake_pool::state::ValidatorList);

impl AsRef<spl_stake_pool::state::ValidatorList> for ValidatorList {
    fn as_ref(&self) -> &spl_stake_pool::state::ValidatorList {
        &self.0
    }
}

// This is necessary so we can use "anchor_spl::token::Mint::LEN"
// because rust does not resolve "anchor_spl::token::Mint::LEN" to
// "spl_token::state::Mint::LEN" automatically
impl ValidatorList {
    pub const LEN: usize = std::mem::size_of::<spl_stake_pool::state::ValidatorList>();
}

// You don't have to implement the "try_deserialize" function
// from this trait. It delegates to
// "try_deserialize_unchecked" by default which is what we want here
// because non-anchor accounts don't have a discriminator to check
impl anchor_lang::AccountDeserialize for ValidatorList {
    fn try_deserialize_unchecked(buf: &mut &[u8]) -> Result<Self> {
        Ok(Self(spl_stake_pool::state::ValidatorList::deserialize(
            buf,
        )?))
    }
}
// AccountSerialize defaults to a no-op which is what we want here
// because it's a foreign program, so our program does not
// have permission to write to the foreign program's accounts anyway
impl anchor_lang::AccountSerialize for ValidatorList {}

impl anchor_lang::Owner for ValidatorList {
    fn owner() -> Pubkey {
        spl_stake_pool::ID
    }
}

impl Deref for ValidatorList {
    type Target = spl_stake_pool::state::ValidatorList;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
