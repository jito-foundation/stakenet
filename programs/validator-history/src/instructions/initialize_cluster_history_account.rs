use crate::{constants::MAX_ALLOC_BYTES, state::ValidatorHistory, ClusterHistory};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct InitializeClusterHistoryAccount<'info> {
    #[account(
        init,
        payer = signer,
        space = MAX_ALLOC_BYTES,
        seeds = [ClusterHistory::SEED, ],
        bump
    )]
    pub validator_history_account: AccountLoader<'info, ValidatorHistory>,
    pub system_program: Program<'info, System>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handler(_: Context<InitializeClusterHistoryAccount>) -> Result<()> {
    Ok(())
}
