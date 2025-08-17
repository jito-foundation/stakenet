use std::{collections::HashMap, sync::Arc};

use anchor_lang::{prelude::SlotHistory, AnchorDeserialize};
use futures::future::join_all;
use jito_priority_fee_distribution::state::PriorityFeeDistributionAccount;
use log::{error, info};
use regex::Regex;
use rusqlite::Connection;
use solana_client::{
    client_error::ClientErrorKind, nonblocking::rpc_client::RpcClient, rpc_config::RpcBlockConfig,
    rpc_request::RpcError,
};
use solana_metrics::{datapoint_error, datapoint_info};
use solana_sdk::{
    commitment_config::CommitmentConfig, pubkey::Pubkey, signature::Keypair, signer::Signer,
    slot_history,
};
use solana_transaction_status::{
    RewardType, TransactionDetails, UiConfirmedBlock, UiTransactionEncoding,
};
use stakenet_sdk::{
    models::{cluster::Cluster, entries::UpdateInstruction, submit_stats::SubmitStats},
    utils::transactions::submit_chunk_instructions,
};

use crate::{
    entries::priority_fee_commission_entry::derive_priority_fee_distribution_account_address,
    operations::{
        block_metadata::db::DBSlotInfo,
        keeper_operations::{check_flag, KeeperOperations},
    },
    state::{keeper_config::KeeperConfig, keeper_state::KeeperState},
};

use super::errors::BlockMetadataKeeperError;

fn _get_operation() -> KeeperOperations {
    KeeperOperations::BlockMetadataKeeper
}

fn _should_run() -> bool {
    true
}

#[derive(Debug, Default)]
pub struct AggregateBlockInfo {
    pub epoch: u64,
    pub leader_slots: u32,
    pub blocks_produced: u32,
    pub priority_fees: i64,
}

impl AggregateBlockInfo {
    pub fn new(epoch: u64) -> Self {
        Self {
            epoch,
            leader_slots: 0,
            blocks_produced: 0,
            priority_fees: 0,
        }
    }
    pub fn increment_data(&mut self, leader_slots: u32, blocks_produced: u32, priority_fees: i64) {
        self.leader_slots += leader_slots;
        self.blocks_produced += blocks_produced;
        self.priority_fees += priority_fees;
    }
}

pub async fn fire(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64, u64) {
    let client = &keeper_config.client;
    let mut mutex_guard = keeper_config.mut_connection().await;
    let sqlite_connection: &mut Connection = &mut mutex_guard;
    let block_metadata_interval = keeper_config.block_metadata_interval;
    let program_id = &keeper_config.validator_history_program_id;
    let priority_fee_oracle_authority_keypair = keeper_config
        .priority_fee_oracle_authority_keypair
        .as_ref()
        .unwrap();

    let operation = _get_operation();

    let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) =
        keeper_state.copy_runs_errors_and_txs_for_epoch(operation);

    let should_run = _should_run() && check_flag(keeper_config.run_flags, operation);

    if should_run {
        match _process(
            client,
            sqlite_connection,
            block_metadata_interval,
            &keeper_config.redundant_rpc_urls,
            keeper_state,
            program_id,
            &keeper_config.priority_fee_distribution_program_id,
            priority_fee_oracle_authority_keypair,
            keeper_config.tx_retry_count,
            keeper_config.tx_confirmation_seconds,
            keeper_config.priority_fee_in_microlamports,
            keeper_config.no_pack,
            keeper_config.cluster,
            keeper_config.lookback_epochs,
        )
        .await
        {
            Ok(stats) => {
                for message in stats.results.iter().chain(stats.results.iter()) {
                    if let Err(e) = message {
                        datapoint_error!(
                            "block-metadata-keeper-error",
                            ("error", e.to_string(), String),
                        );
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
                    "block-metadata-keeper-error",
                    ("error", e.to_string(), String),
                );
                errors_for_epoch += 1;
            }
        }
    }

    (operation, runs_for_epoch, errors_for_epoch, txs_for_epoch)
}

