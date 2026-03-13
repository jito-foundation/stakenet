//! This program starts several threads to manage the creation of validator history accounts,
//! and the updating of the various data feeds within the accounts.
//! It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.

use std::{collections::HashMap, str::FromStr, sync::Arc};

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_metrics::datapoint_error;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use stakenet_sdk::models::{
    entries::UpdateInstruction, errors::JitoTransactionError, submit_stats::SubmitStats,
};
use stakenet_sdk::utils::transactions::submit_instructions;
use validator_history::{ValidatorHistory, ValidatorHistoryEntry};

use crate::{
    entries::is_jito_bam_client_entry::IsJitoBamClientEntry,
    state::{keeper_config::KeeperConfig, keeper_state::KeeperState},
};

use super::keeper_operations::{check_flag, KeeperOperations};

pub struct CopyIsJitoBamClientOperation<'a> {
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

impl<'a> CopyIsJitoBamClientOperation<'a> {
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

    fn operation() -> KeeperOperations {
        KeeperOperations::CopyIsJitoBamClient
    }

    fn should_run() -> bool {
        true
    }

    fn is_uploaded(
        validator_history_map: &HashMap<Pubkey, ValidatorHistory>,
        vote_account: &Pubkey,
        epoch: u64,
    ) -> bool {
        if let Some(validator_history) = validator_history_map.get(vote_account) {
            if let Some(latest_entry) = validator_history.history.last() {
                return latest_entry.epoch == epoch as u16
                    && latest_entry.is_jito_bam_client
                        != ValidatorHistoryEntry::default().is_jito_bam_client;
            }
        }
        false
    }

    pub async fn fire(
        keeper_config: &'a KeeperConfig,
        keeper_state: &'a KeeperState,
    ) -> (KeeperOperations, u64, u64, u64) {
        let op = Self::new(keeper_config, keeper_state);
        let operation = Self::operation();

        let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) =
            keeper_state.copy_runs_errors_and_txs_for_epoch(operation);

        if Self::should_run() && check_flag(keeper_config.run_flags, operation) {
            match op.process().await {
                Ok(stats) => {
                    for message in stats.results.iter() {
                        if let Err(e) = message {
                            datapoint_error!(
                                "is-jito-bam-client-error",
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

    async fn process(&self) -> Result<SubmitStats, JitoTransactionError> {
        let epoch_info = &self.keeper_state.epoch_info;
        let validator_history_map = &self.keeper_state.validator_history_map;
        let current_epoch_tip_distribution_map =
            &self.keeper_state.current_epoch_tip_distribution_map;

        let existing_entries = current_epoch_tip_distribution_map
            .iter()
            .filter_map(|(pubkey, account)| account.as_ref().map(|_| *pubkey))
            .collect::<Vec<_>>();

        let entries_to_update = existing_entries
            .into_iter()
            .filter(|entry| !Self::is_uploaded(validator_history_map, entry, epoch_info.epoch))
            .collect::<Vec<Pubkey>>();

        let bam_validators = self
            .keeper_config
            .kobe_client
            .get_bam_validators(epoch_info.epoch)
            .await
            .map_err(|e| JitoTransactionError::Custom(e.to_string()))?
            .bam_validators;

        let update_instructions = entries_to_update
            .iter()
            .map(|vote_account| {
                let is_jito_bam_client = bam_validators.iter().any(|bam_v| {
                    Pubkey::from_str(&bam_v.vote_account)
                        .map(|pubkey| pubkey == *vote_account)
                        .unwrap_or(false)
                });

                IsJitoBamClientEntry::new(
                    *vote_account,
                    &self.program_id,
                    &self.keypair.pubkey(),
                    epoch_info.epoch,
                    is_jito_bam_client,
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
