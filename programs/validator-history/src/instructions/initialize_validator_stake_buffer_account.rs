use anchor_lang::prelude::*;

use crate::{constants::MAX_ALLOC_BYTES, ValidatorStakeBuffer};

#[derive(Accounts)]
pub struct InitializeValidatorStakeBufferAccount<'info> {
    #[account(
        init,
        payer = signer,
        space = MAX_ALLOC_BYTES,
        seeds = [ValidatorStakeBuffer::SEED],
        bump
    )]
    pub validator_stake_buffer_account: AccountLoader<'info, ValidatorStakeBuffer>,
    pub system_program: Program<'info, System>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

/// Initializes the [ValidatorStakeBuffer] account
///
/// Leaves data zero initialized, as the account needs to reallocated many times before we can
/// start using the buffer for aggregation of validator stake.
pub fn handle_initialize_validator_stake_buffer_account(
    _ctx: Context<InitializeValidatorStakeBufferAccount>,
) -> Result<()> {
    Ok(())
}
