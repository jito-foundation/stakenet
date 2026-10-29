//! Copies each validator's leader slot count for the current epoch into their
//! respective ValidatorHistory accounts.
//!
//! This is a lightweight crank of `update_priority_fee_history` that only fills
//! `total_leader_slots`, derived from the cluster leader schedule. Every other field written
//! by that instruction (`total_priority_fees`, `blocks_produced`, `block_data_updated_at_slot`)
//! is set to the [`ValidatorHistoryEntry`] default (i.e. "unset"), leaving the block metadata
//! keeper free to fill them in later with real data.
//!
//! The leader schedule for an epoch is fixed at the start of that epoch, so this runs once per
//! epoch and only for validators whose `total_leader_slots` is still unset, which keeps it from
//! clobbering anything the block metadata keeper has already uploaded.

use std::{collections::HashMap, sync::Arc};

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
use validator_history::ValidatorHistoryEntry;

use crate::{
    entries::priority_fee_and_block_metadata_entry::PriorityFeeAndBlockMetadataEntry,
    operations::block_metadata::operations::get_leader_schedule_safe,
    state::{keeper_config::KeeperConfig, keeper_state::KeeperState},
};

use super::keeper_operations::{check_flag, KeeperOperations};

/// Manages copying the leader schedule slot counts into validator history accounts.
pub struct UpdateLeaderSlotsOperation<'a> {
    /// RPC Client
    client: Arc<RpcClient>,

    /// Priority fee oracle authority keypair - the signer for `update_priority_fee_history`
    priority_fee_oracle_authority_keypair: Arc<Keypair>,

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

impl<'a> UpdateLeaderSlotsOperation<'a> {
    /// Creates a new operation from the keeper's config and current state.
    pub fn new(keeper_config: &'a KeeperConfig, keeper_state: &'a KeeperState) -> Self {
        Self {
            client: keeper_config.client.clone(),
            priority_fee_oracle_authority_keypair: keeper_config
                .priority_fee_oracle_authority_keypair
                .clone()
                .expect(
                    "UpdateLeaderSlotsOperation requires priority_fee_oracle_authority_keypair",
                ),
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
        KeeperOperations::UpdateLeaderSlots
    }

    /// Returns `true` when the operation should execute.
    fn should_run(epoch_info: &EpochInfo, runs_for_epoch: u64) -> bool {
        epoch_info.slot_index > epoch_info.slots_in_epoch / 100 && runs_for_epoch < 1
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
                                "update-leader-slots-error",
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
                    datapoint_error!(
                        "update-leader-slots-error",
                        ("error", e.to_string(), String),
                    );
                    errors_for_epoch += 1;
                }
            }
        }

        (operation, runs_for_epoch, errors_for_epoch, txs_for_epoch)
    }

    /// Fetches the current epoch's leader schedule, counts the leader slots for each validator,
    /// and submits `update_priority_fee_history` instructions for every validator that does not
    /// have `total_leader_slots` recorded yet.
    async fn process(&self) -> Result<SubmitStats, JitoTransactionError> {
        let epoch = self.keeper_state.epoch_info.epoch;
        let first_slot_in_epoch = self
            .keeper_state
            .epoch_schedule
            .get_first_slot_in_epoch(epoch);

        let leader_schedule = get_leader_schedule_safe(&self.client, first_slot_in_epoch)
            .await
            .map_err(|e| JitoTransactionError::Custom(e.to_string()))?;

        let leader_slots_by_vote_account = self.leader_slots_by_vote_account(&leader_schedule);

        let default_entry = ValidatorHistoryEntry::default();

        let update_instructions = leader_slots_by_vote_account
            .into_iter()
            .filter(|(vote_account, _)| self.needs_update(vote_account, epoch))
            .map(|(vote_account, total_leader_slots)| {
                let mut entry = PriorityFeeAndBlockMetadataEntry::new(
                    &vote_account,
                    epoch,
                    &self.program_id,
                    &self.priority_fee_oracle_authority_keypair.pubkey(),
                );

                entry.total_leader_slots = total_leader_slots;
                entry.total_priority_fees = default_entry.total_priority_fees;
                entry.blocks_produced = default_entry.blocks_produced;
                entry.highest_global_done_slot = default_entry.block_data_updated_at_slot;

                entry.update_instruction()
            })
            .collect::<Vec<_>>();

        log::info!(
            "Updating leader slots epoch={} instructions={}",
            epoch,
            update_instructions.len()
        );

        submit_instructions(
            &self.client,
            update_instructions,
            &self.priority_fee_oracle_authority_keypair,
            self.priority_fee_in_microlamports,
            self.retry_count,
            self.confirmation_time,
            None,
            self.no_pack,
        )
        .await
        .map_err(|e| e.into())
    }

    /// Converts a leader schedule (identity -> slot indices) into a map of
    /// vote account -> number of leader slots, skipping identities that have no known
    /// vote account or no validator history account.
    fn leader_slots_by_vote_account(
        &self,
        leader_schedule: &HashMap<String, Vec<usize>>,
    ) -> HashMap<Pubkey, u32> {
        let mut leader_slots: HashMap<Pubkey, u32> = HashMap::new();
        let mut missing_vote_accounts = 0;

        for (identity, slots) in leader_schedule.iter() {
            let Some(vote_account) = self
                .keeper_state
                .identity_to_vote_map
                .get(identity)
                .and_then(|vote_account| vote_account.parse::<Pubkey>().ok())
            else {
                missing_vote_accounts += 1;
                continue;
            };

            if !self
                .keeper_state
                .validator_history_map
                .contains_key(&vote_account)
            {
                continue;
            }

            *leader_slots.entry(vote_account).or_default() += slots.len() as u32;
        }

        if missing_vote_accounts > 0 {
            log::warn!("Missing vote accounts for leaders count={missing_vote_accounts}");
        }

        leader_slots
    }

    /// Returns `true` when the validator has no block metadata recorded for `epoch` yet.
    ///
    /// Because this crank writes the default ("unset") value for every field other than
    /// `total_leader_slots`, it must only run against entries the block metadata keeper has not
    /// touched. The block metadata keeper writes all four fields together, so requiring every one
    /// of them to still be unset guarantees we never clobber real data.
    fn needs_update(&self, vote_account: &Pubkey, epoch: u64) -> bool {
        let Some(validator_history) = self.keeper_state.validator_history_map.get(vote_account)
        else {
            return false;
        };

        match validator_history
            .history
            .arr
            .iter()
            .find(|history| history.epoch as u64 == epoch)
        {
            Some(entry) => {
                let default_entry = ValidatorHistoryEntry::default();
                entry.total_leader_slots == default_entry.total_leader_slots
                    && entry.total_priority_fees == default_entry.total_priority_fees
                    && entry.blocks_produced == default_entry.blocks_produced
                    && entry.block_data_updated_at_slot == default_entry.block_data_updated_at_slot
            }
            None => true,
        }
    }
}
