use anchor_lang::prelude::*;
use anchor_lang::Discriminator;

use crate::{
    state::{Config, StewardStateAccount, StewardStateAccountV2},
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
    let v1_is_initialized = v1_account.is_initialized;
    let v1_bump = v1_account.bump;
    let v1_padding = v1_account._padding;

    // Write V2 directly to account data to avoid stack allocation
    drop(data); // Drop the borrow before we mutably borrow
    let mut data = ctx.accounts.state_account.data.borrow_mut();

    // Write the V2 discriminator
    let v2_discriminator = StewardStateAccountV2::DISCRIMINATOR;
    data[0..8].copy_from_slice(v2_discriminator);

    // Write V2 account data directly, field by field to avoid stack allocation
    let v2_bytes = &mut data[8..];
    let mut offset = 0;

    // Helper to write bytes and advance offset
    let mut write_bytes = |bytes: &[u8]| {
        v2_bytes[offset..offset + bytes.len()].copy_from_slice(bytes);
        offset += bytes.len();
    };

    // Write StewardStateV2 fields in order
    write_bytes(bytemuck::bytes_of(&v1_state.state_tag));
    write_bytes(bytemuck::bytes_of(&v1_state.validator_lamport_balances));

    // Convert and write scores (u32 to u64)
    for i in 0..crate::constants::MAX_VALIDATORS {
        let score_u64 = v1_state.scores[i] as u64;
        write_bytes(&score_u64.to_le_bytes());
    }

    write_bytes(bytemuck::bytes_of(&v1_state.sorted_score_indices));

    // Convert and write raw_scores (previously yield_scores, u32 to u64)
    for i in 0..crate::constants::MAX_VALIDATORS {
        let raw_score_u64 = v1_state.yield_scores[i] as u64;
        write_bytes(&raw_score_u64.to_le_bytes());
    }

    write_bytes(bytemuck::bytes_of(&v1_state.sorted_yield_score_indices));
    write_bytes(bytemuck::bytes_of(&v1_state.delegations));
    write_bytes(bytemuck::bytes_of(&v1_state.instant_unstake));
    write_bytes(bytemuck::bytes_of(&v1_state.progress));
    write_bytes(bytemuck::bytes_of(
        &v1_state.validators_for_immediate_removal,
    ));
    write_bytes(bytemuck::bytes_of(&v1_state.validators_to_remove));
    write_bytes(bytemuck::bytes_of(&v1_state.start_computing_scores_slot));
    write_bytes(bytemuck::bytes_of(&v1_state.current_epoch));
    write_bytes(bytemuck::bytes_of(&v1_state.next_cycle_epoch));
    write_bytes(bytemuck::bytes_of(&v1_state.num_pool_validators));
    write_bytes(bytemuck::bytes_of(&v1_state.scoring_unstake_total));
    write_bytes(bytemuck::bytes_of(&v1_state.instant_unstake_total));
    write_bytes(bytemuck::bytes_of(&v1_state.stake_deposit_unstake_total));
    write_bytes(bytemuck::bytes_of(&v1_state.status_flags));
    write_bytes(bytemuck::bytes_of(&v1_state.validators_added));

    // Write reduced padding for V2
    let padding = [0u8; STATE_PADDING_0_SIZE];
    write_bytes(&padding);

    // Write StewardStateAccountV2 wrapper fields
    write_bytes(bytemuck::bytes_of(&v1_is_initialized));
    write_bytes(bytemuck::bytes_of(&v1_bump));
    write_bytes(bytemuck::bytes_of(&v1_padding));

    msg!("Successfully migrated steward state from V1 to V2");
    msg!("Preserved {} validators", v1_state.num_pool_validators);
    msg!("Current state: {:?}", v1_state.state_tag);
    msg!("Next cycle epoch: {}", v1_state.next_cycle_epoch);

    Ok(())
}
