use super::keeper_operations::{check_flag, KeeperOperations};
use crate::state::keeper_config::KeeperConfig;
use crate::{
    entries::priority_fee_commission_entry::ValidatorPriorityFeeCommissionEntry,
    state::keeper_state::KeeperState,
};
use log::error as log_error;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::datapoint_error;
use solana_sdk::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use stakenet_sdk::models::entries::UpdateInstruction;
use stakenet_sdk::models::errors::JitoTransactionError;
use stakenet_sdk::models::submit_stats::SubmitStats;
use stakenet_sdk::utils::transactions::submit_chunk_instructions;
use std::sync::Arc;
use validator_history::MerkleRootUploadAuthority;

fn _get_operation() -> KeeperOperations {
    KeeperOperations::PriorityFeeCommission
}

fn _should_run() -> bool {
    true
}

#[allow(clippy::too_many_arguments)]
async fn _process(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    priority_fee_distribution_program_id: &Pubkey,
    keeper_state: &KeeperState,
    retry_count: u16,
    confirmation_time: u64,
    priority_fee_in_microlamports: u64,
    lookback_epochs: u64,
    lookback_start_offset_epochs: u64,
    no_pack: bool,
) -> Result<SubmitStats, JitoTransactionError> {
    update_priority_fee_commission(
        client,
        keypair,
        program_id,
        priority_fee_distribution_program_id,
        keeper_state,
        retry_count,
        confirmation_time,
        priority_fee_in_microlamports,
        lookback_epochs,
        lookback_start_offset_epochs,
        no_pack,
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
    let priority_fee_distribution_program_id = &keeper_config.priority_fee_distribution_program_id;
    let priority_fee_in_microlamports = keeper_config.priority_fee_in_microlamports;

    let operation = _get_operation();
    let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) =
        keeper_state.copy_runs_errors_and_txs_for_epoch(operation);

    let should_run = _should_run() && check_flag(keeper_config.run_flags, operation);

    if should_run {
        match _process(
            client,
            keypair,
            program_id,
            priority_fee_distribution_program_id,
            keeper_state,
            keeper_config.tx_retry_count,
            keeper_config.tx_confirmation_seconds,
            priority_fee_in_microlamports,
            keeper_config.lookback_epochs,
            keeper_config.lookback_start_offset_epochs,
            keeper_config.no_pack,
        )
        .await
        {
            Ok(stats) => {
                for message in stats.results.iter().chain(stats.results.iter()) {
                    if let Err(e) = message {
                        log_error!("ERROR: {}", e);
                        datapoint_error!(
                            "priority-fee-commission-error",
                            ("error", e.to_string(), String),
                        );
                        errors_for_epoch += 1;
                    } else {
                        txs_for_epoch += 1;
                    }
                }
                if stats.errors == 0 {
                    runs_for_epoch += 1;
                }
            }
            Err(e) => {
                datapoint_error!(
                    "priority-fee-commission-error",
                    ("error", e.to_string(), String),
                );
                errors_for_epoch += 1;
            }
        };
    }

    (operation, runs_for_epoch, errors_for_epoch, txs_for_epoch)
}

// ----------------- OPERATION SPECIFIC FUNCTIONS -----------------

#[allow(clippy::too_many_arguments)]
pub async fn update_priority_fee_commission(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    priority_fee_distribution_program_id: &Pubkey,
    keeper_state: &KeeperState,
    retry_count: u16,
    confirmation_time: u64,
    priority_fee_in_microlamports: u64,
    lookback_epochs: u64,
    lookback_start_offset_epochs: u64,
    _no_pack: bool,
) -> Result<SubmitStats, JitoTransactionError> {
    // Only update Epoch N-1 since, priority fees are not yet finalized
    let epoch_info = &keeper_state.epoch_info;
    let current_epoch = epoch_info.epoch;

    let mut all_update_instructions: Vec<Instruction> = Vec::new();

    let epoch_range = (current_epoch - lookback_epochs - lookback_start_offset_epochs)
        ..(current_epoch - lookback_start_offset_epochs);
    for epoch in epoch_range {
        let update_instructions = keeper_state
            .validator_history_map
            .keys()
            .filter_map(|vote_account| {
                if let Some(validator_history) =
                    keeper_state.validator_history_map.get(vote_account)
                {
                    let should_update = validator_history.history.arr.iter().any(|entry| {
                        entry.epoch as u64 == epoch
                            && (entry.priority_fee_merkle_root_upload_authority
                                == MerkleRootUploadAuthority::Unset || entry.total_priority_fees == 0)
                    });

                    if !should_update {
                        return None;
                    }
                }

                Some(
                    ValidatorPriorityFeeCommissionEntry::new(
                        vote_account,
                        epoch,
                        program_id,
                        priority_fee_distribution_program_id,
                        &keypair.pubkey(),
                    )
                    .update_instruction(),
                )
            })
            .collect::<Vec<_>>();

        all_update_instructions.extend(update_instructions);
    }

    // Don't bundle under 160, such that TXs wont fail in larger bundles
    let chunk_size = match all_update_instructions.len() {
        0..=160 => 1,
        _ => 8,
    };

    let submit_result = submit_chunk_instructions(
        client,
        all_update_instructions,
        keypair,
        priority_fee_in_microlamports,
        retry_count,
        confirmation_time,
        None,
        chunk_size,
    )
    .await;

    submit_result.map_err(|e| e.into())
}
