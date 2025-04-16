use std::{collections::HashMap, sync::Arc};

use anchor_lang::prelude::EpochSchedule;
use log::debug;
use regex::Regex;
use rusqlite::Connection;
use solana_client::{
    client_error::ClientErrorKind, nonblocking::rpc_client::RpcClient, rpc_config::RpcBlockConfig,
    rpc_request::RpcError,
};
use solana_metrics::datapoint_error;
use solana_sdk::{
    clock::{DEFAULT_SLOTS_PER_EPOCH, DEFAULT_TICKS_PER_SECOND, DEFAULT_TICKS_PER_SLOT},
    commitment_config::CommitmentConfig,
    epoch_info::EpochInfo,
};
use solana_transaction_status::{
    RewardType, TransactionDetails, UiConfirmedBlock, UiTransactionEncoding,
};
use stakenet_sdk::models::submit_stats::SubmitStats;

use crate::{
    operations::{
        block_metadata::db::{
            batch_insert_leader_block_data, fetch_block_keeper_metadata,
            upsert_block_keeper_metadata, BlockKeeperMetadata,
        },
        keeper_operations::{check_flag, KeeperOperations},
    },
    state::{keeper_config::KeeperConfig, keeper_state::KeeperState},
};

use super::{db::LeaderBlockMetadata, errors::BlockMetadataKeeperError};

fn _get_operation() -> KeeperOperations {
    KeeperOperations::GossipUpload
}

fn _should_run() -> bool {
    true
}

#[derive(Debug, Default)]
pub struct AggregateBlockInfo {
    pub epoch: u64,
    pub leader_slots: u16,
    pub blocks_produced: u16,
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
    pub fn increment_data(&mut self, leader_slots: u16, blocks_produced: u16, priority_fees: i64) {
        self.leader_slots += leader_slots;
        self.blocks_produced += blocks_produced;
        self.priority_fees += priority_fees;
    }
}

type LeadersMap = HashMap<String, AggregateBlockInfo>;

