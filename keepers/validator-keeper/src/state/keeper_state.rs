use std::collections::{HashMap, HashSet};

use bytemuck::Zeroable;
use solana_client::rpc_response::RpcVoteAccountInfo;
use solana_metrics::datapoint_info;
use solana_sdk::{
    account::Account, epoch_info::EpochInfo, pubkey::Pubkey,
    vote::program::id as get_vote_program_id,
};
use validator_history::{ClusterHistory, ValidatorHistory};

use crate::{
    derive_validator_history_address,
    operations::keeper_operations::{KeeperCreates, KeeperOperations},
};

pub struct KeeperState {
    pub epoch_info: EpochInfo,

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
}
impl KeeperState {
    pub fn new() -> Self {
        Self::default()
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
            .map(|vote_account| derive_validator_history_address(vote_account, program_id))
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
        )
    }
}

impl Default for KeeperState {
    fn default() -> Self {
        Self {
            epoch_info: EpochInfo {
                epoch: 0,
                slot_index: 0,
                slots_in_epoch: 0,
                absolute_slot: 0,
                block_height: 0,
                transaction_count: None,
            },
            runs_for_epoch: [0; KeeperOperations::LEN],
            errors_for_epoch: [0; KeeperOperations::LEN],
            txs_for_epoch: [0; KeeperOperations::LEN],
            created_accounts_for_epoch: [0; KeeperCreates::LEN],
            vote_account_map: HashMap::new(),
            validator_history_map: HashMap::new(),
            all_history_vote_account_map: HashMap::new(),
            all_get_vote_account_map: HashMap::new(),
            previous_epoch_tip_distribution_map: HashMap::new(),
            current_epoch_tip_distribution_map: HashMap::new(),
            cluster_history: ClusterHistory::zeroed(),
            keeper_balance: 0,
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
