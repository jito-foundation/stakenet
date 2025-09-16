use anchor_lang::prelude::*;
use anchor_lang::Discriminator;

use crate::constants::MAX_VALIDATORS;
use crate::BitMask;
use crate::Delegation;
use crate::StewardStateEnum;
use crate::StewardStateV2;
use crate::{
    state::{Config, StewardStateAccount, StewardStateAccountV2},
    utils::get_config_admin,
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
    let mut data = ctx.accounts.state_account.data.borrow_mut();

    // Verify this is a V1 account by checking the discriminator
    if &data[0..8] != StewardStateAccount::DISCRIMINATOR {
        return Err(ProgramError::InvalidAccountData.into());
    }

    // ==========================================
    // STEP 1: Calculate sizes and offsets
    // ==========================================

    // The discriminator takes 8 bytes at the beginning
    let base = 8usize;

    // Calculate the size of each data structure
    let size_u32_array = 4 * MAX_VALIDATORS; // 4 bytes per u32
    let size_u16_array = 2 * MAX_VALIDATORS; // 2 bytes per u16
    let size_u64_array = 8 * MAX_VALIDATORS; // 8 bytes per u64
    let size_state_enum = core::mem::size_of::<StewardStateEnum>();
    let size_delegation = core::mem::size_of::<Delegation>() * MAX_VALIDATORS;
    let size_bitmask = core::mem::size_of::<BitMask>();

    // Calculate V1 layout offsets (where each field starts in the V1 account)
    // The V1 layout is:
    // - state_tag (StewardStateEnum)
    // - balances (u64 array)
    // - scores (u32 array)
    // - sorted_score_indices (u16 array)
    // - yield_scores (u32 array)
    // - sorted_yield_score_indices (u16 array)
    // - delegations (Delegation array)
    // - instant_unstake (BitMask)
    // - progress (BitMask)
    // - validators_for_immediate_removal (BitMask)
    // - validators_to_remove (BitMask)
    // - start_slot (u64)
    // - current_epoch (u64)
    // - next_cycle_epoch (u64)
    // - num_pool_validators (u64)
    // - scoring_unstake_total (u64)
    // - instant_unstake_total (u64)
    // - stake_deposit_unstake_total (u64)
    // - status_flags (u32)
    // - validators_added (u16)
    // - _padding ([u8; 232])

    let off_v1_balances = size_state_enum;
    let off_v1_scores = off_v1_balances + size_u64_array;
    let off_v1_sorted_score_indices = off_v1_scores + size_u32_array;
    let off_v1_yield_scores = off_v1_sorted_score_indices + size_u16_array;
    let off_v1_sorted_yield_score_indices = off_v1_yield_scores + size_u32_array;
    let off_v1_delegations = off_v1_sorted_yield_score_indices + size_u16_array;
    let off_v1_instant_unstake = off_v1_delegations + size_delegation;
    let off_v1_progress = off_v1_instant_unstake + size_bitmask;
    let off_v1_validators_for_immediate_removal = off_v1_progress + size_bitmask;
    let off_v1_validators_to_remove = off_v1_validators_for_immediate_removal + size_bitmask;
    let off_v1_start_slot = off_v1_validators_to_remove + size_bitmask;
    let off_v1_current_epoch = off_v1_start_slot + 8;
    let off_v1_next_cycle_epoch = off_v1_current_epoch + 8;
    let off_v1_num_pool_validators = off_v1_next_cycle_epoch + 8;
    let off_v1_scoring_unstake_total = off_v1_num_pool_validators + 8;
    let off_v1_instant_unstake_total = off_v1_scoring_unstake_total + 8;
    let off_v1_stake_deposit_unstake_total = off_v1_instant_unstake_total + 8;
    let off_v1_status_flags = off_v1_stake_deposit_unstake_total + 8;
    let off_v1_validators_added = off_v1_status_flags + 4;
    let off_v1_padding = off_v1_validators_added + 2;

    // ==========================================
    // STEP 2: Save data that will be overwritten
    // ==========================================

    // The sorted_score_indices will be overwritten when we expand scores from u32 to u64
    // so we need to save it first
    let mut sorted_score_indices_data = vec![0u8; size_u16_array];
    sorted_score_indices_data.copy_from_slice(
        &data[base + off_v1_sorted_score_indices
            ..base + off_v1_sorted_score_indices + size_u16_array],
    );

    // Save yield_scores as they will become the new raw_scores field in V2
    let mut yield_scores_data = vec![0u8; size_u32_array];
    yield_scores_data.copy_from_slice(
        &data[base + off_v1_yield_scores..base + off_v1_yield_scores + size_u32_array],
    );

    // ==========================================
    // STEP 3: Expand scores from u32 to u64
    // ==========================================

    // The main challenge: scores in V1 are u32, but in V2 they're u64
    // We need to expand them in-place, working backwards to avoid overwriting data
    for i in (0..MAX_VALIDATORS).rev() {
        let src_off = off_v1_scores + i * 4; // Source: u32 at position i
        let dst_off = off_v1_scores + i * 8; // Destination: u64 at position i

        // Read the u32 value
        let mut buf4 = [0u8; 4];
        buf4.copy_from_slice(&data[base + src_off..base + src_off + 4]);
        let val = u32::from_le_bytes(buf4) as u64;

        // Write it as u64
        let bytes = val.to_le_bytes();
        data[base + dst_off..base + dst_off + 8].copy_from_slice(&bytes);
    }

    // ==========================================
    // STEP 4: Restore sorted_score_indices at new location
    // ==========================================

    // After expanding scores, sorted_score_indices needs to move to its new position
    // In V2, it comes right after the expanded u64 scores array
    let off_v2_sorted_score_indices = off_v1_scores + size_u64_array;
    data[base + off_v2_sorted_score_indices..base + off_v2_sorted_score_indices + size_u16_array]
        .copy_from_slice(&sorted_score_indices_data);

    // ==========================================
    // STEP 5: Add new raw_scores field at the end
    // ==========================================

    // V2 adds a new field: raw_scores (u64 array)
    // We place it where the padding used to be in V1 (plus 2 bytes for alignment)
    // These raw_scores come from the V1 yield_scores (expanded from u32 to u64)
    let off_v2_raw_scores = base + off_v1_padding + 2;
    for i in 0..MAX_VALIDATORS {
        let src_off = i * 4;
        let dst_off = off_v2_raw_scores + i * 8;

        // Read the u32 yield_score
        let mut buf4 = [0u8; 4];
        buf4.copy_from_slice(&yield_scores_data[src_off..src_off + 4]);
        let val = u32::from_le_bytes(buf4) as u64;

        // Write it as u64 raw_score
        let bytes = val.to_le_bytes();
        data[dst_off..dst_off + 8].copy_from_slice(&bytes);
    }

    // ==========================================
    // STEP 6: Update discriminator to V2
    // ==========================================

    // Mark this as a V2 account by updating the discriminator
    data[0..8].copy_from_slice(StewardStateAccountV2::DISCRIMINATOR);

    // ==========================================
    // STEP 7: Handle account-level fields
    // ==========================================

    // Account-level fields come after the state struct itself
    // V1 layout after state: is_initialized (1 byte), bump (1 byte), _padding (6 bytes)
    // V2 layout after state: _padding0 (1 byte), bump (1 byte), _padding1 (6 bytes)

    // The main change: is_initialized is no longer needed in V2 (discriminator serves this purpose)
    // so it becomes padding

    // Calculate offset to account-level fields (after the state struct)
    let state_v2_size = core::mem::size_of::<StewardStateV2>();
    let account_fields_offset = base + state_v2_size;

    // Zero out _padding0 (was is_initialized in V1, now repurposed as padding)
    data[account_fields_offset] = 0;

    // bump field stays at the same position (offset + 1), no change needed
    // _padding1 stays at the same position as V1 _padding (offset + 2..offset + 8), no change needed

    Ok(())
}
