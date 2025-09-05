use anchor_lang::prelude::*;
use anchor_lang::Discriminator;

use crate::{
    state::{Config, StewardStateAccount, StewardStateAccountV2, StewardStateV2},
    utils::get_config_admin,
    STATE_PADDING_0_SIZE,
};

#[derive(Accounts)]
pub struct MigrateStateToV2<'info> {
    #[account(
        mut,
        seeds = [StewardStateAccountV2::SEED, config.key().as_ref()],
        bump,
    )]
    /// CHECK: We're reading this as V1 and writing as V2
    pub state_account: AccountInfo<'info>,

    pub config: AccountLoader<'info, Config>,

    #[account(address = get_config_admin(&config)?)]
    pub signer: Signer<'info>,
}

pub fn handler(ctx: Context<MigrateStateToV2>) -> Result<()> {
    // Deserialize the account data as V1
    let data = ctx.accounts.state_account.data.borrow();

    // Verify this is a V1 account by checking the discriminator
    let v1_discriminator = StewardStateAccount::DISCRIMINATOR;
    let account_discriminator = &data[0..8];
    if account_discriminator != v1_discriminator {
        return Err(ProgramError::InvalidAccountData.into());
    }

    // Skip the 8-byte discriminator and cast to V1 struct (zero-copy)
    // Ensure we have enough bytes
    let v1_size = std::mem::size_of::<StewardStateAccount>();
    if data.len() < 8 + v1_size {
        return Err(ProgramError::AccountDataTooSmall.into());
    }

    let v1_bytes = &data[8..8 + v1_size];
    let v1_account: &StewardStateAccount = bytemuck::from_bytes(v1_bytes);
    let v1_state = v1_account.state;

    // Create V2 state with converted values
    let v2_state = StewardStateV2 {
        // Preserve state machine position - no disruption
        state_tag: v1_state.state_tag,

        // Preserve validator tracking
        validator_lamport_balances: v1_state.validator_lamport_balances,

        // Convert scores from u32 to u64 (zero-extend)
        scores: {
            let mut scores = [0u64; crate::constants::MAX_VALIDATORS];
            for i in 0..crate::constants::MAX_VALIDATORS {
                scores[i] = v1_state.scores[i] as u64;
            }
            scores
        },
        sorted_score_indices: v1_state.sorted_score_indices,

        // Convert raw scores (previously yield_scores) from u32 to u64
        raw_scores: {
            let mut raw_scores = [0u64; crate::constants::MAX_VALIDATORS];
            for i in 0..crate::constants::MAX_VALIDATORS {
                raw_scores[i] = v1_state.yield_scores[i] as u64;
            }
            raw_scores
        },
        sorted_raw_score_indices: v1_state.sorted_yield_score_indices,

        // Preserve all operational state
        delegations: v1_state.delegations,
        instant_unstake: v1_state.instant_unstake,
        progress: v1_state.progress,
        validators_for_immediate_removal: v1_state.validators_for_immediate_removal,
        validators_to_remove: v1_state.validators_to_remove,

        // Preserve cycle metadata
        start_computing_scores_slot: v1_state.start_computing_scores_slot,
        current_epoch: v1_state.current_epoch,
        next_cycle_epoch: v1_state.next_cycle_epoch,
        num_pool_validators: v1_state.num_pool_validators,
        scoring_unstake_total: v1_state.scoring_unstake_total,
        instant_unstake_total: v1_state.instant_unstake_total,
        stake_deposit_unstake_total: v1_state.stake_deposit_unstake_total,
        status_flags: v1_state.status_flags,
        validators_added: v1_state.validators_added,

        // Reduced padding in V2
        _padding0: [0u8; STATE_PADDING_0_SIZE],
    };

    // Create V2 account wrapper
    let v2_account = StewardStateAccountV2 {
        state: v2_state,
        is_initialized: v1_account.is_initialized,
        bump: v1_account.bump,
        _padding: v1_account._padding,
    };

    // Write V2 back to account with new discriminator (same size, no realloc needed!)
    drop(data); // Drop the borrow before we mutably borrow
    let mut data = ctx.accounts.state_account.data.borrow_mut();

    // Write the V2 discriminator
    let v2_discriminator = StewardStateAccountV2::DISCRIMINATOR;
    data[0..8].copy_from_slice(v2_discriminator);

    // Write V2 account data after discriminator using bytemuck
    let v2_bytes = bytemuck::bytes_of(&v2_account);
    data[8..8 + v2_bytes.len()].copy_from_slice(v2_bytes);

    msg!("Successfully migrated steward state from V1 to V2");
    msg!("Preserved {} validators", v2_state.num_pool_validators);
    msg!("Current state: {:?}", v2_state.state_tag);
    msg!("Next cycle epoch: {}", v2_state.next_cycle_epoch);

    Ok(())
}
