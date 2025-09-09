use anchor_lang::prelude::*;
use anchor_lang::Discriminator;
use core::ptr;

use crate::{
    state::{Config, StewardStateAccount, StewardStateAccountV2},
    utils::get_config_admin,
    STATE_PADDING_0_SIZE_V2,
};

#[derive(Accounts)]
pub struct MigrateStateToV2<'info> {
    #[account(
        mut,
        seeds = [StewardStateAccount::SEED, config.key().as_ref()],
        bump,
    )]
    /// CHECK: We're reading this as V1 and writing as V2
    pub state_account: AccountInfo<'info>,

    pub config: AccountLoader<'info, Config>,

    #[account(address = get_config_admin(&config)?)]
    pub signer: Signer<'info>,
}

pub fn handler(ctx: Context<MigrateStateToV2>) -> Result<()> {
    // Borrow account data mutably; we will read/write in-place using raw pointers.
    let mut data = ctx.accounts.state_account.data.borrow_mut();

    // Verify this is a V1 account by checking the discriminator
    msg!("Found discriminator: {:?}", &data[0..8]);
    msg!(
        "Expected V1 discriminator: {:?}",
        StewardStateAccount::DISCRIMINATOR
    );
    if &data[0..8] != StewardStateAccount::DISCRIMINATOR {
        return Err(ProgramError::InvalidAccountData.into());
    }

    // Offsets within the account after the 8-byte discriminator
    let base = 8usize;

    // Sizes and shifts
    let max = crate::constants::MAX_VALIDATORS;
    let sz_u64_arr = core::mem::size_of::<u64>() * max; // 8 * MAX
    let sz_u32_arr = core::mem::size_of::<u32>() * max; // 4 * MAX
    let sz_u16_arr = core::mem::size_of::<u16>() * max; // 2 * MAX
    let sz_deleg = core::mem::size_of::<crate::Delegation>() * max; // 8 * MAX
    let sz_bitmask = core::mem::size_of::<crate::bitmask::BitMask>(); // 8 * ceil(MAX/64)

    let shift_scores = sz_u64_arr - sz_u32_arr; // +4 * MAX
    let shift_raw = sz_u64_arr - sz_u32_arr; // +4 * MAX
    let _total_shift = shift_scores + shift_raw; // +8 * MAX

    // V1 layout offsets (relative to base)
    let off_v1_state_tag = 0usize;
    let off_v1_balances = off_v1_state_tag + core::mem::size_of::<crate::StewardStateEnum>();
    let off_v1_scores = off_v1_balances + sz_u64_arr;
    let off_v1_sorted_score_indices = off_v1_scores + sz_u32_arr;
    let off_v1_yield_scores = off_v1_sorted_score_indices + sz_u16_arr;
    let off_v1_sorted_yield_score_indices = off_v1_yield_scores + sz_u32_arr;
    let off_v1_delegations = off_v1_sorted_yield_score_indices + sz_u16_arr;
    let off_v1_instant_unstake = off_v1_delegations + sz_deleg;
    let off_v1_progress = off_v1_instant_unstake + sz_bitmask;
    let off_v1_validators_for_immediate_removal = off_v1_progress + sz_bitmask;
    let off_v1_validators_to_remove = off_v1_validators_for_immediate_removal + sz_bitmask;
    let off_v1_start_slot = off_v1_validators_to_remove + sz_bitmask;
    let off_v1_current_epoch = off_v1_start_slot + 8;
    let off_v1_next_cycle_epoch = off_v1_current_epoch + 8;
    let off_v1_num_pool_validators = off_v1_next_cycle_epoch + 8;
    let off_v1_scoring_unstake_total = off_v1_num_pool_validators + 8;
    let off_v1_instant_unstake_total = off_v1_scoring_unstake_total + 8;
    let off_v1_stake_deposit_unstake_total = off_v1_instant_unstake_total + 8;
    let off_v1_status_flags = off_v1_stake_deposit_unstake_total + 8;
    let off_v1_validators_added = off_v1_status_flags + 4;
    let _off_v1_padding0 = off_v1_validators_added + 2;

    // V2 layout offsets (relative to base)
    let off_v2_state_tag = off_v1_state_tag;
    let off_v2_balances = off_v2_state_tag + core::mem::size_of::<crate::StewardStateEnum>();
    let off_v2_scores = off_v2_balances + sz_u64_arr; // u64[MAX]
    let off_v2_sorted_score_indices = off_v2_scores + sz_u64_arr; // after expanded scores
    let off_v2_raw_scores = off_v2_sorted_score_indices + sz_u16_arr; // u64[MAX]
    let off_v2_sorted_raw_score_indices = off_v2_raw_scores + sz_u64_arr;
    let off_v2_delegations = off_v2_sorted_raw_score_indices + sz_u16_arr;
    let off_v2_instant_unstake = off_v2_delegations + sz_deleg;
    let off_v2_progress = off_v2_instant_unstake + sz_bitmask;
    let off_v2_validators_for_immediate_removal = off_v2_progress + sz_bitmask;
    let off_v2_validators_to_remove = off_v2_validators_for_immediate_removal + sz_bitmask;
    let off_v2_start_slot = off_v2_validators_to_remove + sz_bitmask;
    let off_v2_current_epoch = off_v2_start_slot + 8;
    let off_v2_next_cycle_epoch = off_v2_current_epoch + 8;
    let off_v2_num_pool_validators = off_v2_next_cycle_epoch + 8;
    let off_v2_scoring_unstake_total = off_v2_num_pool_validators + 8;
    let off_v2_instant_unstake_total = off_v2_scoring_unstake_total + 8;
    let off_v2_stake_deposit_unstake_total = off_v2_instant_unstake_total + 8;
    let off_v2_status_flags = off_v2_stake_deposit_unstake_total + 8;
    let off_v2_validators_added = off_v2_status_flags + 4;
    let off_v2_padding0 = off_v2_validators_added + 2;

    // Raw pointer to the start of state bytes
    let p = unsafe { data.as_mut_ptr().add(base) };

    // Helper for memmove-like copy within the same buffer
    unsafe fn copy_bytes(p: *mut u8, src_off: usize, dst_off: usize, len: usize) {
        if len == 0 || src_off == dst_off {
            return;
        }
        ptr::copy(p.add(src_off), p.add(dst_off), len);
    }

    // Move trailing fields (those after yield_scores) forward by total_shift, from end to start
    unsafe {
        // validators_added (2)
        copy_bytes(p, off_v1_validators_added, off_v2_validators_added, 2);
        // status_flags (4)
        copy_bytes(p, off_v1_status_flags, off_v2_status_flags, 4);
        // stake/unstake totals and counters (8 each)
        copy_bytes(
            p,
            off_v1_stake_deposit_unstake_total,
            off_v2_stake_deposit_unstake_total,
            8,
        );
        copy_bytes(
            p,
            off_v1_instant_unstake_total,
            off_v2_instant_unstake_total,
            8,
        );
        copy_bytes(
            p,
            off_v1_scoring_unstake_total,
            off_v2_scoring_unstake_total,
            8,
        );
        copy_bytes(p, off_v1_num_pool_validators, off_v2_num_pool_validators, 8);
        copy_bytes(p, off_v1_next_cycle_epoch, off_v2_next_cycle_epoch, 8);
        copy_bytes(p, off_v1_current_epoch, off_v2_current_epoch, 8);
        copy_bytes(p, off_v1_start_slot, off_v2_start_slot, 8);
        // Bitmasks
        copy_bytes(
            p,
            off_v1_validators_to_remove,
            off_v2_validators_to_remove,
            sz_bitmask,
        );
        copy_bytes(
            p,
            off_v1_validators_for_immediate_removal,
            off_v2_validators_for_immediate_removal,
            sz_bitmask,
        );
        copy_bytes(p, off_v1_progress, off_v2_progress, sz_bitmask);
        copy_bytes(
            p,
            off_v1_instant_unstake,
            off_v2_instant_unstake,
            sz_bitmask,
        );
        // Delegations
        copy_bytes(p, off_v1_delegations, off_v2_delegations, sz_deleg);
        // sorted_yield_score_indices -> sorted_raw_score_indices
        copy_bytes(
            p,
            off_v1_sorted_yield_score_indices,
            off_v2_sorted_raw_score_indices,
            sz_u16_arr,
        );
    }

    // Convert raw_scores: v1 yield_scores [u32; MAX] -> v2 raw_scores [u64; MAX]
    for i in (0..max).rev() {
        let src_off = off_v1_yield_scores + i * 4;
        let dst_off = off_v2_raw_scores + i * 8;
        // Read LE u32
        let mut buf4 = [0u8; 4];
        buf4.copy_from_slice(&data[base + src_off..base + src_off + 4]);
        let val = u32::from_le_bytes(buf4) as u64;
        let bytes = val.to_le_bytes();
        data[base + dst_off..base + dst_off + 8].copy_from_slice(&bytes);
    }

    // Move sorted_score_indices forward by shift_scores
    unsafe {
        copy_bytes(
            p,
            off_v1_sorted_score_indices,
            off_v2_sorted_score_indices,
            sz_u16_arr,
        );
    }

    // Convert scores: v1 scores [u32; MAX] -> v2 scores [u64; MAX]
    for i in (0..max).rev() {
        let src_off = off_v1_scores + i * 4;
        let dst_off = off_v2_scores + i * 8;
        let mut buf4 = [0u8; 4];
        buf4.copy_from_slice(&data[base + src_off..base + src_off + 4]);
        let val = u32::from_le_bytes(buf4) as u64;
        let bytes = val.to_le_bytes();
        data[base + dst_off..base + dst_off + 8].copy_from_slice(&bytes);
    }

    // Zero reduced padding for V2
    for i in 0..STATE_PADDING_0_SIZE_V2 {
        data[base + off_v2_padding0 + i] = 0u8;
    }

    // Write the V2 discriminator
    let v2_discriminator = StewardStateAccountV2::DISCRIMINATOR;
    data[0..8].copy_from_slice(v2_discriminator);

    msg!("Successfully migrated steward state from V1 to V2");
    Ok(())
}
