use std::sync::Arc;

use solana_sdk::{signature::Keypair, signer::Signer};
use stakenet_sdk::{
    models::{
        aggregate_accounts::AllStewardAccounts, errors::JitoTransactionError,
        submit_stats::SubmitStats,
    },
    utils::{
        instructions::{compute_coinbase_targets, compute_directed_stake_meta},
        transactions::{package_instructions, submit_packaged_transactions},
    },
};

use crate::entries::CU_COPY_DIRECTED_STAKE_TARGETS_PER_IX;
use crate::state::keeper_config::KeeperConfig;

/// Instructions packed per `copy_directed_stake_targets` transaction.
const TARGETS_PER_TX: usize = 8;

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

    log::info!(
        "Copying directed stake targets kind=normal instructions={}",
        normal_ixs.len()
    );

    let compute_limit = CU_COPY_DIRECTED_STAKE_TARGETS_PER_IX.saturating_mul(TARGETS_PER_TX as u32);

    let normal_txs_to_run = package_instructions(
        &normal_ixs,
        TARGETS_PER_TX,
        Some(*priority_fee),
        Some(compute_limit),
        None,
    );
    let normal_stats =
        submit_packaged_transactions(client, normal_txs_to_run, &keypair, Some(50), None).await?;
    stats.combine(&normal_stats);

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
        "Copying directed stake targets kind=coinbase_delegation instructions={}",
        coinbase_delegation_ixs.len()
    );

    let coinbase_delegation_txs_to_run = package_instructions(
        &coinbase_delegation_ixs,
        TARGETS_PER_TX,
        Some(*priority_fee),
        Some(compute_limit),
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
