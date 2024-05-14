/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/

use std::{
    collections::{HashMap, HashSet},
    default,
    error::Error,
    fmt,
    net::SocketAddr,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use anchor_lang::AccountDeserialize;
use clap::{arg, command, Parser};
use keeper_core::{
    get_multiple_accounts_batched, get_vote_accounts_with_retry, submit_instructions,
    submit_transactions, Cluster, CreateUpdateStats, SubmitStats, TransactionExecutionError,
};
use log::*;
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_response::RpcVoteAccountInfo};
use solana_metrics::{datapoint_error, set_host_id};
use solana_sdk::{
    blake3::Hash,
    epoch_info::{self, EpochInfo},
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
};
use tokio::time::sleep;
use validator_history::{constants::MIN_VOTE_EPOCHS, ValidatorHistory};

use crate::operations::keeper_operations::KeeperOperations;

pub struct KeeperState {
    pub epoch_info: EpochInfo,

    pub runs_for_epoch: [u64; KeeperOperations::LEN],
    pub errors_for_epoch: [u64; KeeperOperations::LEN],
    // submit_stats: [SubmitStats; KeeperOperations::LEN],
    pub closed_vote_accounts: HashSet<Pubkey>,
    pub vote_account_map: HashMap<Pubkey, RpcVoteAccountInfo>,
    pub validator_history_map: HashMap<Pubkey, ValidatorHistory>,
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
            // submit_stats: [SubmitStats::default(); KeeperOperations::LEN],
            closed_vote_accounts: HashSet::new(),
            vote_account_map: HashMap::new(),
            validator_history_map: HashMap::new(),
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
