use std::collections::{HashMap, HashSet};

use solana_client::rpc_response::RpcVoteAccountInfo;
use solana_sdk::{epoch_info::EpochInfo, pubkey::Pubkey};
use validator_history::ValidatorHistory;

use crate::operations::keeper_operations::KeeperOperations;

pub struct KeeperState {
    pub epoch_info: EpochInfo,

    pub runs_for_epoch: [u64; KeeperOperations::LEN],
    pub errors_for_epoch: [u64; KeeperOperations::LEN],
    // submit_stats: [SubmitStats; KeeperOperations::LEN],
    pub closed_vote_accounts: HashSet<Pubkey>,
    pub vote_account_map: HashMap<Pubkey, RpcVoteAccountInfo>,
    pub validator_history_map: HashMap<Pubkey, ValidatorHistory>,
    pub keeper_balance: u64,
}
impl KeeperState {
    pub fn new() -> Self {
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
            closed_vote_accounts: HashSet::new(),
            vote_account_map: HashMap::new(),
            validator_history_map: HashMap::new(),
            keeper_balance: 0,
        }
    }

    pub fn get_mut_runs_for_epoch(&mut self, operation: KeeperOperations) -> &mut u64 {
        &mut self.runs_for_epoch[operation as usize]
    }

    pub fn get_mut_errors_for_epoch(&mut self, operation: KeeperOperations) -> &mut u64 {
        &mut self.errors_for_epoch[operation as usize]
    }

    pub fn copy_runs_and_errors_for_epoch(&self, operation: KeeperOperations) -> (u64, u64) {
        let index = operation as usize;
        (
            self.runs_for_epoch[index].clone(),
            self.errors_for_epoch[index].clone(),
        )
    }

    pub fn set_runs_and_errors_for_epoch(
        &mut self,
        (operation, runs_for_epoch, errors_for_epoch): (KeeperOperations, u64, u64),
    ) {
        let index = operation as usize;
        self.runs_for_epoch[index] = runs_for_epoch;
        self.errors_for_epoch[index] = errors_for_epoch;
    }
}
