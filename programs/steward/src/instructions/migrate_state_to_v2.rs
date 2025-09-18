use anchor_lang::prelude::*;
use anchor_lang::Discriminator;

use crate::constants::MAX_VALIDATORS;
use crate::state::{Config, StewardStateAccount, StewardStateAccountV2};

// V1 and V2 have the same size, so we can use the same range for both
const ACCOUNT_SIZE: usize = core::mem::size_of::<StewardStateAccount>();
const ACCOUNT_RANGE: core::ops::Range<usize> = 8..8 + ACCOUNT_SIZE;

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
    // STEP 2: Update discriminator to V2
    // ==========================================
    data[0..8].copy_from_slice(StewardStateAccountV2::DISCRIMINATOR);

    // ==========================================
    // STEP 3: Copy yield_scores to raw_scores location (safe - no overlap)
    // ==========================================
    for i in 0..MAX_VALIDATORS {
        // Grab value from v1 location
        let yield_score = {
            let v1_account: &StewardStateAccount = bytemuck::from_bytes(&data[ACCOUNT_RANGE]);
            v1_account.state.yield_scores[i]
        };
        // Write to v2 location
        let v2_account: &mut StewardStateAccountV2 =
            bytemuck::from_bytes_mut(&mut data[ACCOUNT_RANGE]);
        v2_account.state.raw_scores[i] = yield_score as u64;
    }

    // ==========================================
    // STEP 4: Copy sorted_score_indices to new V2 location
    // ==========================================
    for i in 0..MAX_VALIDATORS {
        // Grab value from v1 location
        let val = {
            let v1_account: &StewardStateAccount = bytemuck::from_bytes(&data[ACCOUNT_RANGE]);
            v1_account.state.sorted_score_indices[i]
        };
        // Write to v2 location
        let v2_account: &mut StewardStateAccountV2 =
            bytemuck::from_bytes_mut(&mut data[ACCOUNT_RANGE]);
        v2_account.state.sorted_score_indices[i] = val;
    }

    // ==========================================
    // STEP 5: Expand scores from u32 to u64 (backwards to avoid overwriting)
    // ==========================================
    for i in (0..MAX_VALIDATORS).rev() {
        // Grab value from v1 location
        let score_u32 = {
            let v1_account: &StewardStateAccount = bytemuck::from_bytes(&data[ACCOUNT_RANGE]);
            v1_account.state.scores[i]
        };
        // Write to v2 location
        let v2_account: &mut StewardStateAccountV2 =
            bytemuck::from_bytes_mut(&mut data[ACCOUNT_RANGE]);
        v2_account.state.scores[i] = score_u32 as u64;
    }

    // Clear the old is_initialized field (now padding)
    let v2_account: &mut StewardStateAccountV2 = bytemuck::from_bytes_mut(&mut data[ACCOUNT_RANGE]);
    v2_account._padding0 = 0;

    Ok(())
}

/// Verify the account has the V1 discriminator
fn verify_v1_discriminator(data: &[u8]) -> Result<()> {
    if &data[0..8] != StewardStateAccount::DISCRIMINATOR {
        return Err(ProgramError::InvalidAccountData.into());
    }
    Ok(())
}
