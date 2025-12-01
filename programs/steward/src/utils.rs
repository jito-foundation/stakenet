use std::ops::Not;

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{clock::Epoch, program_pack::Pack, stake};
use borsh::{BorshDeserialize, BorshSerialize};
use spl_pod::{bytemuck::pod_from_bytes, primitives::PodU64};
use spl_stake_pool::{
    big_vec::BigVec,
    state::{StakeStatus, ValidatorListHeader, ValidatorStakeInfo},
};

use crate::{
    constants::{
        PUBKEY_SIZE, STAKE_STATUS_OFFSET, TRANSIENT_STAKE_SEED_LENGTH, TRANSIENT_STAKE_SEED_OFFSET,
        U64_SIZE, VALIDATOR_LIST_HEADER_SIZE, VEC_SIZE_BYTES, VOTE_ADDRESS_OFFSET,
    },
    errors::StewardError,
    Config, Delegation, StewardStateAccountV2, StewardStateEnum,
};

/// Checks called before any cranking state function. Note that expected_state is optional -
/// this is due to ComputeScores handling it's own state check.
pub fn state_checks(
    clock: &Clock,
    config: &Config,
    state_account: &StewardStateAccountV2,
    validator_list_account_info: &AccountInfo,
    expected_state: Option<StewardStateEnum>,
) -> Result<()> {
    if config.is_paused() {
        return Err(StewardError::StateMachinePaused.into());
    }

    if let Some(expected_state) = expected_state {
        msg!(
            "Expected state: {}, Current state: {}",
            expected_state,
            state_account.state.state_tag
        );
        require!(
            state_account.state.state_tag == expected_state,
            StewardError::InvalidState
        );
    }

    require!(
        clock.epoch == state_account.state.current_epoch,
        StewardError::EpochMaintenanceNotComplete
    );

    require!(
        state_account.state.validators_for_immediate_removal.count() == 0,
        StewardError::ValidatorsNeedToBeRemoved
    );

    // Ensure we have a 1-1 mapping between the number of validators
    let validators_in_list = get_validator_list_length(validator_list_account_info)?;
    require!(
        state_account.state.num_pool_validators as usize
            + state_account.state.validators_added as usize
            == validators_in_list,
        StewardError::ListStateMismatch
    );

    Ok(())
}

pub fn remove_validator_check(
    clock: &Clock,
    config: &Config,
    state_account: &StewardStateAccountV2,
    validator_list_account_info: &AccountInfo,
) -> Result<()> {
    if config.is_paused() {
        return Err(StewardError::StateMachinePaused.into());
    }

    require!(
        clock.epoch == state_account.state.current_epoch,
        StewardError::EpochMaintenanceNotComplete
    );

    // Ensure we have a 1-1 mapping between the number of validators
    let validators_in_list = get_validator_list_length(validator_list_account_info)?;
    require!(
        state_account.state.num_pool_validators as usize
            + state_account.state.validators_added as usize
            == validators_in_list,
        StewardError::ListStateMismatch
    );

    Ok(())
}

pub fn add_validator_check(
    clock: &Clock,
    config: &Config,
    state_account: &StewardStateAccountV2,
    validator_list_account_info: &AccountInfo,
) -> Result<()> {
    if config.is_paused() {
        return Err(StewardError::StateMachinePaused.into());
    }

    require!(
        clock.epoch == state_account.state.current_epoch,
        StewardError::EpochMaintenanceNotComplete
    );

    require!(
        state_account.state.validators_for_immediate_removal.count() == 0,
        StewardError::ValidatorsNeedToBeRemoved
    );

    // Ensure we have a 1-1 mapping between the number of validators
    let validators_in_list = get_validator_list_length(validator_list_account_info)?;
    require!(
        state_account.state.num_pool_validators as usize
            + state_account.state.validators_added as usize
            == validators_in_list,
        StewardError::ListStateMismatch
    );

    Ok(())
}

pub fn get_stake_pool_address(account: &AccountLoader<Config>) -> Result<Pubkey> {
    let config = account.load()?;
    Ok(config.stake_pool)
}

pub fn get_validator_list(account: &AccountLoader<Config>) -> Result<Pubkey> {
    let config = account.load()?;
    Ok(config.validator_list)
}

pub fn get_config_admin(account: &AccountLoader<Config>) -> Result<Pubkey> {
    let config = account.load()?;
    Ok(config.admin)
}

pub fn get_config_blacklist_authority(account: &AccountLoader<Config>) -> Result<Pubkey> {
    let config = account.load()?;
    Ok(config.blacklist_authority)
}

pub fn get_config_parameter_authority(account: &AccountLoader<Config>) -> Result<Pubkey> {
    let config = account.load()?;
    Ok(config.parameters_authority)
}

