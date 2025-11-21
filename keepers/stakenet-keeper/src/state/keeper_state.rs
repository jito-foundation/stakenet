use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anchor_lang::prelude::{EpochSchedule, SlotHistory};
use bytemuck::Zeroable;
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_response::RpcVoteAccountInfo};
use solana_metrics::datapoint_info;
use solana_sdk::{
    account::Account, epoch_info::EpochInfo, pubkey::Pubkey,
    vote::program::id as get_vote_program_id,
};
use stakenet_sdk::{
    models::{
        aggregate_accounts::{AllStewardAccounts, AllValidatorAccounts},
        errors::JitoTransactionError,
    },
    utils::accounts::get_validator_history_address,
};
use validator_history::{ClusterHistory, ValidatorHistory};

use crate::operations::keeper_operations::{KeeperCreates, KeeperOperations};

pub struct StewardProgressFlags {
    pub flags: u8,
}

pub enum StewardProgressFlag {
    ComputeScores = 0x01 << 0,
    ComputeDelegations = 0x01 << 1,
    EpochMaintenance = 0x01 << 2,
    PreLoopIdle = 0x01 << 3,
    ComputeInstantUnstakes = 0x01 << 4,
    Rebalance = 0x01 << 5,
    PostLoopIdle = 0x01 << 6,
}

impl StewardProgressFlags {
    // Set a flag
    pub fn set_flag(&mut self, flag: StewardProgressFlag) {
        self.flags |= flag as u8;
    }

    pub fn clean_flags(&mut self) {
        self.flags = 0;
    }

    // Unset a flag
    pub fn unset_flag(&mut self, flag: StewardProgressFlag) {
        self.flags &= !(flag as u8);
    }

    // Check if a flag is set
    pub fn has_flag(&self, flag: StewardProgressFlag) -> bool {
        self.flags & (flag as u8) != 0
    }
}

#[derive(Clone, Copy)]
pub struct KeeperFlags {
    pub flags: u8,
}

pub enum KeeperFlag {
    Startup = 0x01 << 0,
    RerunVote = 0x01 << 1,
}

impl KeeperFlags {
    // Set a flag
    pub fn set_flag(&mut self, flag: KeeperFlag) {
        self.flags |= flag as u8;
    }

    pub fn clean_flags(&mut self) {
        self.flags = 0;
    }

    // Unset a flag
    pub fn unset_flag(&mut self, flag: KeeperFlag) {
        self.flags &= !(flag as u8);
    }

    // Check if a flag is set
    pub fn check_flag(&self, flag: KeeperFlag) -> bool {
        self.flags & (flag as u8) != 0
    }
}

pub struct KeeperState {
    pub keeper_flags: KeeperFlags,
    pub epoch_info: EpochInfo,
    pub epoch_schedule: EpochSchedule,
    pub slot_history: SlotHistory,

    // Tally array of runs and errors indexed by their respective KeeperOperations
    pub runs_for_epoch: [u64; KeeperOperations::LEN],
    pub errors_for_epoch: [u64; KeeperOperations::LEN],
    pub txs_for_epoch: [u64; KeeperOperations::LEN],

    // Tally for creates
    pub created_accounts_for_epoch: [u64; KeeperCreates::LEN],

    // All vote account info fetched with get_vote_accounts - key'd by their pubkey
    pub vote_account_map: HashMap<Pubkey, RpcVoteAccountInfo>,
    // All validator history entries fetched by get_validator_history_accounts - key'd by their vote_account pubkey
    pub validator_history_map: HashMap<Pubkey, ValidatorHistory>,
    // Maps a Validator's identity address to their vote account address
    pub identity_to_vote_map: HashMap<String, String>,

    // All vote accounts mapped and fetched from validator_history_map - key'd by their vote_account pubkey
    pub all_history_vote_account_map: HashMap<Pubkey, Option<Account>>,
    // All vote accounts mapped and fetched from vote_account_map - key'd by their pubkey
    pub all_get_vote_account_map: HashMap<Pubkey, Option<Account>>,

    // All tip distribution accounts fetched from the last epoch ( current_epoch - 1 ) - key'd by their vote_account pubkey
    pub previous_epoch_tip_distribution_map: HashMap<Pubkey, Option<Account>>,
    // All tip distribution accounts fetched from the current epoch - key'd by their vote_account pubkey
    pub current_epoch_tip_distribution_map: HashMap<Pubkey, Option<Account>>,

    pub cluster_history: ClusterHistory,
    pub keeper_balance: u64,

