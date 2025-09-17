use anchor_lang::prelude::*;
use anchor_lang::Discriminator;

use crate::constants::MAX_VALIDATORS;
use crate::state::{Config, StewardStateAccount, StewardStateAccountV2};
use crate::StewardStateV2;

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
    // 3. sorted_yield_score_indices: becomes sorted_raw_score_indices (stays in place)
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
        base + 8
            + size_u64_array
            + size_u32_array
            + size_u16_array
            + size_u32_array
            + size_u16_array
            + (8 * MAX_VALIDATORS)
            + (4 * core::mem::size_of::<crate::BitMask>())
            + (7 * 8)
            + 4
            + 2
    };
    let off_v2_raw_scores = v1_padding_offset + 2; // Skip the 2-byte _padding0 in V2

    // ==========================================
    // STEP 3: Save yield_scores before it gets overwritten
    // ==========================================

    // We need to save yield_scores before moving sorted_score_indices
    // because sorted_score_indices' new location overlaps with yield_scores in V1
    let yield_scores = {
        let v1_account: &StewardStateAccount =
            bytemuck::from_bytes(&data[8..8 + core::mem::size_of::<StewardStateAccount>()]);
        v1_account.state.yield_scores.to_vec()
    };

    // ==========================================
    // STEP 4: Move sorted_score_indices to new location
    // ==========================================

    // Now we can safely move sorted_score_indices
    // Use copy_within since source and destination don't overlap
    data.copy_within(
        off_v1_sorted_score_indices..off_v1_sorted_score_indices + size_u16_array,
        off_v2_sorted_score_indices,
    );

    // ==========================================
    // STEP 5: Write yield_scores as raw_scores
    // ==========================================

    // Write the saved yield_scores as expanded raw_scores to V2 location
    for (i, &score) in yield_scores.iter().enumerate() {
        let val = score as u64;
        let dst_off = off_v2_raw_scores + i * 8;
        data[dst_off..dst_off + 8].copy_from_slice(&val.to_le_bytes());
    }

    // ==========================================
    // STEP 6: Expand scores from u32 to u64 in place
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
    // STEP 7: Update discriminator and account-level fields
    // ==========================================

    // Update to V2 discriminator
    data[0..8].copy_from_slice(StewardStateAccountV2::DISCRIMINATOR);

    // Handle account-level fields after the state struct
    // The v1 struct had an is_initialized field and we zero that out for the v2 struct as padding
    let state_v2_size = core::mem::size_of::<StewardStateV2>();
    let account_fields_offset = base + state_v2_size;
    data[account_fields_offset] = 0;

    Ok(())
}