pub fn get_config_priority_fee_parameter_authority(
    account: &AccountLoader<Config>,
) -> Result<Pubkey> {
    let config = account.load()?;
    Ok(config.priority_fee_parameters_authority)
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
    match delegation.numerator {
        0 => Ok(0),
        1 => stake_pool_lamports
            .checked_div(delegation.denominator as u64)
            .ok_or_else(|| StewardError::ArithmeticError.into()),
        _ => {
            let target: u64 = (stake_pool_lamports as u128)
                .checked_mul(delegation.numerator as u128)
                .and_then(|x| x.checked_div(delegation.denominator as u128))
                .ok_or(StewardError::ArithmeticError)?
                .try_into()
                .map_err(|_| StewardError::ArithmeticCastError)?;
            Ok(target)
        }
    }
}

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
        .checked_add(U64_SIZE)
        .ok_or(StewardError::ArithmeticError)?;
    let transient_start_index = active_end_index;
    let transient_end_index = transient_start_index
        .checked_add(U64_SIZE)
        .ok_or(StewardError::ArithmeticError)?;
    let slice = &validator_list.data[active_start_index..active_end_index];
    let active_stake_lamport_pod = pod_from_bytes::<PodU64>(slice).unwrap();
    let slice = &validator_list.data[transient_start_index..transient_end_index];
    let some_transient_stake = u64::from(*pod_from_bytes::<PodU64>(slice).unwrap()) != 0;
    Ok((u64::from(*active_stake_lamport_pod), some_transient_stake))
}

