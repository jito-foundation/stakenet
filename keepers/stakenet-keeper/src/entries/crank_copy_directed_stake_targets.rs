use std::sync::Arc;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{instruction::Instruction, signature::Keypair, signer::Signer};
use stakenet_sdk::{
    models::{
        aggregate_accounts::AllStewardAccounts, errors::JitoTransactionError,
        submit_stats::SubmitStats,
    },
    utils::{
        instructions::{
            compute_coinbase_targets, compute_directed_stake_meta, compute_jitosol_prime_targets,
        },
        transactions::{package_instructions, submit_packaged_transactions},
    },
};

use crate::state::keeper_config::KeeperConfig;

/// Packages and submits a batch of `CopyDirectedStakeTargets` instructions.
async fn submit_targets(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    priority_fee: u64,
    kind: &str,
    ixs: &[Instruction],
) -> Result<SubmitStats, JitoTransactionError> {
    log::info!(
        "Copying directed stake targets kind={kind} instructions={}",
        ixs.len()
    );

    let txs_to_run = package_instructions(ixs, 8, Some(priority_fee), Some(1_400_000), None);

    Ok(submit_packaged_transactions(client, txs_to_run, keypair, Some(50), None).await?)
}

/// Copy directed stake targets to [`DirectedStakeMeta`] account
pub async fn crank_copy_directed_stake_targets(
    keeper_config: &KeeperConfig,
    keypair: Arc<Keypair>,
    all_steward_accounts: &AllStewardAccounts,
) -> Result<SubmitStats, JitoTransactionError> {
    let KeeperConfig {
        client,
        steward_program_id: program_id,
        token_mint,
        priority_fee_in_microlamports: priority_fee,
        kobe_client,
        coinbase_vote_pubkey,
        jitosol_prime_vote_pubkey,
        jitosol_prime_share_bps,
        ..
    } = keeper_config;
    let mut stats = SubmitStats::default();

    let normal_ixs = compute_directed_stake_meta(
        client.clone(),
        token_mint,
        &all_steward_accounts.stake_pool_address,
        &all_steward_accounts.config_address,
        &keypair.pubkey(),
        program_id,
    )
    .await
    .map_err(|e| JitoTransactionError::Custom(e.to_string()))?;

    let coinbase_delegation_ixs = compute_coinbase_targets(
        client.clone(),
        kobe_client,
        &all_steward_accounts.config_address,
        &keypair.pubkey(),
        program_id,
        coinbase_vote_pubkey,
    )
    .await
    .map_err(|e| JitoTransactionError::Custom(e.to_string()))?;

    let jitosol_prime_ixs = compute_jitosol_prime_targets(
        client.clone(),
        &all_steward_accounts.config_address,
        &keypair.pubkey(),
        program_id,
        jitosol_prime_vote_pubkey,
        *jitosol_prime_share_bps,
    )
    .await
    .map_err(|e| JitoTransactionError::Custom(e.to_string()))?;

    let normal_stats =
        submit_targets(client, &keypair, *priority_fee, "normal", &normal_ixs).await?;
    stats.combine(&normal_stats);

    let coinbase_and_jitosol_prime_ixs = [coinbase_delegation_ixs, jitosol_prime_ixs].concat();
    let coinbase_and_jitosol_prime_stats = submit_targets(
        client,
        &keypair,
        *priority_fee,
        "coinbase_and_jitosol_prime",
        &coinbase_and_jitosol_prime_ixs,
    )
    .await?;
    stats.combine(&coinbase_and_jitosol_prime_stats);

    Ok(stats)
}
