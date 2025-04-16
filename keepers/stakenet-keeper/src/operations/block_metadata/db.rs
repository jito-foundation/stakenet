use rusqlite::Connection;

use super::{errors::BlockMetadataKeeperError, operations::AggregateBlockInfo};

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

    conn.execute(&query, ()).unwrap();
    Ok(())
}

pub fn fetch_block_keeper_metadata(conn: &Connection) -> Option<BlockKeeperMetadata> {
    let mut res = conn
        .prepare("SELECT id, slot, epoch FROM block_keeper_metadata WHERE id = 1 LIMIT 1")
        .unwrap();
    let mut res = res.query([]).unwrap();
    res.next().unwrap().map(|row| BlockKeeperMetadata {
        id: row.get(0).unwrap(),
        slot: row.get(1).unwrap(),
        epoch: row.get(2).unwrap(),
    })
}

pub fn upsert_block_keeper_metadata(conn: &Connection, epoch: u64, slot: u64) {
    conn.execute(
        "INSERT INTO block_keeper_metadata (id, epoch, slot)
      VALUES (1, ?1, ?2) 
      ON CONFLICT (id) DO UPDATE SET 
          epoch = excluded.epoch,
          slot = excluded.slot",
        [epoch, slot],
    )
    .unwrap();
}

/// Create all necessary tables and indexes. Uses IF NOT EXISTS to be safe
pub fn create_sqlite_tables(conn: &Connection) {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS block_keeper_metadata (
          id    INTEGER PRIMARY KEY,
          slot  INTEGER,
          epoch INTEGER
      )",
        (),
    )
    .unwrap();

    conn.execute(
        "CREATE TABLE IF NOT EXISTS leader_block_metadata (
          vote_key  TEXT,
          total_priority_fees INTEGER,
          leader_slots INTEGER,
          blocks_produced INTEGER,
          block_data_last_update_slot INTEGER
      )",
        (),
    )
    .unwrap();

    // Create index on leader block metadata descending by block_data_last_update_slot
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_leader_block_metadata_last_slot 
      ON leader_block_metadata (vote_key, block_data_last_update_slot DESC
      )",
        (),
    )
    .unwrap();
}