/// Utility to efficiently extract vote pubkey from a validator list.
/// Frankenstein of spl_stake_pool::big_vec::BigVec::deserialize_slice
/// and spl_stake_pool::state::ValidatorStakeInfo::active_lamports_greater_than
#[inline(always)]
pub fn vote_pubkey_at_validator_list_index(
    validator_list: &BigVec<'_>,
    index: usize,
) -> Result<Pubkey> {
    let pubkey_start_index = VEC_SIZE_BYTES
        .saturating_add(index.saturating_mul(ValidatorStakeInfo::LEN))
        .saturating_add(VOTE_ADDRESS_OFFSET);
    let pubkey_end_index = pubkey_start_index.saturating_add(PUBKEY_SIZE);
    let slice: [u8; PUBKEY_SIZE] = validator_list.data[pubkey_start_index..pubkey_end_index]
        .try_into()
        .map_err(|_| StewardError::ArithmeticError)?;
    let pubkey = Pubkey::new_from_array(slice);
    Ok(pubkey)
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

pub fn get_transient_stake_seed_at_index_from_big_vec(
    validator_list: &BigVec<'_>,
    index: usize,
) -> Result<u64> {
    let transient_seed_index = VEC_SIZE_BYTES
        .saturating_add(index.saturating_mul(ValidatorStakeInfo::LEN))
        .saturating_add(TRANSIENT_STAKE_SEED_OFFSET);
    let transient_seed_end_index = transient_seed_index
        .checked_add(TRANSIENT_STAKE_SEED_LENGTH)
        .ok_or(StewardError::ArithmeticError)?;
    let slice: [u8; TRANSIENT_STAKE_SEED_LENGTH] = validator_list.data
        [transient_seed_index..transient_seed_end_index]
        .try_into()
        .map_err(|_| StewardError::ArithmeticError)?;
    let transient_seed = u64::from_le_bytes(slice);
    Ok(transient_seed)
}

pub struct StakeStatusTally {
    pub active: u64,
    pub deactivating_transient: u64,
    pub ready_for_removal: u64,
    pub deactivating_validator: u64,
    pub deactivating_all: u64,
}

pub fn tally_stake_status(validator_list_account_info: &AccountInfo) -> Result<StakeStatusTally> {
    let mut validator_list_data = validator_list_account_info.try_borrow_mut_data()?;
    let (header, validator_list) = ValidatorListHeader::deserialize_vec(&mut validator_list_data)?;
    require!(
        header.account_type == spl_stake_pool::state::AccountType::ValidatorList,
        StewardError::ValidatorListTypeMismatch
    );

    let mut tally = StakeStatusTally {
        active: 0,
        deactivating_transient: 0,
        ready_for_removal: 0,
        deactivating_validator: 0,
        deactivating_all: 0,
    };

    for index in 0..validator_list.len() as usize {
        let stake_status_index = VEC_SIZE_BYTES
            .saturating_add(index.saturating_mul(ValidatorStakeInfo::LEN))
            .checked_add(STAKE_STATUS_OFFSET)
            .ok_or(StewardError::ArithmeticError)?;

        let stake_status = validator_list.data[stake_status_index];

        match stake_status {
            x if x == StakeStatus::Active as u8 => {
                tally.active = tally
                    .active
                    .checked_add(1)
                    .ok_or(StewardError::ArithmeticError)?;
            }
            x if x == StakeStatus::DeactivatingTransient as u8 => {
                tally.deactivating_transient = tally
                    .deactivating_transient
                    .checked_add(1)
                    .ok_or(StewardError::ArithmeticError)?;
            }
            x if x == StakeStatus::ReadyForRemoval as u8 => {
                tally.ready_for_removal = tally
                    .ready_for_removal
                    .checked_add(1)
                    .ok_or(StewardError::ArithmeticError)?;
            }
            x if x == StakeStatus::DeactivatingValidator as u8 => {
                tally.deactivating_validator = tally
                    .deactivating_validator
                    .checked_add(1)
                    .ok_or(StewardError::ArithmeticError)?;
            }
            x if x == StakeStatus::DeactivatingAll as u8 => {
                tally.deactivating_all = tally
                    .deactivating_all
                    .checked_add(1)
                    .ok_or(StewardError::ArithmeticError)?;
            }
            _ => {
                return Err(StewardError::InvalidStakeState.into());
            }
        }
    }

    Ok(tally)
}

pub fn check_validator_list_has_stake_status_other_than(
    validator_list_account_info: &AccountInfo,
    flags: &[StakeStatus],
) -> Result<bool> {
    let mut validator_list_data = validator_list_account_info.try_borrow_mut_data()?;
    let (header, validator_list) = ValidatorListHeader::deserialize_vec(&mut validator_list_data)?;
    require!(
        header.account_type == spl_stake_pool::state::AccountType::ValidatorList,
        StewardError::ValidatorListTypeMismatch
    );

    for index in 0..validator_list.len() as usize {
        let stake_status_index = VEC_SIZE_BYTES
            .saturating_add(index.saturating_mul(ValidatorStakeInfo::LEN))
            .checked_add(STAKE_STATUS_OFFSET)
            .ok_or(StewardError::ArithmeticError)?;

        let stake_status = validator_list.data[stake_status_index];

        let mut has_flag = false;
        for flag in flags.iter() {
            if stake_status == *flag as u8 {
                has_flag = true;
            }
        }

        if !has_flag {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Checks if a stake account can be managed by the pool
/// FROM spl_stake_pool::processor::update_validator_list_balance
pub fn stake_is_usable_by_pool(
    meta: &stake::state::Meta,
    expected_authority: &Pubkey,
    expected_lockup: &stake::state::Lockup,
) -> bool {
    meta.authorized.staker == *expected_authority
        && meta.authorized.withdrawer == *expected_authority
        && meta.lockup == *expected_lockup
}

/// Checks if a stake account is active, without taking into account cooldowns
/// FROM spl_stake_pool::processor::update_validator_list_balance
pub fn stake_is_inactive_without_history(stake: &stake::state::Stake, epoch: Epoch) -> bool {
    stake.delegation.deactivation_epoch < epoch
        || (stake.delegation.activation_epoch == epoch
            && stake.delegation.deactivation_epoch == epoch)
}

pub fn get_validator_list_length(validator_list_account_info: &AccountInfo) -> Result<usize> {
    let mut validator_list_data = validator_list_account_info.try_borrow_mut_data()?;
    let (header, validator_list) = ValidatorListHeader::deserialize_vec(&mut validator_list_data)?;
    require!(
        header.account_type == spl_stake_pool::state::AccountType::ValidatorList,
        StewardError::ValidatorListTypeMismatch
    );
    Ok(validator_list.len() as usize)
}

/// Check if a vote account exists in the validator list
pub fn validator_exists_in_list(
    validator_list_account_info: &AccountInfo,
    vote_account: &Pubkey,
) -> Result<bool> {
    let mut validator_list_data = validator_list_account_info.try_borrow_mut_data()?;
    let (header, validator_list) = ValidatorListHeader::deserialize_vec(&mut validator_list_data)?;
    require!(
        header.account_type == spl_stake_pool::state::AccountType::ValidatorList,
        StewardError::ValidatorListTypeMismatch
    );

    for index in 0..validator_list.len() as usize {
        let vote_pubkey = vote_pubkey_at_validator_list_index(&validator_list, index)?;
        if vote_pubkey == *vote_account {
            return Ok(true);
        }
    }

    Ok(false)
}

/// A boolean type stored as a u8.
#[derive(BorshSerialize, BorshDeserialize, Debug, PartialEq, Eq)]
#[zero_copy]
pub struct U8Bool {
    pub value: u8,
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
