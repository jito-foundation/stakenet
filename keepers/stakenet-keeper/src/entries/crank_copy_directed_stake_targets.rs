use std::sync::Arc;

use kobe_client::client::KobeClient;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_pubkey::Pubkey;
use solana_sdk::{signature::Keypair, signer::Signer};
use stakenet_sdk::{
    models::{
        aggregate_accounts::AllStewardAccounts, errors::JitoTransactionError,
        submit_stats::SubmitStats,
    },
    utils::{
        instructions::{compute_bam_targets, compute_directed_stake_meta},
        transactions::{package_instructions, submit_packaged_transactions},
    },
};

/// Copy directed stake targets to [`DirectedStakeMeta`] account
pub(crate) async fn crank_copy_directed_stake_targets(
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    token_mint_address: &Pubkey,
    priority_fee: Option<u64>,
    kobe_client: &KobeClient,
) -> Result<SubmitStats, JitoTransactionError> {
    let mut stats = SubmitStats::default();

    let mut ixs = compute_directed_stake_meta(
        client.clone(),
        token_mint_address,
        &all_steward_accounts.stake_pool_address,
        &all_steward_accounts.config_address,
        &keypair.pubkey(),
        program_id,
    )
    .await
    .map_err(|e| JitoTransactionError::Custom(e.to_string()))?;

    let bam_ixs = compute_bam_targets(
        client.clone(),
        &kobe_client,
        &all_steward_accounts.config_address,
        &keypair.pubkey(),
        &program_id,
    )
    .await
    .map_err(|e| JitoTransactionError::Custom(e.to_string()))?;

    ixs.extend(bam_ixs);

    log::info!("Copy Directed Stake Targets");

    let update_txs_to_run = package_instructions(&ixs, 8, priority_fee, Some(1_400_000), None);
    let update_stats =
        submit_packaged_transactions(&client, update_txs_to_run, &keypair, Some(50), None).await?;

    stats.combine(&update_stats);

    Ok(stats)
}
