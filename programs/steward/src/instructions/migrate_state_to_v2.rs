use anchor_lang::prelude::*;
use anchor_lang::Discriminator;

use crate::constants::MAX_VALIDATORS;
use crate::StewardStateV1;
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

    // ==========================================
    // STEP 1: Verify this is a V1 account
    // ==========================================

    // Verify this is a V1 account by checking the discriminator
    if &data[0..8] != StewardStateAccount::DISCRIMINATOR {
        return Err(ProgramError::InvalidAccountData.into());
    }

    // ==========================================
    // STEP 2: Calculate offsets
    // ==========================================

    // The migration only changes a few things:
    // 1. scores: expand from u32 to u64 (in place)
    // 2. sorted_score_indices: moves after expanded scores
    // 3. sorted_yield_score_indices: becomes sorted_raw_score_indices (stays in place!)
    // 4. yield_scores: becomes raw_scores at the end (expanded to u64)

    let base = 8usize; // Skip discriminator
    let size_u16_array = 2 * MAX_VALIDATORS;
    let size_u32_array = 4 * MAX_VALIDATORS;
    let size_u64_array = 8 * MAX_VALIDATORS;

    // V1 offsets
    let off_scores = base + 8 + size_u64_array; // After state_tag(8) + balances(u64 array)
    let off_v1_sorted_score_indices = off_scores + size_u32_array;

    // V2 offsets
    let off_v2_sorted_score_indices = off_scores + size_u64_array; // After expanded scores

    // Calculate where raw_scores goes in V2
    // It's where the V1 padding was, after the 2-byte _padding0
    // We need to find the offset of V1's _padding0 field, then add 2 bytes
    let v1_padding_offset = {
        // After all the fields before _padding0 in V1:
        // state_tag + balances + scores + sorted_indices + yield_scores + sorted_yield_indices
        // + delegations + 4 bitmasks + 7 u64s + u32 + u16
        let after_validators_added = base + 8 + size_u64_array + size_u32_array + size_u16_array
            + size_u32_array + size_u16_array + (8 * MAX_VALIDATORS)
            + (4 * core::mem::size_of::<crate::BitMask>()) + (7 * 8) + 4 + 2;
        after_validators_added
    };
    let off_v2_raw_scores = v1_padding_offset + 2; // Skip the 2-byte _padding0 in V2

    // ==========================================
    // STEP 3: Save sorted_score_indices before it gets overwritten
    // ==========================================

    // Use a small buffer to copy in chunks to avoid stack overflow
    let mut sorted_indices_buffer = vec![0u8; size_u16_array];
    sorted_indices_buffer.copy_from_slice(
        &data[off_v1_sorted_score_indices..off_v1_sorted_score_indices + size_u16_array]
    );

    // ==========================================
    // STEP 4: Process yield_scores -> raw_scores
    // ==========================================

    // Read yield_scores from V1 and write as expanded raw_scores to V2 location
    // Process one at a time to avoid borrow conflicts
    for i in 0..MAX_VALIDATORS {
        let val = {
            let v1_account: &StewardStateAccount =
                bytemuck::from_bytes(&data[8..8 + core::mem::size_of::<StewardStateAccount>()]);
            v1_account.state.yield_scores[i] as u64
        }; // Drop the reference here

        let dst_off = off_v2_raw_scores + i * 8;
        data[dst_off..dst_off + 8].copy_from_slice(&val.to_le_bytes());
    }

    // ==========================================
    // STEP 5: Expand scores from u32 to u64 in place
    // ==========================================

    // Work backwards to avoid overwriting unread data
    for i in (0..MAX_VALIDATORS).rev() {
        let val = {
            let v1_account: &StewardStateAccount =
                bytemuck::from_bytes(&data[8..8 + core::mem::size_of::<StewardStateAccount>()]);
            v1_account.state.scores[i] as u64
        }; // Drop the reference here

        let dst_off = off_scores + i * 8;
        data[dst_off..dst_off + 8].copy_from_slice(&val.to_le_bytes());
    }

    // ==========================================
    // STEP 6: Write sorted_score_indices at new location
    // ==========================================

    // Move sorted_score_indices to its new position after expanded scores
    data[off_v2_sorted_score_indices..off_v2_sorted_score_indices + size_u16_array]
        .copy_from_slice(&sorted_indices_buffer);

    // Note: sorted_yield_score_indices stays in place and becomes sorted_raw_score_indices

    // ==========================================
    // STEP 7: Update discriminator and account-level fields
    // ==========================================

    // Get the bump value before updating discriminator
    let v1_bump = {
        let v1_account: &StewardStateAccount =
            bytemuck::from_bytes(&data[8..8 + core::mem::size_of::<StewardStateAccount>()]);
        v1_account.bump
    };

    // Update to V2 discriminator
    data[0..8].copy_from_slice(StewardStateAccountV2::DISCRIMINATOR);

    // Handle account-level fields after the state struct
    // V1: is_initialized (1 byte), bump (1 byte), _padding (6 bytes)
    // V2: _padding0 (1 byte), bump (1 byte), _padding1 (6 bytes)

    let state_v2_size = core::mem::size_of::<StewardStateV2>();
    let account_fields_offset = base + state_v2_size;

    // Zero out _padding0 (was is_initialized in V1)
    data[account_fields_offset] = 0;

    // Preserve bump at the same relative position
    data[account_fields_offset + 1] = v1_bump;

    // _padding1 remains unchanged at offset+2..offset+8

    Ok(())
}