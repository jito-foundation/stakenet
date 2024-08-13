/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/

use crate::state::keeper_config::KeeperConfig;
use crate::state::keeper_state::KeeperState;
use anchor_lang::{InstructionData, ToAccountMetas};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::datapoint_error;
use solana_sdk::{
    compute_budget,
    epoch_info::EpochInfo,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use stakenet_sdk::{
    models::{errors::JitoTransactionExecutionError, submit_stats::SubmitStats},
    utils::{accounts::get_cluster_history_address, transactions::submit_transactions},
};
use std::sync::Arc;

use super::keeper_operations::{check_flag, KeeperOperations};

fn _get_operation() -> KeeperOperations {
    KeeperOperations::ClusterHistory
}

fn _should_run(epoch_info: &EpochInfo, runs_for_epoch: u64) -> bool {
    // Run at 0.1%, 50% and 90% completion of epoch
    (epoch_info.slot_index > epoch_info.slots_in_epoch / 1000 && runs_for_epoch < 1)
        || (epoch_info.slot_index > epoch_info.slots_in_epoch * 5 / 10 && runs_for_epoch < 2)
        || (epoch_info.slot_index > epoch_info.slots_in_epoch * 9 / 10 && runs_for_epoch < 3)
}

async fn _process(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    priority_fee_in_microlamports: u64,
    retry_count: u16,
    confirmation_time: u64,
) -> Result<SubmitStats, JitoTransactionExecutionError> {
    update_cluster_info(
        client,
        keypair,
        program_id,
        priority_fee_in_microlamports,
        retry_count,
        confirmation_time,
    )
    .await
}

pub async fn fire(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64, u64) {
    let client = &keeper_config.client;
    let keypair = &keeper_config.keypair;
    let program_id = &keeper_config.validator_history_program_id;
    let priority_fee_in_microlamports = keeper_config.priority_fee_in_microlamports;
    let retry_count = keeper_config.tx_retry_count;
    let confirmation_time = keeper_config.tx_confirmation_seconds;

    let operation = _get_operation();
    let epoch_info = &keeper_state.epoch_info;

    let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) =
        keeper_state.copy_runs_errors_and_txs_for_epoch(operation);

    let should_run =
        _should_run(epoch_info, runs_for_epoch) && check_flag(keeper_config.run_flags, operation);

    if should_run {
        match _process(
            client,
            keypair,
            program_id,
            priority_fee_in_microlamports,
            retry_count,
            confirmation_time,
        )
        .await
        {
            Ok(stats) => {
                for message in stats.results.iter() {
                    if let Err(e) = message {
                        datapoint_error!("cluster-history-error", ("error", e.to_string(), String),);
                    } else {
                        txs_for_epoch += 1;
                    }
                }

                if stats.errors == 0 {
                    runs_for_epoch += 1;
                }
            }
            Err(e) => {
                datapoint_error!("cluster-history-error", ("error", e.to_string(), String),);
                errors_for_epoch += 1;
            }
        };
    }

    (operation, runs_for_epoch, errors_for_epoch, txs_for_epoch)
}

// ----------------- OPERATION SPECIFIC FUNCTIONS -----------------

pub fn get_update_cluster_info_instructions(
    program_id: &Pubkey,
    keypair: &Pubkey,
    priority_fee_in_microlamports: u64,
) -> Vec<Instruction> {
    let cluster_history_account = get_cluster_history_address(program_id);

    let priority_fee_ix = compute_budget::ComputeBudgetInstruction::set_compute_unit_price(
        priority_fee_in_microlamports,
    );
    let heap_request_ix = compute_budget::ComputeBudgetInstruction::request_heap_frame(256 * 1024);
    let compute_budget_ix =
        compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    let update_instruction = Instruction {
        program_id: *program_id,
        accounts: validator_history::accounts::CopyClusterInfo {
            cluster_history_account,
            slot_history: solana_program::sysvar::slot_history::id(),
            signer: *keypair,
        }
        .to_account_metas(None),
        data: validator_history::instruction::CopyClusterInfo {}.data(),
    };

    vec![
        priority_fee_ix,
        heap_request_ix,
        compute_budget_ix,
        update_instruction,
    ]
}

pub async fn update_cluster_info(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    priority_fee_in_microlamports: u64,
    retry_count: u16,
    confirmation_time: u64,
) -> Result<SubmitStats, JitoTransactionExecutionError> {
    let ixs = get_update_cluster_info_instructions(
        program_id,
        &keypair.pubkey(),
        priority_fee_in_microlamports,
    );

    submit_transactions(client, vec![ixs], keypair, retry_count, confirmation_time).await
}