#[allow(clippy::too_many_arguments)]
async fn _process(
    client: &Arc<RpcClient>,
    sqlite_connection: &mut Connection,
    block_metadata_interval: u64,
    maybe_redundant_rpc_urls: &Option<Arc<Vec<RpcClient>>>,
    keeper_state: &KeeperState,
    program_id: &Pubkey,
    priority_fee_distribution_program_id: &Pubkey,
    priority_fee_oracle_authority_keypair: &Arc<Keypair>,
    retry_count: u16,
    confirmation_time: u64,
    priority_fee_in_microlamports: u64,
    no_pack: bool,
    cluster: Cluster,
    lookback_epochs: u64,
) -> Result<SubmitStats, Box<dyn std::error::Error>> {
    update_block_metadata(
        client,
        sqlite_connection,
        block_metadata_interval,
        maybe_redundant_rpc_urls,
        keeper_state,
        program_id,
        priority_fee_distribution_program_id,
        priority_fee_oracle_authority_keypair,
        retry_count,
        confirmation_time,
        priority_fee_in_microlamports,
        no_pack,
        cluster,
        lookback_epochs,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn update_block_metadata(
    client: &Arc<RpcClient>,
    sqlite_connection: &mut Connection,
    _block_metadata_interval: u64, //TODO take out
    maybe_redundant_rpc_urls: &Option<Arc<Vec<RpcClient>>>,
    keeper_state: &KeeperState,
    program_id: &Pubkey,
    priority_fee_distribution_program_id: &Pubkey,
    priority_fee_oracle_authority_keypair: &Arc<Keypair>,
    retry_count: u16,
    confirmation_time: u64,
    priority_fee_in_microlamports: u64,
    _no_pack: bool, //TODO take out
    cluster: Cluster,
    lookback_epochs: u64,
) -> Result<SubmitStats, Box<dyn std::error::Error>> {
    let identity_to_vote_map = &keeper_state.identity_to_vote_map;
    let slot_history = &keeper_state.slot_history;
    let epoch_schedule = &keeper_state.epoch_schedule;
    let current_epoch_info = &keeper_state.epoch_info;
    let current_epoch = current_epoch_info.epoch;
    let current_finalized_slot = client
        .get_slot_with_commitment(CommitmentConfig::finalized())
        .await?;

    let epoch_range =
        (current_epoch - lookback_epochs)..(current_epoch + 1);

    // 1. Update Epoch Schedule
    for epoch in epoch_range.clone() {
        info!("\n\n\n1. Update Epoch Schedule\n\n\n");
        let start_time = std::time::Instant::now();
        let epoch_starting_slot = epoch_schedule.get_first_slot_in_epoch(epoch);
        let epoch_leader_schedule_result =
            get_leader_schedule_safe(client, epoch_starting_slot).await;

        if epoch_leader_schedule_result.is_err() {
            info!("Could not find leader schedule for epoch {}", epoch);
            continue;
        }

        let epoch_leader_schedule =
            epoch_leader_schedule_result.expect("Could not unwrap epoch schedule");
        match DBSlotInfo::insert_leader_schedule(
            sqlite_connection,
            epoch,
            epoch_schedule,
            &epoch_leader_schedule,
            None,
        ) {
            Ok(write_count) => {
                let time_ms = start_time.elapsed().as_millis();
                info!(
                    "Wrote {} leaders for epoch {} in {:.3}s",
                    write_count,
                    epoch,
                    time_ms as f64 / 1000.0
                )
            }
            Err(err) => error!("Error writing leaders {:?}", err),
        }

        // 1.b Log out missing vote accounts
        for leader in epoch_leader_schedule.keys() {
            if !identity_to_vote_map.contains_key(leader) {
                // TODO
                error!("TODO Could not find Vote for {} in epoch {}", leader, epoch)
            }
        }
    }

    // 2. Update Mapping
    // NOTE: The mapping is only good for the current epoch, however
    // we need some mapping for backfilling the epochs
    for epoch in epoch_range.clone() {
        info!("\n\n\n2. Map Identity to Vote\n\n\n");
        let start_time = std::time::Instant::now();
        match DBSlotInfo::upsert_vote_identity_mapping(
            sqlite_connection,
            epoch,
            identity_to_vote_map,
            None,
        ) {
            Ok(write_counter) => {
                let time_ms = start_time.elapsed().as_millis();
                info!(
                    "Wrote {} identity/vote mappings in {:.3}s",
                    write_counter,
                    time_ms as f64 / 1000.0
                )
            }
            Err(err) => error!("Error updating identity/vote mapping {:?}", err),
        }

        // 2b. Print out all
        // let all_unmapped_identities =
        //     DBSlotInfo::get_unmapped_identity_accounts(sqlite_connection, epoch)?;
        // error!(
        //     "Unmapped identities ({}) \n{:?}\n",
        //     all_unmapped_identities.len(),
        //     all_unmapped_identities
        // );
    }

    // 3. Update Blocks ( Tries to update all blocks )
    {
        info!("\n\n\n3. Update Blocks\n\n\n");
        let start_total_time = std::time::Instant::now();
        let slots_needing_blocks =
            DBSlotInfo::get_slots_needing_blocks(sqlite_connection, current_finalized_slot)?;
        let chunk_size = 1000;

        let mut total_blocks = 0;
        for slots_needing_blocks in slots_needing_blocks.chunks(chunk_size) {
            // Add timing measurement before the operation
            let start_time = std::time::Instant::now();

            let block_data = get_bulk_block_safe(
                client,
                slots_needing_blocks,
                slot_history,
                maybe_redundant_rpc_urls,
                Some(UiTransactionEncoding::Json),
                Some(TransactionDetails::None),
                None,
            )
            .await;
            let mut ok_entries = vec![];
            for (slot, maybe_block_data) in block_data {
                match maybe_block_data {
                    Ok(block) => {
                        let priority_fees = block
                            .rewards
                            .unwrap()
                            .into_iter()
                            .filter(|r| r.reward_type == Some(RewardType::Fee))
                            .map(|r| r.lamports as u64)
                            .sum::<u64>();
                        ok_entries.push((slot, priority_fees));
                    }
                    Err(err) => match err {
                        BlockMetadataKeeperError::SkippedBlock => {
                            DBSlotInfo::set_block_dne(sqlite_connection, slot)?;
                        }
                        _ => {
                            info!(
                                "Could not get block info for slot {} - skipping: {:?}",
                                slot, err
                            )
                        }
                    },
                }
            }

            match DBSlotInfo::upsert_block_data(sqlite_connection, &ok_entries, None) {
                Ok(write_counter) => {
                    let time_ms = start_time.elapsed().as_millis();
                    let blocks_per_second = (write_counter as f64 * 1000.0) / time_ms.max(1) as f64;
                    total_blocks += write_counter;

                    info!(
                        "Wrote {} blocks in {:.3}s ({:.1} blocks/s)",
                        write_counter,
                        time_ms as f64 / 1000.0,
                        blocks_per_second
                    )
                }
                Err(err) => error!("Error writing blocks {:?}", err),
            }
        }

        let time_ms = start_total_time.elapsed().as_millis();
        let blocks_per_second = (total_blocks as f64 * 1000.0) / time_ms.max(1) as f64;
        info!(
            "Wrote Total {} blocks in {:.3}s ({:.1} blocks/s)",
            total_blocks,
            time_ms as f64 / 1000.0,
            blocks_per_second
        );
    }

    // 4. Aggregate Update TXs
    let mut ixs = vec![];
    {
        info!("\n\n\n4. Aggregate Update TXs\n\n\n");

        let mut needs_update_counter = 0;

        let start_time = std::time::Instant::now();
        for epoch in epoch_range {
            let first_slot_in_next_epoch = epoch_schedule.get_first_slot_in_epoch(epoch + 1);
            let update_map = match DBSlotInfo::get_priority_fee_and_block_metadata_entries(
                sqlite_connection,
                epoch,
                program_id,
                &priority_fee_oracle_authority_keypair.pubkey(),
            ) {
                Ok(map) => map,
                Err(err) => {
                    error!("Could not get update map - skipping... {:?}", err);
                    continue;
                }
            };

            for entry in update_map.clone() {
                let (vote_account, entry) = entry;

                // info! out everything that is on chain
                let (
                    mut needs_update,
                    mut validator_history_entry_total_priority_fees,
                    mut validator_history_entry_total_leader_slots,
                    mut validator_history_priority_fee_merkle_root_upload_authority,
                    mut validator_history_entry_priority_fee_commission,
                    mut validator_history_entry_block_data_updated_at_slot,
                    mut validator_history_priority_fee_tips,
                    mut validator_history_entry_blocks_produced,
                ): (bool, i64, i64, i64, i64, i64, i64, i64) = (false, -1, -1, -1, -1, -1, -1, -1);
                if let Some(validator_history) =
                    keeper_state.validator_history_map.get(&entry.vote_account)
                {
                    if let Some(validator_history_entry) = validator_history
                        .history
                        .arr
                        .iter()
                        .find(|history| history.epoch as u64 == epoch)
                    {
                        // Process validator history
                        validator_history_entry_total_priority_fees =
                            validator_history_entry.total_priority_fees as i64;
                        validator_history_entry_total_leader_slots =
                            validator_history_entry.total_leader_slots as i64;
                        validator_history_priority_fee_merkle_root_upload_authority =
                            validator_history_entry.priority_fee_merkle_root_upload_authority
                                as i64;
                        validator_history_entry_priority_fee_commission =
                            validator_history_entry.priority_fee_commission as i64;
                        validator_history_entry_block_data_updated_at_slot =
                            validator_history_entry.block_data_updated_at_slot as i64;
                        validator_history_priority_fee_tips =
                            validator_history_entry.priority_fee_tips as i64;
                        validator_history_entry_blocks_produced =
                            validator_history_entry.blocks_produced as i64;

                        needs_update = validator_history_entry.block_data_updated_at_slot < first_slot_in_next_epoch || validator_history_entry.block_data_updated_at_slot == u64::MAX;
                    }
                }

                // Calculate total lamports transferred
                let (
                    priority_fee_distribution_account,
                    total_lamports_transferred,
                    validator_commission_bps,
                    error_string,
                ) = get_priority_fee_distribution_account_info(
                    client,
                    priority_fee_distribution_program_id,
                    &entry.vote_account,
                    epoch,
                )
                .await;

                //TODO uncomment
                // datapoint_info!(
                //   "pfh-block-info-0.0.9",
                //   ("blocks-error", entry.blocks_error, i64),
                //   ("blocks-left", entry.blocks_left, i64),
                //   ("blocks-missed", entry.blocks_missed, i64),
                //   ("blocks-produced", entry.blocks_produced, i64),
                //   ("epoch", entry.epoch, i64),
                //   ("highest-slot", entry.highest_done_slot, i64),
                //   ("total-leader-slots", entry.total_leader_slots, i64),
                //   ("total-priority-fees", entry.total_priority_fees, i64),
                //   ("pfs-total-lamports-transferred", total_lamports_transferred, i64),
                //   ("pfs-validator-commission-bps", validator_commission_bps, i64 ),
                //   ("pfs-priority-fee-distribution-account", priority_fee_distribution_account.to_string(), String),
                //   ("pfs-priority-fee-distribution-account-error", error_string, Option<String>),
                //   ("vhe-total-priority-fees", validator_history_entry_total_priority_fees, i64),
                //   ("vhe-total-leader-slots", validator_history_entry_total_leader_slots, i64),
                //   ("vhe-priority-fee-merkle-root-upload-authority", validator_history_priority_fee_merkle_root_upload_authority, i64),
                //   ("vhe-priority-fee-commission", validator_history_entry_priority_fee_commission, i64),
                //   ("vhe-block-data-updated-at-slot", validator_history_entry_block_data_updated_at_slot, i64),
                //   ("vhe-priority-fee-tips", validator_history_priority_fee_tips, i64),
                //   ("vhe-blocks-produced", validator_history_entry_blocks_produced, i64),
                //   ("vhe-needs-update", needs_update, bool),
                //   ("update-slot", entry.highest_global_done_slot, i64),
                //   "cluster" => cluster.to_string(),
                //   "vote" => vote_account.to_string(),
                //   "priority-fee-distribution-program" => priority_fee_distribution_program_id.to_string(),
                //   "priority-fee-oracle-authority" => priority_fee_oracle_authority_keypair.pubkey().to_string(),
                //   "validator-history-program" => program_id.to_string(),
                //   "epoch" => format!("{}", epoch),
                // );

                if needs_update {
                    // info!("Block Metadata: {} ({})", vote_account, epoch);
                    needs_update_counter += 1;
                    //TODO uncomment
                    // ixs.push(entry.update_instruction());
                }
            }
        }

        let time_ms = start_time.elapsed().as_millis();
        info!(
            "Aggregated {} in {:.3}s",
            ixs.len(),
            time_ms as f64 / 1000.0,
        );
        info!("Block Metadata: {}", needs_update_counter);

    }



    // 5. Submit TXs
    {
        info!("\n\n\n. Submitting txs ({})\n\n\n", ixs.len());

        let start_time = std::time::Instant::now();
        let submit_result = submit_chunk_instructions(
            client,
            ixs,
            priority_fee_oracle_authority_keypair,
            priority_fee_in_microlamports,
            retry_count,
            confirmation_time,
            None,
            5,
        )
        .await?;

        let time_ms = start_time.elapsed().as_millis();
        info!(
            "Sent {}🟩 {}🟥 in {:.3}s",
            submit_result.successes,
            submit_result.errors,
            time_ms as f64 / 1000.0,
        );

        Ok(submit_result)
    }
}

pub async fn get_priority_fee_distribution_account_info(
    client: &RpcClient,
    priority_fee_distribution_program_id: &Pubkey,
    vote_account: &Pubkey,
    epoch: u64,
) -> (Pubkey, i64, i64, Option<String>) {
    // total lamports transferred, validator commission bps

    let (priority_fee_distribution_account, _) = derive_priority_fee_distribution_account_address(
        priority_fee_distribution_program_id,
        vote_account,
        epoch,
    );
    match client.get_account(&priority_fee_distribution_account).await {
        Ok(account) => {
            let mut data_slice = account.data.as_slice();
            match PriorityFeeDistributionAccount::deserialize(&mut data_slice) {
                Ok(account) => (
                    priority_fee_distribution_account,
                    account.total_lamports_transferred as i64,
                    account.validator_commission_bps as i64,
                    None,
                ),
                Err(error) => {
                    let error_string = format!(
                        "Could not deserialize account data {}-{}-{} = {}: {:?}",
                        priority_fee_distribution_program_id,
                        vote_account,
                        epoch,
                        priority_fee_distribution_account,
                        error
                    );
                    (
                        priority_fee_distribution_account,
                        -1,
                        -1,
                        Some(error_string),
                    )
                }
            }
        }
        Err(error) => {
            let error_string = format!(
                "Could not fetch account {}-{}-{} = {}: {:?}",
                priority_fee_distribution_program_id,
                vote_account,
                epoch,
                priority_fee_distribution_account,
                error
            );
            (
                priority_fee_distribution_account,
                -1,
                -1,
                Some(error_string),
            )
        }
    }
}

pub async fn get_leader_schedule_safe(
    rpc_client: &RpcClient,
    starting_slot: u64,
) -> Result<HashMap<String, Vec<usize>>, BlockMetadataKeeperError> {
    match rpc_client.get_leader_schedule(Some(starting_slot)).await? {
        Some(schedule) => Ok(schedule),
        None => Err(BlockMetadataKeeperError::OtherError(format!(
            "Could not get leader schedule for starting slot {}",
            starting_slot
        ))),
    }
}

async fn get_bulk_block_safe(
    client: &RpcClient,
    slots: &[u64],
    slot_history: &SlotHistory,
    maybe_redundant_rpc_urls: &Option<Arc<Vec<RpcClient>>>,
    encoding: Option<UiTransactionEncoding>,
    transaction_details: Option<TransactionDetails>,
    chunk_size: Option<usize>,
) -> Vec<(u64, Result<UiConfirmedBlock, BlockMetadataKeeperError>)> {
    let chunk_size = chunk_size.unwrap_or(50);

    let mut results = vec![];
    for slots in slots.chunks(chunk_size) {
        let futures = slots
            .iter()
            .map(|&slot| {
                let slot_history = slot_history.clone(); // Clone if needed
                let maybe_redundant_rpc_urls = maybe_redundant_rpc_urls.clone(); // Clone if needed

                async move {
                    let result = get_block_safe(
                        client,
                        slot,
                        &slot_history,
                        &maybe_redundant_rpc_urls,
                        encoding,
                        transaction_details,
                    )
                    .await;
                    (slot, result)
                }
            })
            .collect::<Vec<_>>();

        let future_results = join_all(futures).await;
        results.extend(future_results);
    }

    results
}

/// Wrapper on Solana RPC get_block, but propagates skipped blocks as BlockMetadataKeeperError
async fn get_block_safe(
    client: &RpcClient,
    slot: u64,
    slot_history: &SlotHistory,
    maybe_redundant_rpc_urls: &Option<Arc<Vec<RpcClient>>>,
    encoding: Option<UiTransactionEncoding>,
    transaction_details: Option<TransactionDetails>,
) -> Result<UiConfirmedBlock, BlockMetadataKeeperError> {
    let mut current_client = client;
    let mut redundant_rpc_index = 0;

    let slot_skipped_regex = Regex::new(r"^Slot [\d]+ was skipped").unwrap();

    loop {
        let block_res = current_client
            .get_block_with_config(
                slot,
                RpcBlockConfig {
                    encoding,
                    transaction_details,
                    rewards: Some(true),
                    commitment: Some(CommitmentConfig::finalized()),
                    max_supported_transaction_version: Some(0),
                },
            )
            .await;
        match block_res {
            Ok(block) => return Ok(block),
            Err(err) => match err.kind {
                ClientErrorKind::RpcError(client_rpc_err) => match client_rpc_err {
                    RpcError::RpcResponseError {
                        code,
                        message,
                        data,
                    } => {
                        // These slot skipped errors come from RpcCustomError::SlotSkipped or
                        //  RpcCustomError::LongTermStorageSlotSkipped and may not always mean
                        //  there is no block for a given slot. The additional context are:
                        //  "...or missing due to ledger jump to recent snapshot"
                        //  "...or missing in long-term storage"
                        // Meaning they can arise from RPC issues or lack of history (limit ledger
                        //  space, no big table) accesible  by an RPC. This is why we check
                        // SlotHistory and then follow up with redundant RPC checks.
                        if slot_skipped_regex.is_match(&message) {
                            match slot_history.check(slot) {
                                slot_history::Check::Future => {
                                    return Err(BlockMetadataKeeperError::SlotInFuture(slot))
                                }
                                slot_history::Check::NotFound => {
                                    return Err(BlockMetadataKeeperError::SkippedBlock)
                                }
                                slot_history::Check::TooOld | slot_history::Check::Found => {
                                    // REVIEW: Should we handle TooOld and Found differently?
                                    if let Some(redundant_rpc_urls) = maybe_redundant_rpc_urls {
                                        if redundant_rpc_index >= redundant_rpc_urls.len() {
                                            return Err(BlockMetadataKeeperError::SkippedBlock);
                                        }
                                        current_client = &redundant_rpc_urls[redundant_rpc_index];
                                        redundant_rpc_index += 1;
                                        continue;
                                    } else {
                                        return Err(BlockMetadataKeeperError::SkippedBlock);
                                    }
                                }
                            }
                        }
                        return Err(BlockMetadataKeeperError::RpcError(
                            RpcError::RpcResponseError {
                                code,
                                message,
                                data,
                            },
                        ));
                    }
                    _ => return Err(BlockMetadataKeeperError::RpcError(client_rpc_err)),
                },
                _ => return Err(BlockMetadataKeeperError::SolanaClientError(err)),
            },
        };
    }
}
