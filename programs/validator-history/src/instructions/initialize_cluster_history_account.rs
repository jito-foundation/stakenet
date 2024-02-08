use crate::{constants::MAX_ALLOC_BYTES, ClusterHistory};
use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct InitializeClusterHistoryAccount<'info> {
    #[account(
        init,
        payer = signer,
        space = MAX_ALLOC_BYTES,
        seeds = [ClusterHistory::SEED],
        bump
    )]
    pub cluster_history_account: AccountLoader<'info, ClusterHistory>,
    pub system_program: Program<'info, System>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handle_initialize_cluster_history_account(
    _: Context<InitializeClusterHistoryAccount>,
) -> Result<()> {
    Ok(())
}