    pub all_steward_accounts: Option<Box<AllStewardAccounts>>,
    pub all_steward_validator_accounts: Option<Box<AllValidatorAccounts>>,
    pub all_active_validator_accounts: Option<Box<AllValidatorAccounts>>,
    pub steward_progress_flags: StewardProgressFlags,
    pub cluster_name: String,
}
impl KeeperState {
    pub fn update_identity_to_vote_map(&mut self) {
        self.identity_to_vote_map = self
            .vote_account_map
            .values()
            .map(|vote_account_info| {
                (
                    vote_account_info.node_pubkey.clone(),
                    vote_account_info.vote_pubkey.clone(),
                )
            })
            .collect();
    }

    pub fn increment_update_run_for_epoch(&mut self, operation: KeeperOperations) {
        let index = operation as usize;
        self.runs_for_epoch[index] += 1;
    }

    pub fn increment_update_error_for_epoch(&mut self, operation: KeeperOperations) {
        let index = operation as usize;
        self.errors_for_epoch[index] += 1;
    }

    pub fn increment_update_txs_for_epoch(&mut self, operation: KeeperOperations, txs: u64) {
        let index = operation as usize;
        self.errors_for_epoch[index] += txs;
    }

    pub fn copy_runs_errors_and_txs_for_epoch(
        &self,
        operation: KeeperOperations,
    ) -> (u64, u64, u64) {
        let index = operation as usize;
        (
            self.runs_for_epoch[index],
            self.errors_for_epoch[index],
            self.txs_for_epoch[index],
        )
    }

    pub fn set_runs_errors_and_txs_for_epoch(
        &mut self,
        (operation, runs_for_epoch, errors_for_epoch, txs_for_epoch): (
            KeeperOperations,
            u64,
            u64,
            u64,
        ),
    ) {
        let index = operation as usize;
        self.runs_for_epoch[index] = runs_for_epoch;
        self.errors_for_epoch[index] = errors_for_epoch;
        self.txs_for_epoch[index] = txs_for_epoch;
    }

    pub fn set_runs_errors_txs_and_flags_for_epoch(
        &mut self,
        (operation, runs_for_epoch, errors_for_epoch, txs_for_epoch, flags): (
            KeeperOperations,
            u64,
            u64,
            u64,
            KeeperFlags,
        ),
    ) {
        let index = operation as usize;
        self.runs_for_epoch[index] = runs_for_epoch;
        self.errors_for_epoch[index] = errors_for_epoch;
        self.txs_for_epoch[index] = txs_for_epoch;

        self.keeper_flags = flags;
    }

    pub fn increment_creations_for_epoch(
        &mut self,
        (operation, created_accounts_for_epoch): (KeeperCreates, u64),
    ) {
        let index = operation as usize;
        self.created_accounts_for_epoch[index] += created_accounts_for_epoch;
    }

    pub fn get_history_pubkeys(&self, program_id: &Pubkey) -> HashSet<Pubkey> {
        self.all_history_vote_account_map
            .keys()
            .map(|vote_account| get_validator_history_address(vote_account, program_id))
            .collect()
    }

    pub fn get_closed_vote_accounts(&self) -> HashSet<&Pubkey> {
        self.all_history_vote_account_map
            .iter()
            .filter_map(|(vote_address, vote_account)| match vote_account {
                Some(account) => {
                    if account.owner != get_vote_program_id() {
                        Some(vote_address)
                    } else {
                        None
                    }
                }
                _ => {
                    // If the account is not found, it is considered closed
                    Some(vote_address)
                }
            })
            .collect()
    }

