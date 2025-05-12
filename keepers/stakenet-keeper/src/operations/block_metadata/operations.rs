use std::{collections::HashMap, str::FromStr, sync::Arc};

use anchor_lang::prelude::{EpochSchedule, SlotHistory};
use log::{debug, error, info};
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
    instruction::Instruction,
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    slot_history,
};
use solana_transaction_status::{
    RewardType, TransactionDetails, UiConfirmedBlock, UiTransactionEncoding,
};
use stakenet_sdk::{
    models::{entries::UpdateInstruction, submit_stats::SubmitStats},
    utils::transactions::submit_instructions,
};

use crate::{
    entries::priority_fee_and_block_metadata_entry::PriorityFeeAndBlockMetadataEntry,
    operations::{
        block_metadata::db::{
            batch_insert_leader_block_data, fetch_block_keeper_metadata, prune_prior_records,
            upsert_block_keeper_metadata, BlockKeeperMetadata, DBSlotInfo,
        },
        keeper_operations::{check_flag, KeeperOperations},
    },
    state::{keeper_config::KeeperConfig, keeper_state::KeeperState},
};

use super::{db::LeaderBlockMetadata, errors::BlockMetadataKeeperError};

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

type LeadersMap = HashMap<String, AggregateBlockInfo>;

pub async fn fire(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64, u64) {
    let client = &keeper_config.client;
    let sqlite_connection = &keeper_config.sqlite_connection;
    let block_metadata_interval = keeper_config.block_metadata_interval;
    let program_id = &keeper_config.validator_history_program_id;
    let priority_fee_oracle_authority_keypair = keeper_config
        .priority_fee_oracle_authority_keypair
        .as_ref()
        .unwrap();

    let operation = _get_operation();
    let should_run = _should_run() && check_flag(keeper_config.run_flags, operation);

    let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) =
        keeper_state.copy_runs_errors_and_txs_for_epoch(operation);

    if should_run {
        match _process(
            client,
            sqlite_connection,
            block_metadata_interval,
            &keeper_config.redundant_rpc_urls,
            keeper_state,
            program_id,
            priority_fee_oracle_authority_keypair,
            keeper_config.tx_retry_count,
            keeper_config.tx_confirmation_seconds,
            keeper_config.priority_fee_in_microlamports,
            keeper_config.no_pack,
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
    sqlite_connection: &Arc<Connection>,
    block_metadata_interval: u64,
    maybe_redundant_rpc_urls: &Option<Arc<Vec<RpcClient>>>,
    keeper_state: &KeeperState,
    program_id: &Pubkey,
    priority_fee_oracle_authority_keypair: &Arc<Keypair>,
    retry_count: u16,
    confirmation_time: u64,
    priority_fee_in_microlamports: u64,
    no_pack: bool,
) -> Result<SubmitStats, Box<dyn std::error::Error>> {
    update_block_metadata_2(
        client,
        sqlite_connection,
        block_metadata_interval,
        maybe_redundant_rpc_urls,
        keeper_state,
        program_id,
        priority_fee_oracle_authority_keypair,
        retry_count,
        confirmation_time,
        priority_fee_in_microlamports,
        no_pack,
    )
    .await
}

async fn update_block_metadata_2(
    client: &Arc<RpcClient>,
    sqlite_connection: &Arc<Connection>,
    block_metadata_interval: u64,
    maybe_redundant_rpc_urls: &Option<Arc<Vec<RpcClient>>>,
    keeper_state: &KeeperState,
    program_id: &Pubkey,
    priority_fee_oracle_authority_keypair: &Arc<Keypair>,
    retry_count: u16,
    confirmation_time: u64,
    priority_fee_in_microlamports: u64,
    no_pack: bool,
) -> Result<SubmitStats, Box<dyn std::error::Error>> {
    let identity_to_vote_map = &keeper_state.identity_to_vote_map;
    let slot_history = &keeper_state.slot_history;
    let epoch_schedule = &keeper_state.epoch_schedule;
    let current_epoch_info = &keeper_state.epoch_info;
    let current_epoch = current_epoch_info.epoch;
    let current_finalized_slot = client
        .get_slot_with_commitment(CommitmentConfig::finalized())
        .await?;

    let lookback_epochs = 3;
    let epoch_range = (current_epoch - lookback_epochs)..current_epoch;

    // 1. Update Epoch Schedule
    info!("1. Update Epoch Schedule");
    for epoch in epoch_range.clone() {
        // 1.a Update Epoch Schedule
        let epoch_starting_slot = epoch_schedule.get_first_slot_in_epoch(epoch);

        let epoch_leader_schedule =
            match get_leader_schedule_safe(&client, epoch_starting_slot).await {
                Ok(schedule) => schedule,
                Err(err) => {
                    info!(
                        "Could not get Epoch Leader Schedule for {} - skipping: {:?}",
                        epoch, err
                    );
                    continue;
                }
            };

        let result = DBSlotInfo::create_from_leader_schedule(
            sqlite_connection,
            epoch,
            epoch_schedule,
            &epoch_leader_schedule,
        );
        if result.is_err() {
            info!(
                "Could not write leader squedule to DB {} - skipping: {:?}",
                epoch,
                result.err()
            );
            continue;
        }

        // 1.b Find missing vote accounts
        if epoch == current_epoch {
            for leader in epoch_leader_schedule.keys() {
                if !identity_to_vote_map.contains_key(leader) {
                    // TODO
                    info!("TODO Could not find Vote for {}", leader)
                }
            }
        }
    }

    // 2. Update Mapping
    info!("2. Update Mapping");
    for entry in identity_to_vote_map {
        let identity = match Pubkey::from_str(entry.0.clone().as_str()) {
            Ok(identity) => identity,
            Err(err) => {
                info!("Could not parse identity {} - skipping: {:?}", entry.0, err);
                continue;
            }
        };
        let vote = match Pubkey::from_str(entry.1.clone().as_str()) {
            Ok(vote) => vote,
            Err(err) => {
                info!("Could not parse vote {} - skipping: {:?}", entry.0, err);
                continue;
            }
        };

        let result = DBSlotInfo::update_vote_identity_mapping(
            sqlite_connection,
            current_epoch,
            &identity,
            &vote,
        );
        if result.is_err() {
            info!(
                "Could not write leader squedule to DB {} - skipping: {:?}",
                current_epoch,
                result.err()
            );
            continue;
        }
    }

    // 2b. Print out all
    let all_unmapped_identities = DBSlotInfo::get_unmapped_identity_accounts(sqlite_connection)?;
    if !all_unmapped_identities.is_empty() {
        let mut output_string = String::new();
        output_string.push_str("\n\n ------- UNMAPPED IDENTITIES -------");
        for (identity, epochs) in all_unmapped_identities.iter() {
            output_string.push_str(&format!("\n {} {:?}", identity, epochs));
        }
        output_string.push_str("\n");
        info!("{}", output_string)
    }

    // 3. Update Blocks
    info!("3. Update Blocks");
    let slots_needing_blocks =
        DBSlotInfo::get_slots_needing_blocks(sqlite_connection, current_finalized_slot)?;
    for slot in slots_needing_blocks {
        let maybe_block_data = get_block_safe(
            client,
            slot,
            slot_history,
            maybe_redundant_rpc_urls,
            Some(UiTransactionEncoding::Json),
            Some(TransactionDetails::None),
        )
        .await;

        match maybe_block_data {
            Ok(block) => {
                let priority_fees = block
                    .rewards
                    .unwrap()
                    .into_iter()
                    .find(|r| r.reward_type == Some(RewardType::Fee))
                    .map(|r| r.lamports)
                    .unwrap_or(0) as u64;
                DBSlotInfo::update_block_data(sqlite_connection, slot, priority_fees)?
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

    // 4. Submit Update TXs
    info!("4. Submit Update TXs");
    let mut ixs = vec![];
    for epoch in epoch_range {
        let vote_keys = match DBSlotInfo::get_vote_keys_per_epoch(sqlite_connection, epoch) {
            Ok(vote_keys) => vote_keys,
            Err(err) => {
                info!(
                    "Could not get vote keys for epoch {} - skipping: {:?}",
                    epoch, err
                );
                continue;
            }
        };

        for vote_key in vote_keys {
            let vote = match Pubkey::from_str(vote_key.clone().as_str()) {
                Ok(vote) => vote,
                Err(err) => {
                    info!(
                        "Could not parse vote_key {} - skipping: {:?}",
                        vote_key, err
                    );
                    continue;
                }
            };
            let entry = match DBSlotInfo::get_priority_fee_and_block_metadata_entry(
                sqlite_connection,
                &vote,
                epoch,
                current_finalized_slot,
                program_id,
                &priority_fee_oracle_authority_keypair.pubkey(),
            ) {
                Ok(entry) => entry,
                Err(err) => {
                    info!(
                        "Could not fetch entry for {} - skipping: {:?}",
                        vote_key, err
                    );
                    continue;
                }
            };

            ixs.push(entry.update_instruction());
        }
    }

    let submit_result = submit_instructions(
        client,
        ixs,
        priority_fee_oracle_authority_keypair,
        priority_fee_in_microlamports,
        retry_count,
        confirmation_time,
        None,
        no_pack,
    )
    .await;

    Ok(submit_result?)
}

async fn update_block_metadata(
    client: &Arc<RpcClient>,
    sqlite_connection: &Arc<Connection>,
    block_metadata_interval: u64,
    maybe_redundant_rpc_urls: &Option<Arc<Vec<RpcClient>>>,
    keeper_state: &KeeperState,
    program_id: &Pubkey,
    priority_fee_oracle_authority_keypair: &Arc<Keypair>,
    retry_count: u16,
    confirmation_time: u64,
    priority_fee_in_microlamports: u64,
    no_pack: bool,
) -> Result<SubmitStats, Box<dyn std::error::Error>> {
    let epoch_info = &keeper_state.epoch_info;
    let identity_to_vote_map = &keeper_state.identity_to_vote_map;
    let epoch_schedule = &keeper_state.epoch_schedule;
    let current_finalized_slot = client
        .get_slot_with_commitment(CommitmentConfig::finalized())
        .await?;
    let epoch_starting_slot = epoch_schedule.get_first_slot_in_epoch(epoch_info.epoch);

    // Gather the data for what slot & epoch the keeper last indexed
    let maybe_block_keeper_metadata = fetch_block_keeper_metadata(sqlite_connection)?;
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

    let mut instructions: Vec<Instruction> = vec![];
    // Handle case where current epoch is above the last indexed in SQLlite
    let epochs_diff = epoch_info.epoch - block_keeper_metadata.epoch;
    let mut starting_slot = block_keeper_metadata.slot + 1;
    for relative_epoch in 0..=epochs_diff {
        // For each epoch we are behind we need to generate the update instructions
        let epoch = block_keeper_metadata.epoch + relative_epoch;
        let epoch_ending_slot = epoch_schedule.get_last_slot_in_epoch(epoch);

        // Clamp the ending slot to make sure it's all from the same epoch
        let ending_slot = std::cmp::min(current_finalized_slot, epoch_ending_slot);
        // REVIEW: Do we want to submit the data intra epoch? Or just when an epoch is finalized?
        instructions.extend(
            handle_slots_for_epoch(
                &client,
                &sqlite_connection,
                &epoch_schedule,
                identity_to_vote_map,
                maybe_redundant_rpc_urls,
                &keeper_state.slot_history,
                epoch,
                starting_slot,
                ending_slot,
                program_id,
                &priority_fee_oracle_authority_keypair.pubkey(),
            )
            .await?,
        );

        info!("instructions extended {}", instructions.len());

        starting_slot = ending_slot + 1;
    }

    info!("total ixs {}", instructions.len());

    let submit_result = submit_instructions(
        client,
        instructions,
        priority_fee_oracle_authority_keypair,
        priority_fee_in_microlamports,
        retry_count,
        confirmation_time,
        None,
        no_pack,
    )
    .await;

    // Delete records older than 2 epochs
    prune_prior_records(
        sqlite_connection,
        epoch_schedule.get_first_slot_in_epoch(epoch_info.epoch - 2),
    )?;

    Ok(submit_result?)
}

async fn search_block_for_voter(
    rpc_client: &RpcClient,
    identity: &String,
    slot: u64,
    slot_history: &SlotHistory,
    maybe_redundant_rpc_urls: &Option<Arc<Vec<RpcClient>>>,
) -> Result<Option<String>, BlockMetadataKeeperError> {
    let block = get_block_safe(
        rpc_client,
        slot,
        slot_history,
        maybe_redundant_rpc_urls,
        Some(UiTransactionEncoding::Binary),
        Some(TransactionDetails::Full),
    )
    .await?;

    let identity = Pubkey::from_str(identity).map_err(|err| {
        BlockMetadataKeeperError::OtherError(
            format!("Failed to parse Identity {} - {:?}", identity, err).to_string(),
        )
    })?;

    if let Some(txs) = block.transactions {
        for tx in txs {
            let decoded_tx = tx.transaction.decode().ok_or_else(|| {
                BlockMetadataKeeperError::OtherError(
                    "Failed to decode transaction: None returned".to_string(),
                )
            })?;

            let accounts = decoded_tx.message.static_account_keys();

            let ixs = decoded_tx.message.instructions();
            for ix in ixs {
                let program_id = accounts[ix.program_id_index as usize];

                if program_id.ne(&solana_sdk::vote::program::id()) {
                    continue;
                }

                // TOWER_SYNC discriminator is 14
                // The vote account is always the first account
                // The vote authority is the 2nd account ( most likely identity )
                // https://docs.rs/solana-vote-program/2.2.7/solana_vote_program/vote_instruction/enum.VoteInstruction.html#variant.TowerSync
                //
                // Vote Dq16VEt4GLCpArpdn5mdgx2bjau5tmN3nm3bPGbamGqu Identity(Authority) 73kcKwNwdBDryYdEE3jpReZiokXvx2eQEbqGWQ3YNpEn
                const TOWER_SYNC: u8 = 14;
                const VOTE_ACCOUNT_INDEX: u8 = 0;
                const VOTE_AUTHORITY_INDEX: u8 = 1;
                if ix.data[0] == TOWER_SYNC {
                    let vote_account_index = ix.accounts[VOTE_ACCOUNT_INDEX as usize];
                    let vote_authority_index = ix.accounts[VOTE_AUTHORITY_INDEX as usize];

                    let vote_account = accounts[vote_account_index as usize];
                    let identity_account = accounts[vote_authority_index as usize];

                    if identity.eq(&identity_account) {
                        info!("Found {}", identity_account);
                        return Ok(Some(vote_account.to_string()));
                    }
                }
            }
        }
    }

    Ok(None)
}

pub async fn search_for_voter(
    rpc_client: &RpcClient,
    identity: &String,
    slot: u64,
    slot_history: &SlotHistory,
    maybe_redundant_rpc_urls: &Option<Arc<Vec<RpcClient>>>,
) -> Option<String> {
    // let start_of_search = (slot / DEFAULT_SLOTS_PER_EPOCH) * DEFAULT_SLOTS_PER_EPOCH;
    // let end_of_search =
    //     ((slot + DEFAULT_SLOTS_PER_EPOCH) / DEFAULT_SLOTS_PER_EPOCH) * DEFAULT_SLOTS_PER_EPOCH;

    let slots_to_search = 10;
    let start_of_search = slot - slots_to_search / 2;
    let end_of_search = slot + slots_to_search / 2;

    // Forward Search
    for slot in start_of_search..=end_of_search {
        match search_block_for_voter(
            rpc_client,
            identity,
            slot,
            slot_history,
            maybe_redundant_rpc_urls,
        )
        .await
        {
            Ok(maybe_voter) => {
                if let Some(voter) = maybe_voter {
                    info!("Found {}:{} - {}", identity, slot, voter);
                    return Some(voter);
                } else {
                    info!("Could not find {}:{}", identity, slot);
                }
            }
            Err(err) => {
                error!(
                    "Could not find with error {}:{} - {:?}",
                    identity, slot, err
                );
            } // Break on error
        }
    }

    None
}

pub async fn populate_relative_slot_leaders(
    rpc_client: &RpcClient,
    identity_to_vote_map: &HashMap<String, String>,
    slot_history: &SlotHistory,
    maybe_redundant_rpc_urls: &Option<Arc<Vec<RpcClient>>>,
    starting_slot: u64,
) -> Result<Vec<LeaderInfo>, BlockMetadataKeeperError> {
    let mut relative_slot_leaders: Vec<LeaderInfo> =
        vec![LeaderInfo::default(); DEFAULT_SLOTS_PER_EPOCH as usize];

    let rpc_leader_schedule = match rpc_client.get_leader_schedule(Some(starting_slot)).await? {
        Some(schedule) => schedule,
        None => {
            return Err(BlockMetadataKeeperError::OtherError(
                "Could not get leader schedule".to_string(),
            ))
        }
    };

    let mut identity_to_vote_map = identity_to_vote_map.clone();
    let mut ignore_leader: Vec<String> = vec![];

    for (leader, slots) in rpc_leader_schedule.iter() {
        for relative_slot in slots {
            let absolute_slot = starting_slot + *relative_slot as u64;

            if ignore_leader.contains(&leader) {
                continue;
            }

            // Convert leader (identity pubkey) to be the vote_key
            if let Some(voter) = identity_to_vote_map.get(leader) {
                relative_slot_leaders[*relative_slot] = LeaderInfo::new(leader, Some(voter))
            } else {
                match search_for_voter(
                    rpc_client,
                    leader,
                    absolute_slot,
                    slot_history,
                    maybe_redundant_rpc_urls,
                )
                .await
                {
                    Some(voter) => {
                        identity_to_vote_map.insert(leader.clone(), voter.clone());
                        relative_slot_leaders[*relative_slot] =
                            LeaderInfo::new(leader, Some(&voter))
                    }
                    None => {
                        ignore_leader.push(leader.clone());
                        relative_slot_leaders[*relative_slot] = LeaderInfo::new(leader, None)
                    }
                }
            }
        }
    }

    Ok(relative_slot_leaders)
}

pub async fn handle_slots_for_epoch(
    rpc_client: &RpcClient,
    conn: &Connection,
    epoch_schedule: &EpochSchedule,
    identity_to_vote_map: &HashMap<String, String>,
    maybe_redundant_rpc_urls: &Option<Arc<Vec<RpcClient>>>,
    slot_history: &SlotHistory,
    epoch: u64,
    starting_slot: u64,
    ending_slot: u64,
    program_id: &Pubkey,
    priority_fee_oracle_authority: &Pubkey,
) -> Result<Vec<Instruction>, BlockMetadataKeeperError> {
    debug!(
        "Gathering data for slots: {} - {}",
        starting_slot, ending_slot
    );

    let relative_slot_leaders: Vec<LeaderInfo> = populate_relative_slot_leaders(
        rpc_client,
        identity_to_vote_map,
        slot_history,
        maybe_redundant_rpc_urls,
        starting_slot,
    )
    .await?;

    let epoch_starting_slot = epoch_schedule.get_first_slot_in_epoch(epoch);
    info!("relative slot leaders {}", relative_slot_leaders.len());

    let aggregate_info = aggregate_information(
        &rpc_client,
        epoch,
        epoch_starting_slot,
        starting_slot,
        ending_slot,
        relative_slot_leaders,
        slot_history,
        maybe_redundant_rpc_urls,
    )
    .await?;

    // Update the SQL lite DB with the aggregate information
    info!("aggregate_info {}", aggregate_info.len());
    let leader_block_metadatas = get_updated_leader_block_metadatas(
        &conn,
        &epoch_schedule,
        starting_slot - 1,
        ending_slot,
        aggregate_info,
    )?;

    info!("leader block data {}", leader_block_metadatas.len());

    batch_insert_leader_block_data(&conn, &leader_block_metadatas)?;
    // Update the block_keeper_metadata record
    upsert_block_keeper_metadata(&conn, epoch, ending_slot)?;

    let instructions = leader_block_metadatas
        .iter()
        .filter_map(|leader_block_metadata| {
            let vote_key = Pubkey::from_str(&leader_block_metadata.vote_key).ok()?;

            Some(
                PriorityFeeAndBlockMetadataEntry::new(
                    &vote_key,
                    epoch,
                    program_id,
                    priority_fee_oracle_authority,
                    leader_block_metadata.total_priority_fees.try_into().ok()?,
                    leader_block_metadata.leader_slots,
                    leader_block_metadata.blocks_produced,
                    leader_block_metadata.block_data_last_update_slot,
                )
                .update_instruction(),
            )
        })
        .collect::<Vec<_>>();

    info!("IX len {}", instructions.len());

    Ok(instructions)
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
    // REVIEW: Here we are only creating LeaderBlockMetadata for those that came from the
    //  newly aggregated information. This means that validator's that did not have a leader
    //  slot in the aggregated range will not show up in this vector.
    // IF we want to make sure all ValidatorHistoryEntries are updated with the same update
    // slot, then we likely need to pull those other records in.
    // IF transactions are submited only when an epoch is finalized, then we would also
    // have to pull those records in.
    //
    // It probably makes the most sense to pull them in here, but worth a quick discussion
    //  on TX submissions and leaving this here as a reminder.

    Ok(records)
}

fn increment_leader_info(
    leaders_map: &mut LeadersMap,
    leader: &String,
    epoch: u64,
    leader_slots: u32,
    blocks_produced: u32,
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
    slot_leaders: Vec<LeaderInfo>,
    slot_history: &SlotHistory,
    maybe_redundant_rpc_urls: &Option<Arc<Vec<RpcClient>>>,
) -> Result<LeadersMap, BlockMetadataKeeperError> {
    let mut res: LeadersMap = HashMap::new();
    // We use an inclusive range as the program relies on it being included
    for slot in starting_slot..=ending_slot {
        let relative_slot = slot - epoch_starting_slot;
        let leader_info = slot_leaders[relative_slot as usize].clone();
        let maybe_block_data = get_block_safe(
            client,
            slot,
            slot_history,
            maybe_redundant_rpc_urls,
            Some(UiTransactionEncoding::Json),
            Some(TransactionDetails::None),
        )
        .await;
        match maybe_block_data {
            Ok(block) => {
                if let Ok(voter) = leader_info.clone().get_voter_safe() {
                    // get the priority fee rewards for the block.
                    let priority_fees = block
                        .rewards
                        .unwrap()
                        .into_iter()
                        .find(|r| r.reward_type == Some(RewardType::Fee))
                        .map(|r| r.lamports)
                        .unwrap_or(0);
                    increment_leader_info(&mut res, &voter, epoch, 1, 1, priority_fees);
                } else {
                    error!(
                        "Skipping... Could not find voter for {}",
                        leader_info.identity
                    )
                }
            }
            Err(err) => match err {
                BlockMetadataKeeperError::SkippedBlock => {
                    if let Ok(voter) = leader_info.clone().get_voter_safe() {
                        increment_leader_info(&mut res, &voter, epoch, 1, 0, 0);
                    } else {
                        error!(
                            "Skipping with error... Could not find voter for {}. {}",
                            leader_info.identity, err
                        )
                    }
                }
                _ => return Err(err),
            },
        }
        info!("Done with slot {}", slot)
    }

    info!("Done with all slots");

    Ok(res)
}

pub async fn get_leader_schedule_safe(
    rpc_client: &RpcClient,
    starting_slot: u64,
) -> Result<HashMap<String, Vec<usize>>, BlockMetadataKeeperError> {
    match rpc_client.get_leader_schedule(Some(starting_slot)).await? {
        Some(schedule) => Ok(schedule),
        None => Err(BlockMetadataKeeperError::OtherError(
            "Could not get leader schedule".to_string(),
        )),
    }
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
                        let slot_skipped_regex = Regex::new(r"^Slot [\d]+ was skipped").unwrap();
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

#[derive(Clone, Default)]
pub struct LeaderInfo {
    pub identity: String,
    pub voter: Option<String>,
}

impl LeaderInfo {
    pub fn new(identity: &String, voter: Option<&String>) -> Self {
        LeaderInfo {
            identity: identity.clone(),
            voter: match voter {
                Some(voter) => Some(voter.clone()),
                None => None,
            },
        }
    }

    pub fn get_voter_safe(self) -> Result<String, BlockMetadataKeeperError> {
        let voter = self
            .voter
            .as_ref()
            .ok_or_else(|| BlockMetadataKeeperError::MissingVoteKey(self.identity.clone()))?;

        Ok(voter.clone())
    }
}
