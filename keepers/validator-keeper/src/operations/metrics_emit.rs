use std::sync::Arc;

/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/
use crate::state::keeper_state::KeeperState;
use crate::{derive_cluster_history_address, get_balance_with_retry};
use anchor_lang::AccountDeserialize;
use log::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::datapoint_info;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use validator_history::{ClusterHistory, ValidatorHistoryEntry};

use super::keeper_operations::KeeperOperations;

fn _get_operation() -> KeeperOperations {
    return KeeperOperations::EmitMetrics;
}

fn _should_run() -> bool {
    true
}

async fn _process(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    keeper_state: &KeeperState,
) -> Result<(), Box<dyn std::error::Error>> {
    emit_validator_history_metrics(client, program_id, &keypair.pubkey(), keeper_state).await
}

fn _emit(runs_for_epoch: i64, errors_for_epoch: i64) {
    datapoint_info!(
        "emit-metrics-stats",
        ("runs_for_epoch", runs_for_epoch, i64),
        ("errors_for_epoch", errors_for_epoch, i64),
    );
}

pub async fn fire_and_emit(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64) {
    let operation = _get_operation();
    let (mut runs_for_epoch, mut errors_for_epoch) =
        keeper_state.copy_runs_and_errors_for_epoch(operation.clone());

    match _process(client, keypair, program_id, keeper_state).await {
        Ok(_) => {
            runs_for_epoch += 1;
        }
        Err(e) => {
            errors_for_epoch += 1;
            error!("Failed to emit validator history metrics: {}", e);
        }
    }

    _emit(runs_for_epoch as i64, errors_for_epoch as i64);

    (operation, runs_for_epoch, errors_for_epoch)
}

// ----------------- OPERATION SPECIFIC FUNCTIONS -----------------
pub async fn emit_validator_history_metrics(
    client: &RpcClient,
    program_id: &Pubkey,
    keeper_address: &Pubkey,
    keeper_state: &KeeperState,
) -> Result<(), Box<dyn std::error::Error>> {
    let epoch_info = &keeper_state.epoch_info;
    let vote_accounts = &keeper_state.vote_account_map.values().collect::<Vec<_>>();
    let validator_histories = &keeper_state
        .validator_history_map
        .values()
        .collect::<Vec<_>>();

    let mut ips = 0;
    let mut versions = 0;
    let mut types = 0;
    let mut mev_comms = 0;
    let mut comms = 0;
    let mut epoch_credits = 0;
    let mut stakes = 0;
    let num_validators = validator_histories.len();
    let default = ValidatorHistoryEntry::default();
    for validator_history in validator_histories {
        if let Some(entry) = validator_history.history.last() {
            if entry.epoch as u64 != epoch_info.epoch {
                continue;
            }
            if entry.ip != default.ip {
                ips += 1;
            }
            if !(entry.version.major == default.version.major
                && entry.version.minor == default.version.minor
                && entry.version.patch == default.version.patch)
            {
                versions += 1;
            }
            if entry.client_type != default.client_type {
                types += 1;
            }
            if entry.mev_commission != default.mev_commission {
                mev_comms += 1;
            }
            if entry.commission != default.commission {
                comms += 1;
            }
            if entry.epoch_credits != default.epoch_credits {
                epoch_credits += 1;
            }
            if entry.activated_stake_lamports != default.activated_stake_lamports {
                stakes += 1;
            }
        }
    }

    let cluster_history_address = derive_cluster_history_address(program_id);
    let cluster_history_account = client.get_account(&cluster_history_address).await?;
    let cluster_history =
        ClusterHistory::try_deserialize(&mut cluster_history_account.data.as_slice())?;

    let mut cluster_history_blocks: i64 = 0;
    let cluster_history_entry = cluster_history.history.last();
    if let Some(cluster_history) = cluster_history_entry {
        // Looking for previous epoch to be updated
        if cluster_history.epoch as u64 == epoch_info.epoch.saturating_sub(1) {
            cluster_history_blocks = 1;
        }
    }

    let get_vote_accounts_count = vote_accounts.len();

    let keeper_balance = get_balance_with_retry(client, keeper_address.clone()).await?;

    //TODO update with newest metrics
    datapoint_info!(
        "validator-history-stats",
        ("num_validator_histories", num_validators, i64),
        ("num_ips", ips, i64),
        ("num_versions", versions, i64),
        ("num_client_types", types, i64),
        ("num_mev_commissions", mev_comms, i64),
        ("num_commissions", comms, i64),
        ("num_epoch_credits", epoch_credits, i64),
        ("num_stakes", stakes, i64),
        ("cluster_history_blocks", cluster_history_blocks, i64),
        ("slot_index", epoch_info.slot_index, i64),
        (
            "num_get_vote_accounts_responses",
            get_vote_accounts_count,
            i64
        ),
    );

    datapoint_info!(
        "stakenet-keeper-stats",
        ("balance_lamports", keeper_balance, i64),
    );

    Ok(())
}