    pub fn get_all_open_vote_accounts(&self) -> HashSet<&Pubkey> {
        self.all_history_vote_account_map
            .iter()
            .filter_map(|(vote_address, vote_account)| match vote_account {
                Some(account) => {
                    if account.owner == get_vote_program_id() {
                        Some(vote_address)
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect()
    }

    pub fn get_live_vote_accounts(&self) -> HashSet<&Pubkey> {
        self.all_get_vote_account_map
            .iter()
            .filter(|(_, vote_account)| {
                if let Some(account) = vote_account {
                    account.owner == get_vote_program_id()
                } else {
                    false
                }
            })
            .map(|(pubkey, _)| pubkey)
            .collect()
    }

    pub fn emit(&self) {
        datapoint_info!(
            "keeper-state",
            // EPOCH INFO
            ("epoch", self.epoch_info.epoch as i64, i64),
            ("slot_index", self.epoch_info.slot_index as i64, i64),
            ("slots_in_epoch", self.epoch_info.slots_in_epoch as i64, i64),
            ("absolute_slot", self.epoch_info.absolute_slot as i64, i64),
            ("block_height", self.epoch_info.block_height as i64, i64),
            // KEEPER STATE
            ("keeper_balance", self.keeper_balance as i64, i64),
            (
                "vote_account_map_count",
                self.vote_account_map.len() as i64,
                i64
            ),
            (
                "validator_history_map_count",
                self.validator_history_map.len() as i64,
                i64
            ),
            (
                "all_history_vote_account_map_count",
                self.all_history_vote_account_map.len() as i64,
                i64
            ),
            (
                "all_get_vote_account_map_count",
                self.all_get_vote_account_map.len() as i64,
                i64
            ),
            (
                "previous_epoch_tip_distribution_map_count",
                self.previous_epoch_tip_distribution_map.len() as i64,
                i64
            ),
            (
                "current_epoch_tip_distribution_map_count",
                self.current_epoch_tip_distribution_map.len() as i64,
                i64
            ),
            ("cluster", &self.cluster_name, String),
        )
    }

    pub fn set_cluster_name(&mut self, cluster_name: &str) {
        self.cluster_name = cluster_name.to_owned();
    }

    /// Determines if directed stake targets should be copied this epoch.
    ///
    /// Returns `true` if:
    /// - Epoch is more than 50% complete
    /// - Directed stake metadata hasn't been updated this epoch
    pub async fn should_copy_directed_stake_targets(
        &self,
        client: Arc<RpcClient>,
        _program_id: &Pubkey,
    ) -> Result<bool, JitoTransactionError> {
        if let Some(ref _steward_state) = self.all_steward_accounts {
            let current_slot = client.get_slot().await?;
            let slots_in_epoch = self.epoch_schedule.slots_per_epoch;
            let slot_index = current_slot
                .checked_sub(
                    self.epoch_schedule
                        .get_first_slot_in_epoch(self.epoch_info.epoch),
                )
                .ok_or(JitoTransactionError::Custom(
                    "Failed to calculate".to_string(),
                ))?;
            let epoch_progress = slot_index as f64 / slots_in_epoch as f64;

            let should_run_copy_directed_targets = epoch_progress > 0.5;

            return Ok(should_run_copy_directed_targets);
        }

        Ok(false)
    }
}

impl Default for KeeperState {
    fn default() -> Self {
        Self {
            keeper_flags: KeeperFlags { flags: 0 },
            epoch_info: EpochInfo {
                epoch: 0,
                slot_index: 0,
                slots_in_epoch: 0,
                absolute_slot: 0,
                block_height: 0,
                transaction_count: None,
            },
            epoch_schedule: EpochSchedule::default(),
            slot_history: SlotHistory::default(),
            runs_for_epoch: [0; KeeperOperations::LEN],
            errors_for_epoch: [0; KeeperOperations::LEN],
            txs_for_epoch: [0; KeeperOperations::LEN],
            created_accounts_for_epoch: [0; KeeperCreates::LEN],
            vote_account_map: HashMap::new(),
            validator_history_map: HashMap::new(),
            identity_to_vote_map: HashMap::new(),
            all_history_vote_account_map: HashMap::new(),
            all_get_vote_account_map: HashMap::new(),
            previous_epoch_tip_distribution_map: HashMap::new(),
            current_epoch_tip_distribution_map: HashMap::new(),
            cluster_history: ClusterHistory::zeroed(),
            keeper_balance: 0,
            all_steward_accounts: None,
            all_steward_validator_accounts: None,
            all_active_validator_accounts: None,
            steward_progress_flags: StewardProgressFlags { flags: 0 },
            cluster_name: String::new(),
        }
    }
}

impl std::fmt::Debug for KeeperState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeeperState")
            .field("epoch_info", &self.epoch_info)
            .field("runs_for_epoch", &self.runs_for_epoch)
            .field("errors_for_epoch", &self.errors_for_epoch)
            .field("vote_account_map_count", &self.vote_account_map.len())
            .field(
                "validator_history_map_count",
                &self.validator_history_map.len(),
            )
            .field(
                "all_history_vote_account_map_count",
                &self.all_history_vote_account_map.len(),
            )
            .field(
                "all_get_vote_account_map_count",
                &self.all_get_vote_account_map.len(),
            )
            .field(
                "previous_epoch_tip_distribution_map_count",
                &self.previous_epoch_tip_distribution_map.len(),
            )
            .field(
                "current_epoch_tip_distribution_map_count",
                &self.current_epoch_tip_distribution_map.len(),
            )
            // .field("cluster_history", &self.cluster_history)
            .field("keeper_balance", &self.keeper_balance)
            .finish()
    }
}
