use anchor_lang::prelude::*;

use crate::{constants::MAX_ALLOC_BYTES, StakeAggregation};

#[derive(Accounts)]
pub struct InitializeStakeAggregationAccount<'info> {
    #[account(
        init,
        payer = signer,
        space = MAX_ALLOC_BYTES,
        seeds = [StakeAggregation::SEED],
        bump
    )]
    pub stake_aggregation_account: AccountLoader<'info, StakeAggregation>,
    pub system_program: Program<'info, System>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

/// Initializes the [StakeAggregation] account
///
/// Leaves data zero initialized, as the account needs to reallocated many times before we can
/// start using the buffer for aggregation of validator stake amounts.
pub fn handle_initialize_stake_aggregation_account(
    _ctx: Context<InitializeStakeAggregationAccount>,
) -> Result<()> {
    Ok(())
}