pub async fn fire(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64, u64) {
    let client = &keeper_config.client;
    let sqlite_connection = &keeper_config.sqlite_connection;
    let block_metadata_interval = keeper_config.block_metadata_interval;
    let epoch_info = &keeper_state.epoch_info;
    let identity_to_vote_map = &keeper_state.identity_to_vote_map;

    let operation = _get_operation();
    let should_run = _should_run() && check_flag(keeper_config.run_flags, operation);

    let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) =
        keeper_state.copy_runs_errors_and_txs_for_epoch(operation);

    if should_run {
        match _process(
            client,
            epoch_info,
            sqlite_connection,
            block_metadata_interval,
            identity_to_vote_map,
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

async fn _process(
    client: &Arc<RpcClient>,
    epoch_info: &EpochInfo,
    sqlite_connection: &Arc<Connection>,
    block_metadata_interval: u64,
    identity_to_vote_map: &HashMap<String, String>,
) -> Result<SubmitStats, Box<dyn std::error::Error>> {
    update_block_metadata(
        client,
        epoch_info,
        sqlite_connection,
        block_metadata_interval,
        identity_to_vote_map,
    )
    .await
}

async fn update_block_metadata(
    client: &Arc<RpcClient>,
    epoch_info: &EpochInfo,
    sqlite_connection: &Arc<Connection>,
    block_metadata_interval: u64,
    identity_to_vote_map: &HashMap<String, String>,
) -> Result<SubmitStats, Box<dyn std::error::Error>> {
    // TODO: Put EpochSchedule into KeeperState to avoid unncessary calls
    let epoch_schedule = client.get_epoch_schedule().await.unwrap();
    let current_finalized_slot = client
        .get_slot_with_commitment(CommitmentConfig::finalized())
        .await
        .unwrap();
    let epoch_starting_slot = epoch_schedule.get_first_slot_in_epoch(epoch_info.epoch);
    let next_epoch_starting_slot = epoch_schedule.get_first_slot_in_epoch(epoch_info.epoch + 1);

    // Gather the data for what slot & epoch the keeper last indexed
    let maybe_block_keeper_metadata = fetch_block_keeper_metadata(sqlite_connection);
    let block_keeper_metadata = match maybe_block_keeper_metadata {
        Some(block_keeper_metadata) => block_keeper_metadata,
        None => {
            // When block_keeper_metadata does not exist, we assume the keeper has not ran
            // before and pick an appropriate starting slot relative to the keeper's interval.
            let solana_ticks_per_keeper_interval =
                block_metadata_interval * DEFAULT_TICKS_PER_SECOND;
            let estimated_slots_per_keeper_interval =
                solana_ticks_per_keeper_interval / DEFAULT_TICKS_PER_SLOT;
            // Non-issue, but this could underflow if started during chain genesis.
            let potential_starting_slot = current_finalized_slot
                .checked_sub(estimated_slots_per_keeper_interval)
                .unwrap();
            let slot = std::cmp::max(potential_starting_slot, epoch_starting_slot);
            BlockKeeperMetadata::new(1, slot, epoch_info.epoch)
        }
    };
    debug!("block_keeper_metadata {:?}", block_keeper_metadata);
    let starting_slot = block_keeper_metadata.slot + 1;
    // Clamp the ending slot to make sure it's all from the same epoch
    let ending_slot = std::cmp::min(current_finalized_slot, next_epoch_starting_slot - 1);

    // Conditional is to avoid the case where
    if starting_slot < ending_slot {
        handle_slots_for_epoch(
            &client,
            &sqlite_connection,
            &epoch_schedule,
            identity_to_vote_map,
            block_keeper_metadata.epoch,
            starting_slot,
            ending_slot,
        )
        .await?;
    }
    // TODO: Handle case where epoch is more than 1 above

    // If current epoch != last epoch, then we should run a second time with the next
    //  epoch's information
    if epoch_info.epoch != block_keeper_metadata.epoch {
        handle_slots_for_epoch(
            &client,
            &sqlite_connection,
            &epoch_schedule,
            identity_to_vote_map,
            block_keeper_metadata.epoch,
            next_epoch_starting_slot,
            current_finalized_slot,
        )
        .await?;
    }

    // TODO: If block_keeper_metadata.epoch != current_epoch_info.epoch, we know the epoch has
    // transitioned and we should submit the data to validator history for each validator
    Ok(SubmitStats {
        successes: 0,
        errors: 0,
        results: vec![],
    })
}

pub async fn handle_slots_for_epoch(
    rpc_client: &RpcClient,
    conn: &Connection,
    epoch_schedule: &EpochSchedule,
    identity_to_vote_map: &HashMap<String, String>,
    epoch: u64,
    starting_slot: u64,
    ending_slot: u64,
) -> Result<(), BlockMetadataKeeperError> {
    debug!(
        "Gathering data for slots: {} - {}",
        starting_slot, ending_slot
    );
    let rpc_leader_schedule = rpc_client
        .get_leader_schedule(Some(starting_slot))
        .await
        .unwrap()
        .expect("leader_schedule");
    let mut relative_slot_leaders: Vec<Option<&String>> =
        vec![None; DEFAULT_SLOTS_PER_EPOCH as usize];
    for (leader, slots) in rpc_leader_schedule.iter() {
        for relative_slot in slots {
            // Convert leader (identity pubkey) to be the vote_key
            if let Some(vote_key) = identity_to_vote_map.get(leader) {
                relative_slot_leaders[*relative_slot] = Some(vote_key);
            } else {
                return Err(BlockMetadataKeeperError::MissingVoteKey(leader.to_owned()));
            }
        }
    }

    let epoch_starting_slot = epoch_schedule.get_first_slot_in_epoch(epoch);

    let aggregate_info = aggregate_information(
        &rpc_client,
        epoch,
        epoch_starting_slot,
        starting_slot,
        ending_slot,
        relative_slot_leaders,
    )
    .await?;

    // Update the SQL lite DB with the aggregate information
    let leader_block_metadatas = get_updated_leader_block_metadatas(
        &conn,
        &epoch_schedule,
        starting_slot - 1,
        ending_slot,
        aggregate_info,
    )?;
    batch_insert_leader_block_data(&conn, leader_block_metadatas)?;
    // Update the block_keeper_metadata record
    upsert_block_keeper_metadata(&conn, epoch, ending_slot);

    Ok(())
}

pub fn get_updated_leader_block_metadatas(
    conn: &Connection,
    epoch_schedule: &EpochSchedule,
    previous_update_slot: u64,
    block_data_last_update_slot: u64,
    leader_aggregate_block_info: LeadersMap,
) -> Result<Vec<LeaderBlockMetadata>, BlockMetadataKeeperError> {
    // Fetch the latest leader_block_metadata for a given leader
    let mut res = conn
  .prepare("SELECT vote_key, total_priority_fees, leader_slots, blocks_produced, block_data_last_update_slot FROM leader_block_metadata WHERE block_data_last_update_slot = ?1")
  .unwrap();
    let leader_block_metadatas = res
        .query_map([previous_update_slot], |row| {
            Ok(LeaderBlockMetadata {
                vote_key: row.get(0).unwrap(),
                total_priority_fees: row.get(1).unwrap(),
                leader_slots: row.get(2).unwrap(),
                blocks_produced: row.get(3).unwrap(),
                block_data_last_update_slot: row.get(4).unwrap(),
            })
        })
        .unwrap();

    let leader_block_metadatas_map: HashMap<String, LeaderBlockMetadata> = leader_block_metadatas
        .into_iter()
        .filter(|x| x.is_ok())
        .map(|x| (x.as_ref().unwrap().vote_key.clone(), x.unwrap()))
        .collect();

    let records: Vec<LeaderBlockMetadata> = leader_aggregate_block_info
        .into_iter()
        .map(|(leader, aggregate_block_info)| {
            // Check if the existing block metadata is apart of the same epoch
            let maybe_leader_block_metadata = leader_block_metadatas_map.get(&leader);
            match maybe_leader_block_metadata {
                Some(leader_block_metadata) => {
                    if aggregate_block_info.epoch
                        == epoch_schedule
                            .get_epoch(leader_block_metadata.block_data_last_update_slot)
                    {
                        // When same epoch increment the data before storing
                        leader_block_metadata.new_and_increment_data(
                            aggregate_block_info.priority_fees,
                            aggregate_block_info.leader_slots,
                            aggregate_block_info.blocks_produced,
                            block_data_last_update_slot,
                        )
                    } else {
                        // When different epoch, we start with fresh data
                        LeaderBlockMetadata::new_from_aggregate_data(
                            leader,
                            block_data_last_update_slot,
                            aggregate_block_info,
                        )
                    }
                }
                None => LeaderBlockMetadata::new_from_aggregate_data(
                    leader,
                    block_data_last_update_slot,
                    aggregate_block_info,
                ),
            }
        })
        .collect();

    Ok(records)
}

fn increment_leader_info(
    leaders_map: &mut LeadersMap,
    leader: &String,
    epoch: u64,
    leader_slots: u16,
    blocks_produced: u16,
    priority_fees: i64,
) {
    match leaders_map.get_mut(leader) {
        Some(leader_info) => {
            leader_info.increment_data(leader_slots, blocks_produced, priority_fees);
        }
        None => {
            let mut leader_info = AggregateBlockInfo::new(epoch);
            leader_info.increment_data(leader_slots, blocks_produced, priority_fees);
            leaders_map.insert(leader.clone(), leader_info);
        }
    }
}

/// Iterates through each slot from _starting_slot_ to _ending_slot_, inclusive of the ending slot.
/// Lots the block metadata and aggreates the information on a per leader basis.
pub async fn aggregate_information(
    client: &RpcClient,
    epoch: u64,
    epoch_starting_slot: u64,
    starting_slot: u64,
    ending_slot: u64,
    slot_leaders: Vec<Option<&String>>,
) -> Result<LeadersMap, BlockMetadataKeeperError> {
    let mut res: LeadersMap = HashMap::new();
    // We use an inclusive range as the program relies on it being included
    for slot in starting_slot..=ending_slot {
        let relative_slot = slot - epoch_starting_slot;
        let leader = slot_leaders[relative_slot as usize].unwrap();
        let maybe_block_data = get_block(client, slot).await;
        match maybe_block_data {
            Ok(block) => {
                // get the priority fee rewards for the block.
                let priority_fees = block
                    .rewards
                    .unwrap()
                    .into_iter()
                    .find(|r| r.reward_type == Some(RewardType::Fee))
                    .map(|r| r.lamports)
                    .unwrap_or(0);
                increment_leader_info(&mut res, leader, epoch, 1, 1, priority_fees);
            }
            Err(err) => match err {
                BlockMetadataKeeperError::SkippedBlock => {
                    // TODO: Add some redundancy to check with other RPCs and validate block was skipped.
                    increment_leader_info(&mut res, leader, epoch, 1, 0, 0);
                }
                _ => return Err(err),
            },
        }
    }
    Ok(res)
}

/// Wrapper on Solana RPC get_block, but propagates skipped blocks as BlockMetadataKeeperError
async fn get_block(
    client: &RpcClient,
    slot: u64,
) -> Result<UiConfirmedBlock, BlockMetadataKeeperError> {
    let block_res = client
        .get_block_with_config(
            slot,
            RpcBlockConfig {
                encoding: Some(UiTransactionEncoding::Json),
                transaction_details: Some(TransactionDetails::None),
                rewards: Some(true),
                commitment: Some(CommitmentConfig::finalized()),
                max_supported_transaction_version: Some(0),
            },
        )
        .await;
    let block = match block_res {
        Ok(block) => block,
        Err(err) => match err.kind {
            ClientErrorKind::RpcError(client_rpc_err) => match client_rpc_err {
                RpcError::RpcResponseError {
                    code,
                    message,
                    data,
                } => {
                    let slot_skipped_regex = Regex::new(r"^Slot [\d]+ was skipped").unwrap();
                    if slot_skipped_regex.is_match(&message) {
                        return Err(BlockMetadataKeeperError::SkippedBlock);
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
    Ok(block)
}
