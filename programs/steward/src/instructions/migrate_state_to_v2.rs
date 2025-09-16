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

    // Verify this is a V1 account
    if &data[0..8] != StewardStateAccount::DISCRIMINATOR {
        return Err(ProgramError::InvalidAccountData.into());
    }

    // Sizes
    let base = 8usize;
    let size_u32_array = 4 * MAX_VALIDATORS;
    let size_u16_array = 2 * MAX_VALIDATORS;
    let size_u64_array = 8 * MAX_VALIDATORS;
    let size_state_enum = core::mem::size_of::<StewardStateEnum>();
    let size_delegation = core::mem::size_of::<Delegation>() * MAX_VALIDATORS;
    let size_bitmask = core::mem::size_of::<BitMask>();

    // V1 layout offsets
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

    // Save sorted_score_indices before it gets overwritten
    let mut sorted_score_indices_data = vec![0u8; size_u16_array];
    sorted_score_indices_data.copy_from_slice(
        &data[base + off_v1_sorted_score_indices
            ..base + off_v1_sorted_score_indices + size_u16_array],
    );

    // Save yield_scores to convert to raw_scores
    let mut yield_scores_data = vec![0u8; size_u32_array];
    yield_scores_data.copy_from_slice(
        &data[base + off_v1_yield_scores..base + off_v1_yield_scores + size_u32_array],
    );

    // Expand scores from u32 to u64 in place (work backwards to avoid overwriting)
    for i in (0..MAX_VALIDATORS).rev() {
        let src_off = off_v1_scores + i * 4;
        let dst_off = off_v1_scores + i * 8;
        let mut buf4 = [0u8; 4];
        buf4.copy_from_slice(&data[base + src_off..base + src_off + 4]);
        let val = u32::from_le_bytes(buf4) as u64;
        let bytes = val.to_le_bytes();
        data[base + dst_off..base + dst_off + 8].copy_from_slice(&bytes);
    }

    // Move sorted_score_indices to its new position after expanded scores
    let off_v2_sorted_score_indices = off_v1_scores + size_u64_array;
    data[base + off_v2_sorted_score_indices..base + off_v2_sorted_score_indices + size_u16_array]
        .copy_from_slice(&sorted_score_indices_data);

    // Write raw_scores at the end (old padding location + 2 bytes)
    let off_v2_raw_scores = base + off_v1_padding + 2;
    for i in 0..MAX_VALIDATORS {
        let src_off = i * 4;
        let dst_off = off_v2_raw_scores + i * 8;
        let mut buf4 = [0u8; 4];
        buf4.copy_from_slice(&yield_scores_data[src_off..src_off + 4]);
        let val = u32::from_le_bytes(buf4) as u64;
        let bytes = val.to_le_bytes();
        data[dst_off..dst_off + 8].copy_from_slice(&bytes);
    }

    // Write the V2 discriminator
    data[0..8].copy_from_slice(StewardStateAccountV2::DISCRIMINATOR);

    // Handle account-level fields after the state struct
    // V1 layout after state: is_initialized (1 byte), bump (1 byte), _padding (6 bytes)
    // V2 layout after state: _padding0 (1 byte), bump (1 byte), _padding1 (6 bytes)

    // Calculate offset to account-level fields (after the state struct)
    let state_v2_size = core::mem::size_of::<StewardStateV2>();
    let account_fields_offset = base + state_v2_size;

    // Zero out _padding0 (was is_initialized in V1, now repurposed as padding)
    data[account_fields_offset] = 0;

    // bump field stays at the same position (offset + 1), no change needed
    // _padding1 stays at the same position as V1 _padding (offset + 2..offset + 8), no change needed

    Ok(())
}
