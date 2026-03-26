//! Copies the Jito BAM client status for each validator
//! into their respective [`ValidatorHistory`] accounts.
//!
//! This operation queries the Kobe API to determine which validators are registered
//! BAM clients, then writes a boolean flag (`is_bam_connected`) to each validator's
//! on-chain history entry for the current epoch.
//!
//! The operation runs at 30%, 60%, and 90% epoch completion to ensure BAM connection
//! data is captured, spaced out to avoid missing all runs if the keeper is down late in the epoch.

use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::Arc,
};

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::datapoint_error;
use solana_sdk::{
    epoch_info::EpochInfo,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use stakenet_sdk::{
    models::{entries::UpdateInstruction, errors::JitoTransactionError, submit_stats::SubmitStats},
    utils::transactions::submit_instructions,
};
use validator_history::{ValidatorHistory, ValidatorHistoryEntry};

use crate::{
    entries::is_bam_connected_entry::IsBamConnectedEntry,
    state::{keeper_config::KeeperConfig, keeper_state::KeeperState},
};

use super::keeper_operations::{check_flag, KeeperOperations};

/// Manages the copying of Jito BAM client status into validator history accounts.
///
/// Constructed from [`KeeperConfig`] and [`KeeperState`], this struct holds all
/// the context needed to fetch BAM validator data from the Kobe API and submit
/// on-chain transactions that record each validator's BAM participation status.
pub struct CopyIsBamConnectedOperation<'a> {
    /// RPC Client
    client: Arc<RpcClient>,

    /// Keypair
    keypair: Arc<Keypair>,

    /// Validator History Program ID
    program_id: Pubkey,

    /// Keeper Config
    keeper_config: &'a KeeperConfig,

    /// Keeper State
    keeper_state: &'a KeeperState,

    /// Retry count
    retry_count: u16,

    /// Confirmation Time
    confirmation_time: u64,

    /// Priority Fee
    priority_fee_in_microlamports: u64,

    /// No pack
    no_pack: bool,
}

impl<'a> CopyIsBamConnectedOperation<'a> {
    /// Creates a new operation from the keeper's config and current state.
    pub fn new(keeper_config: &'a KeeperConfig, keeper_state: &'a KeeperState) -> Self {
        Self {
            client: keeper_config.client.clone(),
            keypair: keeper_config.keypair.clone(),
            program_id: keeper_config.validator_history_program_id,
            keeper_config,
            keeper_state,
            retry_count: keeper_config.tx_retry_count,
            confirmation_time: keeper_config.tx_confirmation_seconds,
            priority_fee_in_microlamports: keeper_config.priority_fee_in_microlamports,
            no_pack: keeper_config.no_pack,
        }
    }

    /// Returns the [`KeeperOperations`] variant for this operation.
    fn operation() -> KeeperOperations {
        KeeperOperations::CopyIsBamConnected
    }

    /// Returns `true` when the operation should execute.
    ///
    /// Runs up to 3 times per epoch at 30%, 60%, and 90% slot completion.
    /// Spaced out to avoid missing all runs if the keeper is down late in the epoch.
    fn should_run(epoch_info: &EpochInfo, runs_for_epoch: u64) -> bool {
        (epoch_info.slot_index > epoch_info.slots_in_epoch * 30 / 100 && runs_for_epoch < 1)
            || (epoch_info.slot_index > epoch_info.slots_in_epoch * 60 / 100 && runs_for_epoch < 2)
            || (epoch_info.slot_index > epoch_info.slots_in_epoch * 90 / 100 && runs_for_epoch < 3)
    }

    /// Checks whether the `is_jito_bam_client` field has already been written
    /// for the given vote account in the specified epoch.
    fn is_uploaded(
        validator_history_map: &HashMap<Pubkey, ValidatorHistory>,
        vote_account: &Pubkey,
        epoch: u64,
    ) -> bool {
        if let Some(validator_history) = validator_history_map.get(vote_account) {
            if let Some(latest_entry) = validator_history.history.last() {
                return latest_entry.epoch == epoch as u16
                    && latest_entry.is_bam_connected
                        != ValidatorHistoryEntry::default().is_bam_connected;
            }
        }
        false
    }

    /// Entry point for the operation. Checks whether the operation should run,
    /// executes it, and returns updated run/error/transaction counts for the epoch.
    pub async fn fire(&self) -> (KeeperOperations, u64, u64, u64) {
        let operation = Self::operation();

        let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) = self
            .keeper_state
            .copy_runs_errors_and_txs_for_epoch(operation);

        let should_run = Self::should_run(&self.keeper_state.epoch_info, runs_for_epoch)
            && check_flag(self.keeper_config.run_flags, operation);

        if should_run {
            match self.process().await {
                Ok(stats) => {
                    for message in stats.results.iter() {
                        if let Err(e) = message {
                            datapoint_error!(
                                "is-bam-connected-error",
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
                    datapoint_error!("is-jito-bam-client-error", ("error", e.to_string(), String),);
                    errors_for_epoch += 1;
                }
            }
        }

        (operation, runs_for_epoch, errors_for_epoch, txs_for_epoch)
    }

    /// Fetches BAM validator data from the Kobe API, determines each validator's
    /// BAM client status, and submits `CopyIsJitoBamClient` instructions on-chain
    /// for all validators that haven't been updated this epoch.
    async fn process(&self) -> Result<SubmitStats, JitoTransactionError> {
        let epoch_info = &self.keeper_state.epoch_info;
        let validator_history_map = &self.keeper_state.validator_history_map;
        let candidates: Vec<Pubkey> = validator_history_map.keys().copied().collect();

        // Filter out closed/reassigned vote accounts
        let mut live_vote_accounts: HashSet<Pubkey> = HashSet::new();
        for chunk in candidates.chunks(100) {
            let accounts = self
                .client
                .get_multiple_accounts(chunk)
                .await
                .map_err(|e| JitoTransactionError::Custom(e.to_string()))?;
            for (pubkey, account) in chunk.iter().zip(accounts.iter()) {
                if account.is_some() {
                    live_vote_accounts.insert(*pubkey);
                }
            }
        }

        let entries_to_update: Vec<Pubkey> = candidates
            .into_iter()
            .filter(|pubkey| live_vote_accounts.contains(pubkey))
            .collect();

        let bam_validators = self
            .keeper_config
            .kobe_client
            .get_bam_validators(epoch_info.epoch)
            .await
            .map_err(|e| JitoTransactionError::Custom(e.to_string()))?
            .bam_validators;

        let bam_vote_accounts: HashSet<Pubkey> = bam_validators
            .iter()
            .filter_map(|bam_v| Pubkey::from_str(&bam_v.vote_account).ok())
            .collect();

        let update_instructions = entries_to_update
            .iter()
            .map(|vote_account| {
                let is_bam_connected = bam_vote_accounts.contains(vote_account);

                IsBamConnectedEntry::new(
                    *vote_account,
                    &self.program_id,
                    &self.keypair.pubkey(),
                    epoch_info.epoch,
                    is_bam_connected,
                )
                .update_instruction()
            })
            .collect::<Vec<_>>();

        submit_instructions(
            &self.client,
            update_instructions,
            &self.keypair,
            self.priority_fee_in_microlamports,
            self.retry_count,
            self.confirmation_time,
            None,
            self.no_pack,
        )
        .await
        .map_err(|e| e.into())
    }
}
