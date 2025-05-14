use std::{collections::HashMap, str::FromStr};

use anchor_lang::prelude::EpochSchedule;
use log::{error, info};
use rusqlite::{params, Connection};
use solana_client::rpc_response::RpcLeaderSchedule;
use solana_sdk::pubkey::Pubkey;

use crate::entries::priority_fee_and_block_metadata_entry::PriorityFeeAndBlockMetadataEntry;

use super::{errors::BlockMetadataKeeperError, operations::AggregateBlockInfo};

// -------------------------- NEW SCHEMA -----------------------------
#[repr(u8)]
#[derive(Debug)]
pub enum DBSlotInfoState {
    Created = 0x10,
    Done = 0x11,
    BlockDNE = 0x12,
    Error = 0x13,
}

impl DBSlotInfoState {
    pub fn from_u8(state: u8) -> Result<Self, BlockMetadataKeeperError> {
        if state == DBSlotInfoState::Created as u8 {
            return Ok(DBSlotInfoState::Created);
        }
        if state == DBSlotInfoState::Done as u8 {
            return Ok(DBSlotInfoState::Done);
        }
        if state == DBSlotInfoState::BlockDNE as u8 {
            return Ok(DBSlotInfoState::BlockDNE);
        }
        if state == DBSlotInfoState::Error as u8 {
            return Ok(DBSlotInfoState::Error);
        }

        Err(BlockMetadataKeeperError::OtherError(format!(
            "Could not map state {}",
            state
        )))
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct DBSlotInfo {
    pub identity_key: String,
    pub vote_key: Option<String>,
    pub epoch: u64,
    pub absolute_slot: u64,
    pub relative_slot: u64,
    pub priority_fees: u64,
    pub state: DBSlotInfoState,
    pub error_string: Option<String>,
}

impl DBSlotInfo {
    // -------------------- HELPERS -----------------------------
    fn from_db_row(row: &rusqlite::Row<'_>) -> Result<Self, BlockMetadataKeeperError> {
        let state_raw = row.get(6)?;
        let state = DBSlotInfoState::from_u8(state_raw)?;

        Ok(Self {
            absolute_slot: row.get(0)?,
            relative_slot: row.get(1)?,
            epoch: row.get(2)?,
            vote_key: row.get(3)?,
            identity_key: row.get(4)?,
            priority_fees: row.get(5)?,
            state,
            error_string: row.get(7)?,
        })
    }

    // -------------------- STAGES -----------------------------

    // 1. Updates the leader schedule such that we know we have every entry for a given epoch
    pub fn upsert_leader_schedule(
        connection: &mut Connection,
        epoch: u64,
        epoch_schedule: &EpochSchedule,
        leader_schedule: &RpcLeaderSchedule,
        chunk_size: Option<usize>,
    ) -> Result<u64, BlockMetadataKeeperError> {
        let chunk_size = chunk_size.unwrap_or(100);
        let first_slot_in_epoch = epoch_schedule.get_first_slot_in_epoch(epoch);

        let schedule_length: usize = leader_schedule.iter().map(|entry| entry.1.len()).sum();
        let slots_written = Self::get_slots_per_epoch(connection, epoch)?;

        if schedule_length == slots_written.len() {
            return Ok(0);
        }

        // Prepare the SQL statement once
        let sql = "INSERT OR IGNORE INTO slot_info (
            absolute_slot,
            relative_slot,
            epoch,
            vote_key,
            identity_key,
            priority_fees,
            state,
            error_string
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)";

        let mut write_counter = 0;
        let mut transaction = connection.transaction()?;
        for leader in leader_schedule.iter() {
            let identity_key = leader.0;
            let relative_slots = leader.1;

            // Process each slot individually
            for relative_slots in relative_slots.chunks(chunk_size) {
                for relative_slot in relative_slots {
                    let absolute_slot = first_slot_in_epoch + *relative_slot as u64;

                    if slots_written.contains(&absolute_slot) {
                        continue;
                    }

                    write_counter += 1;
                    transaction.execute(
                        sql,
                        params![
                            absolute_slot,
                            relative_slot,
                            epoch,
                            "", // vote_key is empty at this point, will be updated later
                            identity_key,
                            0,                              // priority_fees default to 0
                            DBSlotInfoState::Created as u8, // Set initial state to Created
                            ""                              // error_string default to empty
                        ],
                    )?;
                }

                // info!("Wrote {} Leaders", write_counter);
                transaction.commit()?;
                transaction = connection.transaction()?;
            }
        }

        Ok(write_counter)
    }

    // 2. Update the Vote Identity Mapping only for the current epoch.
    pub fn upsert_vote_identity_mapping(
        connection: &mut Connection,
        epoch: u64,
        mapping: &HashMap<String, String>, // identity, vote
        chunk_size: Option<usize>,
    ) -> Result<u64, BlockMetadataKeeperError> {
        let chunk_size = chunk_size.unwrap_or(100);
        let unmapped = match Self::get_unmapped_identity_accounts(connection, epoch) {
            Ok(list) => list,
            Err(_) => mapping.keys().cloned().collect(),
        };

        let sql = "UPDATE slot_info
         SET vote_key = ?
         WHERE epoch = ? AND identity_key = ? AND vote_key = ''";

        let mut write_counter = 0;
        let mut transaction = connection.transaction()?;
        let entries: Vec<_> = mapping.iter().collect();

        // Only write to the entried that are not already mapped
        let entries_to_write: Vec<_> = entries
            .iter()
            .filter(|entry| unmapped.contains(entry.0))
            .collect();

        for entries in entries_to_write.chunks(chunk_size) {
            for entry in entries {
                let identity_key = entry.0.to_string();
                let vote_key = entry.1.to_string();

                write_counter += 1;
                transaction.execute(&sql, params![vote_key, epoch, identity_key])?;
            }
            transaction.commit()?;
            transaction = connection.transaction()?;
            info!("Wrote {} Mappings", write_counter);
        }

        Ok(write_counter)
    }

    pub fn upsert_block_data(
        connection: &mut Connection,
        entries: &Vec<(u64, u64)>, // slot, priority_fees
        chunk_size: Option<usize>,
    ) -> Result<u64, BlockMetadataKeeperError> {
        let chunk_size = chunk_size.unwrap_or(50);

        let sql = "UPDATE slot_info
         SET priority_fees = ?, state = ?
         WHERE absolute_slot = ?";

        let mut write_counter = 0;
        let mut transaction = connection.transaction()?;
        for entries in entries.chunks(chunk_size) {
            for entry in entries {
                let (slot, priority_fees) = entry;

                write_counter += 1;
                transaction.execute(
                    &sql,
                    params![priority_fees, DBSlotInfoState::Done as u8, slot],
                )?;
            }

            transaction.commit()?;
            transaction = connection.transaction()?;
        }

        Ok(write_counter)
    }

    // Block DNE
    pub fn set_block_dne(
        connection: &Connection,
        slot: u64,
    ) -> Result<(), BlockMetadataKeeperError> {
        let sql = "UPDATE slot_info
         SET priority_fees = ?, state = ?
         WHERE absolute_slot = ? AND state = ?";

        connection.execute(
            &sql,
            params![
                0,
                DBSlotInfoState::BlockDNE as u8,
                slot,
                DBSlotInfoState::Created as u8
            ],
        )?;

        Ok(())
    }

    // Block Error
    pub fn set_block_error(
        connection: &Connection,
        slot: u64,
        error_string: &String,
    ) -> Result<(), BlockMetadataKeeperError> {
        let sql = "UPDATE slot_info
         SET priority_fees = ?, state = ?, error_string = ?
         WHERE absolute_slot = ? AND state = ?";

        connection.execute(
            &sql,
            params![
                0,
                DBSlotInfoState::Error as u8,
                error_string,
                slot,
                DBSlotInfoState::Created as u8
            ],
        )?;

        Ok(())
    }

    pub fn get_unmapped_identity_accounts(
        connection: &Connection,
        epoch: u64,
    ) -> Result<Vec<String>, BlockMetadataKeeperError> {
        // Prepare query to find all distinct identity_keys where vote_key is empty
        let mut statement = connection.prepare(
            "SELECT DISTINCT identity_key
             FROM slot_info
             WHERE vote_key = '' AND epoch = ?
             ORDER BY identity_key ASC",
        )?;

        // Execute query and map the results to a Vec<String>
        let unmapped_results = statement.query_map(params![epoch], |row| {
            let identity_key: String = row.get(0)?;
            Ok(identity_key)
        })?;

        // Collect results into a Vec<String>
        let mut unmapped_identities = Vec::new();
        for result in unmapped_results {
            unmapped_identities.push(result?);
        }

        Ok(unmapped_identities)
    }

    pub fn get_slots_needing_blocks(
        connection: &Connection,
        current_slot: u64,
    ) -> Result<Vec<u64>, BlockMetadataKeeperError> {
        // Prepare query to find slots in Created state before current_slot
        // Ordered by absolute_slot ASC (oldest first) with a limit
        let mut statement = connection.prepare(
            "SELECT absolute_slot
             FROM slot_info
             WHERE state = ? AND absolute_slot < ?
             ORDER BY absolute_slot ASC",
        )?;

        // Execute query with parameters
        let slot_results = statement.query_map(
            params![DBSlotInfoState::Created as u8, current_slot],
            |row| Ok(row.get::<_, u64>(0)?),
        )?;

        // Collect results into a Vec<u64>
        let mut slots_needing_update = Vec::new();
        for slot_result in slot_results {
            let slot = slot_result?;
            slots_needing_update.push(slot);
        }

        info!(
            "Found {} slots that need updating",
            slots_needing_update.len()
        );
        Ok(slots_needing_update)
    }

    pub fn get_vote_keys_for_epoch(
        connection: &Connection,
        epoch: u64,
    ) -> Result<Vec<String>, BlockMetadataKeeperError> {
        // Prepare query to find all non-empty vote keys for the given epoch
        let mut statement = connection.prepare(
            "SELECT DISTINCT vote_key
             FROM slot_info
             WHERE epoch = ? AND vote_key != ''
             ORDER BY vote_key ASC",
        )?;

        // Execute query with the epoch parameter
        let vote_key_results =
            statement.query_map(params![epoch], |row| Ok(row.get::<_, String>(0)?))?;

        // Collect results into a Vec<String>
        let mut vote_keys = Vec::new();
        for vote_key_result in vote_key_results {
            let vote_key = vote_key_result?;
            vote_keys.push(vote_key);
        }

        Ok(vote_keys)
    }

    pub fn get_slots_per_epoch(
        connection: &Connection,
        epoch: u64,
    ) -> Result<Vec<u64>, BlockMetadataKeeperError> {
        // Prepare query to find all non-empty vote keys for the given epoch
        let mut statement = connection.prepare(
            "SELECT absolute_slot
             FROM slot_info
             WHERE epoch = ?
             ORDER BY absolute_slot ASC",
        )?;

        // Execute query with the epoch parameter
        let absolute_slot_results =
            statement.query_map(params![epoch], |row| Ok(row.get::<_, u64>(0)?))?;

        // Collect results into a Vec<String>
        let mut absolute_slots = Vec::new();
        for absolute_slot_result in absolute_slot_results {
            let absolute_slot = absolute_slot_result?;
            absolute_slots.push(absolute_slot);
        }

        Ok(absolute_slots)
    }

    // To Entry
    pub fn get_priority_fee_and_block_metadata_entries(
        connection: &Connection,
        epoch_schedule: &EpochSchedule,
        epoch: u64,
        program_id: &Pubkey,
        priority_fee_oracle_authority: &Pubkey,
    ) -> Result<HashMap<String, PriorityFeeAndBlockMetadataEntry>, BlockMetadataKeeperError> {
        // Fetch all entries for the given vote account and epoch
        let mut statement = connection.prepare(
            "SELECT
                absolute_slot,
                relative_slot,
                epoch,
                vote_key,
                identity_key,
                priority_fees,
                state,
                error_string
            FROM slot_info
            WHERE epoch = ? AND state = ? AND vote_key != ''
            ORDER BY absolute_slot ASC",
        )?;
        let slot_infos = statement
            .query_map(params![epoch, DBSlotInfoState::Done as u8], |row| {
                Ok(Self::from_db_row(row))
            })?;

        let mut map = HashMap::<String, PriorityFeeAndBlockMetadataEntry>::new();
        for slot_info in slot_infos {
            let slot_info = slot_info??;

            let vote_key_string = match slot_info.vote_key {
                Some(vote_key) => vote_key,
                None => {
                    error!("No vote key - skipping");
                    continue;
                }
            };
            let vote_key = match Pubkey::from_str(&vote_key_string) {
                Ok(vote_key) => vote_key,
                Err(_) => {
                    error!("Could not parse vote key - skipping");
                    continue;
                }
            };

            let entry = map.entry(vote_key.to_string()).or_insert_with(|| {
                PriorityFeeAndBlockMetadataEntry::new(
                    &vote_key,
                    epoch,
                    program_id,
                    priority_fee_oracle_authority,
                )
            });

            entry.total_leader_slots += 1;
            match slot_info.state {
                DBSlotInfoState::Created => {
                    entry.blocks_left += 1;
                }
                DBSlotInfoState::Done => {
                    entry.blocks_produced += 1;
                    entry.total_priority_fees += slot_info.priority_fees;
                }
                DBSlotInfoState::BlockDNE => {
                    entry.blocks_missed += 1;
                }
                DBSlotInfoState::Error => {
                    entry.blocks_error += 1;
                    info!("Block Error {:?}", slot_info.error_string);
                }
            }

            entry.highest_slot = slot_info.absolute_slot.max(entry.highest_slot);
            entry.update_slot = if entry.blocks_left > 0 {
                entry.highest_slot
            } else {
                epoch_schedule.get_first_slot_in_epoch(entry.epoch + 1)
            };
        }

        Ok(map)
    }
}

// --------------- OLD SCHEMA -------------------

#[allow(dead_code)]
#[derive(Debug)]
pub struct BlockKeeperMetadata {
    id: u8,
    pub slot: u64,
    pub epoch: u64,
}
impl BlockKeeperMetadata {
    pub fn new(id: u8, slot: u64, epoch: u64) -> Self {
        Self { id, slot, epoch }
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct LeaderBlockMetadata {
    pub vote_key: String,
    pub block_data_last_update_slot: u64,
    pub total_priority_fees: i64,
    pub leader_slots: u32,
    pub blocks_produced: u32,
}

impl LeaderBlockMetadata {
    pub fn new_from_aggregate_data(
        vote_key: String,
        block_data_last_update_slot: u64,
        agg_data: AggregateBlockInfo,
    ) -> Self {
        Self {
            vote_key,
            total_priority_fees: agg_data.priority_fees,
            leader_slots: agg_data.leader_slots,
            blocks_produced: agg_data.blocks_produced,
            block_data_last_update_slot,
        }
    }

    pub fn new_and_increment_data(
        &self,
        total_priority_fees: i64,
        leader_slots: u32,
        blocks_produced: u32,
        block_data_last_update_slot: u64,
    ) -> Self {
        Self {
            vote_key: self.vote_key.clone(),
            total_priority_fees: self.total_priority_fees + total_priority_fees,
            leader_slots: self.leader_slots + leader_slots,
            blocks_produced: self.blocks_produced + blocks_produced,
            block_data_last_update_slot,
        }
    }
}

pub fn batch_insert_leader_block_data(
    conn: &Connection,
    records: &Vec<LeaderBlockMetadata>,
) -> Result<(), BlockMetadataKeeperError> {
    let data: String = records
        .iter()
        .map(|record| {
            format!(
                "('{}', {}, {}, {}, {})",
                record.vote_key,
                record.total_priority_fees.to_string(),
                record.leader_slots.to_string(),
                record.blocks_produced.to_string(),
                record.block_data_last_update_slot.to_string()
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let query = format!("INSERT INTO leader_block_metadata (vote_key, total_priority_fees, leader_slots, blocks_produced, block_data_last_update_slot) VALUES {}", data);

    conn.execute(&query, ())?;
    Ok(())
}

pub fn fetch_block_keeper_metadata(
    conn: &Connection,
) -> Result<Option<BlockKeeperMetadata>, BlockMetadataKeeperError> {
    let mut res =
        conn.prepare("SELECT id, slot, epoch FROM block_keeper_metadata WHERE id = 1 LIMIT 1")?;
    let mut res = res.query([]).unwrap();
    Ok(res.next().unwrap().map(|row| BlockKeeperMetadata {
        id: row.get(0).unwrap(),
        slot: row.get(1).unwrap(),
        epoch: row.get(2).unwrap(),
    }))
}

pub fn upsert_block_keeper_metadata(
    conn: &Connection,
    epoch: u64,
    slot: u64,
) -> Result<(), BlockMetadataKeeperError> {
    conn.execute(
        "INSERT INTO block_keeper_metadata (id, epoch, slot)
      VALUES (1, ?1, ?2)
      ON CONFLICT (id) DO UPDATE SET
          epoch = excluded.epoch,
          slot = excluded.slot",
        [epoch, slot],
    )?;
    Ok(())
}

/// Create all necessary tables and indexes. Uses IF NOT EXISTS to be safe
pub fn create_sqlite_tables(conn: &Connection) -> Result<(), BlockMetadataKeeperError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS slot_info (
          absolute_slot  INTEGER PRIMARY KEY,
          relative_slot  INTEGER,
          epoch INTEGER,
          vote_key TEXT,
          identity_key TEXT,
          priority_fees INTEGER,
          state INTEGER,
          error_string TEXT
      )",
        (),
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS slot_info (
          id    INTEGER PRIMARY KEY,
          slot  INTEGER,
          epoch INTEGER
      )",
        (),
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS block_keeper_metadata (
          id    INTEGER PRIMARY KEY,
          slot  INTEGER,
          epoch INTEGER
      )",
        (),
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS leader_block_metadata (
          vote_key  TEXT,
          total_priority_fees INTEGER,
          leader_slots INTEGER,
          blocks_produced INTEGER,
          block_data_last_update_slot INTEGER
      )",
        (),
    )?;

    // Create index on leader block metadata descending by block_data_last_update_slot
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_leader_block_metadata_last_slot
      ON leader_block_metadata (vote_key, block_data_last_update_slot DESC
      )",
        (),
    )?;
    Ok(())
}

// Deletes all records prior to a given slot (non-inclusive)
pub fn prune_prior_records(conn: &Connection, slot: u64) -> Result<(), BlockMetadataKeeperError> {
    conn.execute(
        "DELETE FROM leader_block_metadata WHERE block_data_last_update_slot < ?1",
        [slot],
    )?;
    Ok(())
}
