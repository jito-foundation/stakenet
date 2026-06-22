use std::sync::Arc;

use solana_sdk::{signature::Keypair, signer::Signer};
use stakenet_sdk::{
    models::{
        aggregate_accounts::AllStewardAccounts, errors::JitoTransactionError,
        submit_stats::SubmitStats,
    },
    utils::{
        instructions::compute_coinbase_targets,
        transactions::{package_instructions, submit_packaged_transactions},
    },
};

use crate::state::keeper_config::KeeperConfig;

/// Copy directed stake targets to [`DirectedStakeMeta`] account
pub async fn crank_copy_directed_stake_targets(
    keeper_config: &KeeperConfig,
    keypair: Arc<Keypair>,
    all_steward_accounts: &AllStewardAccounts,
) -> Result<SubmitStats, JitoTransactionError> {
    let KeeperConfig {
        client,
        steward_program_id: program_id,
        token_mint: _token_mint_address,
        priority_fee_in_microlamports: priority_fee,
        kobe_client,
        coinbase_vote_pubkey,
        ..
    } = keeper_config;
    let mut stats = SubmitStats::default();

    // let normal_ixs = compute_directed_stake_meta(
    //     client.clone(),
    //     token_mint_address,
    //     &all_steward_accounts.stake_pool_address,
    //     &all_steward_accounts.config_address,
    //     &keypair.pubkey(),
    //     program_id,
    // )
    // .await
    // .map_err(|e| JitoTransactionError::Custom(e.to_string()))?;

    // log::info!("Normal copy directed stake targets: {}", normal_ixs.len());

    // let normal_txs_to_run =
    //     package_instructions(&normal_ixs, 8, Some(*priority_fee), Some(1_400_000), None);
    // let normal_stats =
    //     submit_packaged_transactions(client, normal_txs_to_run, &keypair, Some(50), None).await?;
    // stats.combine(&normal_stats);

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

    log::info!(
        "Coinbase delegation copy directed stake targets: {}",
        coinbase_delegation_ixs.len()
    );

    let coinbase_delegation_txs_to_run = package_instructions(
        &coinbase_delegation_ixs,
        8,
        Some(*priority_fee),
        Some(1_400_000),
        None,
    );
    let coinbase_delegation_stats = submit_packaged_transactions(
        client,
        coinbase_delegation_txs_to_run,
        &keypair,
        Some(50),
        None,
    )
    .await?;
    stats.combine(&coinbase_delegation_stats);

    Ok(stats)
}
