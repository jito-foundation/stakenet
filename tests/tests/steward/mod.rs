#![allow(unexpected_cfgs)]
mod test_algorithms;
mod test_cycle;
mod test_directed_stake_instructions;
mod test_epoch_maintenance;
mod test_integration;
mod test_parameters;
mod test_priority_fee_parameters;
mod test_scoring;
mod test_spl_passthrough;
mod test_state_methods;
mod test_state_transitions;
mod test_steward;

use spl_stake_pool::state::{PodStakeStatus, ValidatorStakeInfo};

pub fn serialize_validator_list(validator_list: &[ValidatorStakeInfo]) -> Vec<u8> {
    let mut data = Vec::new();

    // First, write the length of the list as u32 (Borsh convention)
    data.extend_from_slice(&(validator_list.len() as u32).to_le_bytes());

    // Then serialize each validator
    for validator in validator_list {
        // Serialize each field in order
        // PodU64 fields (8 bytes each)
        data.extend_from_slice(&u64::from(validator.active_stake_lamports).to_le_bytes());
        data.extend_from_slice(&u64::from(validator.transient_stake_lamports).to_le_bytes());
        data.extend_from_slice(&u64::from(validator.last_update_epoch).to_le_bytes());
        data.extend_from_slice(&u64::from(validator.transient_seed_suffix).to_le_bytes());

        // PodU32 fields (4 bytes each)
        data.extend_from_slice(&u32::from(validator.unused).to_le_bytes());
        data.extend_from_slice(&u32::from(validator.validator_seed_suffix).to_le_bytes());

        // PodStakeStatus - it's a transparent wrapper around u8, so we can access the inner value
        // Since it's repr(transparent), we can transmute or access the inner u8
        let status_byte = unsafe { *(&validator.status as *const PodStakeStatus as *const u8) };
        data.push(status_byte);

        // Pubkey (32 bytes)
        data.extend_from_slice(validator.vote_account_address.as_ref());
    }

    data
}
