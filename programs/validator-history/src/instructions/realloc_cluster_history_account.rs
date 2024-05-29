use crate::{constants::MAX_ALLOC_BYTES, ClusterHistory, ClusterHistoryEntry};
use anchor_lang::prelude::*;

fn get_realloc_size(account_info: &AccountInfo) -> usize {
    let account_size = account_info.data_len();

    // If account is already over-allocated, don't try to shrink
    if account_size < ClusterHistory::SIZE {
        ClusterHistory::SIZE.min(account_size + MAX_ALLOC_BYTES)
    } else {
        account_size
    }
}

fn is_initialized(account_info: &AccountInfo) -> Result<bool> {
    let account_data = account_info.as_ref().try_borrow_data()?;

    // discriminator + version_number
    let vote_account_pubkey_bytes = account_data[(8 + 8)..(8 + 8 + 32)].to_vec();

    // If pubkey is all zeroes, then it's not initialized
    Ok(vote_account_pubkey_bytes.iter().any(|&x| x != 0))
}

#[derive(Accounts)]
pub struct ReallocClusterHistoryAccount<'info> {
    #[account(
        mut,
        realloc = get_realloc_size(cluster_history_account.as_ref()),
        realloc::payer = signer,
        realloc::zero = false,
        seeds = [ClusterHistory::SEED],
        bump
    )]
    pub cluster_history_account: AccountLoader<'info, ClusterHistory>,
    pub system_program: Program<'info, System>,
    #[account(mut)]
    pub signer: Signer<'info>,
}

pub fn handle_realloc_cluster_history_account(
    ctx: Context<ReallocClusterHistoryAccount>,
) -> Result<()> {
    let account_size = ctx.accounts.cluster_history_account.as_ref().data_len();
    if account_size >= ClusterHistory::SIZE
        && !is_initialized(ctx.accounts.cluster_history_account.as_ref())?
    {
        // Can actually initialize values now that the account is proper size
        let mut cluster_history_account = ctx.accounts.cluster_history_account.load_mut()?;

        cluster_history_account.bump = ctx.bumps.cluster_history_account;
        cluster_history_account.struct_version = 0;
        cluster_history_account.history.idx =
            (cluster_history_account.history.arr.len() - 1) as u64;
        for _ in 0..cluster_history_account.history.arr.len() {
            cluster_history_account
                .history
                .push(ClusterHistoryEntry::default());
        }
        cluster_history_account.history.is_empty = 1;
    }

    Ok(())
}
