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
    verify_v1_discriminator(&data)?;

    // ==========================================
    // STEP 2: Calculate all offsets upfront
    // ==========================================
    let offsets = MigrationOffsets::new();

    // ==========================================
    // STEP 3: Extract V1 data that needs preservation
    // ==========================================

    // Extract yield_scores before it gets overwritten by sorted_score_indices movement
    let yield_scores = extract_v1_yield_scores(&data);

    // Extract all scores for expansion (we'll read them one by one during expansion)
    let v1_account_size = core::mem::size_of::<StewardStateAccount>();

    // ==========================================
    // STEP 4: Move sorted_score_indices to new location
    // ==========================================
    move_sorted_indices(&mut data, &offsets);

    // ==========================================
    // STEP 5: Write yield_scores as raw_scores
    // ==========================================
    write_raw_scores(&mut data, &yield_scores, offsets.v2_raw_scores);

    // ==========================================
    // STEP 6: Expand scores from u32 to u64 in place
    // ==========================================
    expand_scores_in_place(&mut data, &offsets, v1_account_size);

    // ==========================================
    // STEP 7: Update discriminator and finalize
    // ==========================================
    finalize_migration(&mut data);

    Ok(())
}

/// Container for all offset calculations to improve readability
struct MigrationOffsets {
    scores: usize,
    v1_sorted_indices: usize,
    v2_sorted_indices: usize,
    v2_raw_scores: usize,
}

impl MigrationOffsets {
    fn new() -> Self {
        const DISCRIMINATOR_SIZE: usize = 8;
        const STATE_TAG_SIZE: usize = 8;
        const U16_ARRAY_SIZE: usize = 2 * MAX_VALIDATORS;
        const U32_ARRAY_SIZE: usize = 4 * MAX_VALIDATORS;
        const U64_ARRAY_SIZE: usize = 8 * MAX_VALIDATORS;
        const DELEGATION_SIZE: usize = 8 * MAX_VALIDATORS;
        const BITMASK_SIZE: usize = core::mem::size_of::<crate::BitMask>();

        let base = DISCRIMINATOR_SIZE;

        // Scores come after state_tag and balances array
        let scores = base + STATE_TAG_SIZE + U64_ARRAY_SIZE;

        // In V1, sorted_indices comes after u32 scores array
        let v1_sorted_indices = scores + U32_ARRAY_SIZE;

        // In V2, sorted_indices comes after expanded u64 scores array
        let v2_sorted_indices = scores + U64_ARRAY_SIZE;

        // Calculate where raw_scores goes in V2 (where V1 padding was)
        // This is after all V1 fields plus 2-byte _padding0
        let v1_state_end = base + STATE_TAG_SIZE
            + U64_ARRAY_SIZE      // balances
            + U32_ARRAY_SIZE      // scores (u32 in V1)
            + U16_ARRAY_SIZE      // sorted_score_indices
            + U32_ARRAY_SIZE      // yield_scores
            + U16_ARRAY_SIZE      // sorted_yield_score_indices
            + DELEGATION_SIZE     // delegations
            + (4 * BITMASK_SIZE)  // 4 bitmasks
            + (7 * 8)            // 7 u64 fields
            + 4                  // 1 u32 field
            + 2; // 1 u16 field

        let v2_raw_scores = v1_state_end + 2; // Skip 2-byte _padding0

        Self {
            scores,
            v1_sorted_indices,
            v2_sorted_indices,
            v2_raw_scores,
        }
    }
}

/// Verify the account has the V1 discriminator
fn verify_v1_discriminator(data: &[u8]) -> Result<()> {
    if &data[0..8] != StewardStateAccount::DISCRIMINATOR {
        return Err(ProgramError::InvalidAccountData.into());
    }
    Ok(())
}

/// Extract yield_scores from V1 account using bytemuck
fn extract_v1_yield_scores(data: &[u8]) -> Vec<u32> {
    let v1_account: &StewardStateAccount =
        bytemuck::from_bytes(&data[8..8 + core::mem::size_of::<StewardStateAccount>()]);
    v1_account.state.yield_scores.to_vec()
}

/// Move sorted_score_indices from V1 location to V2 location
fn move_sorted_indices(data: &mut [u8], offsets: &MigrationOffsets) {
    const U16_ARRAY_SIZE: usize = 2 * MAX_VALIDATORS;

    data.copy_within(
        offsets.v1_sorted_indices..offsets.v1_sorted_indices + U16_ARRAY_SIZE,
        offsets.v2_sorted_indices,
    );
}

/// Write yield_scores as expanded raw_scores at the specified offset
fn write_raw_scores(data: &mut [u8], yield_scores: &[u32], offset: usize) {
    for (i, &score) in yield_scores.iter().enumerate() {
        let val_u64 = score as u64;
        let dst_offset = offset + i * 8;
        data[dst_offset..dst_offset + 8].copy_from_slice(&val_u64.to_le_bytes());
    }
}

/// Expand scores from u32 to u64 in place (working backwards to avoid overwriting)
fn expand_scores_in_place(data: &mut [u8], offsets: &MigrationOffsets, v1_account_size: usize) {
    for i in (0..MAX_VALIDATORS).rev() {
        // Read the u32 score
        let val_u32 = {
            let v1_account: &StewardStateAccount =
                bytemuck::from_bytes(&data[8..8 + v1_account_size]);
            v1_account.state.scores[i]
        };

        // Write as u64 at the expanded position
        let val_u64 = val_u32 as u64;
        let dst_offset = offsets.scores + i * 8;
        data[dst_offset..dst_offset + 8].copy_from_slice(&val_u64.to_le_bytes());
    }
}

/// Update discriminator and clear the old is_initialized field
fn finalize_migration(data: &mut [u8]) {
    // Update to V2 discriminator
    data[0..8].copy_from_slice(StewardStateAccountV2::DISCRIMINATOR);

    // Clear the old is_initialized field (becomes padding in V2)
    const DISCRIMINATOR_SIZE: usize = 8;
    let state_v2_size = core::mem::size_of::<StewardStateV2>();
    let account_fields_offset = DISCRIMINATOR_SIZE + state_v2_size;
    data[account_fields_offset] = 0;
}
