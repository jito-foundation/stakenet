//! Records how many leader slots each validator is scheduled for in the current epoch.
//!
//! Alpenglow pays half of each slot's rewards to its leader, so the participation ratio
//! needs each validator's `total_leader_slots`. The block metadata keeper also writes this
//! field, but only as a side effect of walking every block of the epoch, which lags well
//! behind the epoch it describes. This operation writes the same value from a single
//! `getLeaderSchedule` call, so the field is correct from the start of the epoch.
//!
//! The leader schedule is keyed by validator identity, so entries whose identity has no
//! known vote account, or whose vote account has no validator history account, are skipped.
//! Validators whose on-chain entry already holds the scheduled count are skipped too, which
//! makes the operation a no-op once it has succeeded for the epoch.

use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::{
    entries::leader_slots_entry::LeaderSlotsEntry,
    state::{keeper_config::KeeperConfig, keeper_state::KeeperState},
};
use log::*;
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

use super::keeper_operations::{check_flag, KeeperOperations};

fn _get_operation() -> KeeperOperations {
    KeeperOperations::UpdateLeaderSlots
}

/// Returns `true` when the operation should execute.
///
/// Runs at 1% and 50% epoch completion. The leader schedule is fixed for the whole epoch, so
/// the second run only exists to cover validator history accounts created after the first.
fn _should_run(epoch_info: &EpochInfo, runs_for_epoch: u64) -> bool {
    (epoch_info.slot_index > epoch_info.slots_in_epoch / 100 && runs_for_epoch < 1)
        || (epoch_info.slot_index > epoch_info.slots_in_epoch * 5 / 10 && runs_for_epoch < 2)
}

pub async fn fire(
    keeper_config: &KeeperConfig,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64, u64) {
    let client = &keeper_config.client;
    let program_id = &keeper_config.validator_history_program_id;
    let oracle_authority_keypair = keeper_config
        .oracle_authority_keypair
        .as_ref()
        .expect("update_leader_slots requires oracle_authority_keypair");

    let operation = _get_operation();
    let (mut runs_for_epoch, mut errors_for_epoch, mut txs_for_epoch) =
        keeper_state.copy_runs_errors_and_txs_for_epoch(operation);

    let should_run = _should_run(&keeper_state.epoch_info, runs_for_epoch)
        && check_flag(keeper_config.run_flags, operation);

    if should_run {
        match update_leader_slots(
            client,
            oracle_authority_keypair,
            program_id,
            keeper_config.priority_fee_in_microlamports,
            keeper_config.tx_retry_count,
            keeper_config.tx_confirmation_seconds,
            keeper_state,
            keeper_config.no_pack,
        )
        .await
        {
            Ok(stats) => {
                for message in stats.results.iter() {
                    if let Err(e) = message {
                        datapoint_error!("leader-slots-error", ("error", e.to_string(), String),);
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
                datapoint_error!("leader-slots-error", ("error", e.to_string(), String),);
                errors_for_epoch += 1;
            }
        };
    }

    (operation, runs_for_epoch, errors_for_epoch, txs_for_epoch)
}

// ----------------- OPERATION SPECIFIC FUNCTIONS -----------------

/// Fetches the current epoch's leader schedule and submits `UpdateLeaderSlots` instructions
/// for every validator whose on-chain count does not already match the schedule.
#[allow(clippy::too_many_arguments)]
pub async fn update_leader_slots(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    priority_fee_in_microlamports: u64,
    retry_count: u16,
    confirmation_time: u64,
    keeper_state: &KeeperState,
    no_pack: bool,
) -> Result<SubmitStats, JitoTransactionError> {
    let epoch = keeper_state.epoch_info.epoch;
    let first_slot_in_epoch = keeper_state.epoch_schedule.get_first_slot_in_epoch(epoch);

    let leader_schedule = client
        .get_leader_schedule(Some(first_slot_in_epoch))
        .await
        .map_err(|e| JitoTransactionError::Custom(e.to_string()))?
        .ok_or_else(|| {
            JitoTransactionError::Custom(format!("No leader schedule for epoch {epoch}"))
        })?;

    let leader_slots = leader_slots_by_vote_account(&leader_schedule, keeper_state, epoch);

    let update_instructions = leader_slots
        .into_iter()
        .filter(|(vote_account, total_leader_slots)| {
            !leader_slots_uploaded(keeper_state, vote_account, epoch, *total_leader_slots)
        })
        .map(|(vote_account, total_leader_slots)| {
            LeaderSlotsEntry::new(
                vote_account,
                program_id,
                &keypair.pubkey(),
                epoch,
                total_leader_slots,
            )
            .update_instruction()
        })
        .collect::<Vec<_>>();

    submit_instructions(
        client,
        update_instructions,
        keypair,
        priority_fee_in_microlamports,
        retry_count,
        confirmation_time,
        None,
        no_pack,
    )
    .await
    .map_err(|e| e.into())
}

/// Converts an identity-keyed leader schedule into a slot count per vote account.
///
/// Leaders without a known vote account, without a validator history account, or whose vote
/// account is no longer owned by the vote program are dropped: the first two have nowhere to
/// write to, and the last would fail the instruction's `owner` constraint.
fn leader_slots_by_vote_account(
    leader_schedule: &HashMap<String, Vec<usize>>,
    keeper_state: &KeeperState,
    epoch: u64,
) -> Vec<(Pubkey, u32)> {
    let live_vote_accounts = keeper_state.get_live_vote_accounts();
    let mut missing_vote_account = 0usize;
    let mut missing_history = 0usize;
    let mut closed_vote_account = 0usize;

    let leader_slots = leader_schedule
        .iter()
        .filter_map(|(identity, slots)| {
            let Some(vote_account) = keeper_state.identity_to_vote_map.get(identity) else {
                missing_vote_account += 1;
                return None;
            };

            let vote_account = Pubkey::from_str(vote_account)
                .inspect_err(|e| error!("Invalid vote account pubkey {vote_account}: {e}"))
                .ok()?;

            if !keeper_state
                .validator_history_map
                .contains_key(&vote_account)
            {
                missing_history += 1;
                return None;
            }

            if !live_vote_accounts.contains(&vote_account) {
                closed_vote_account += 1;
                return None;
            }

            // A leader can hold at most `slots_in_epoch` slots, which fits in a `u32`.
            Some((vote_account, slots.len() as u32))
        })
        .collect::<Vec<_>>();

    if missing_vote_account > 0 || missing_history > 0 || closed_vote_account > 0 {
        warn!(
            "Skipping leaders epoch={epoch} missing_vote_account={missing_vote_account} missing_history={missing_history} closed_vote_account={closed_vote_account}"
        );
    }

    leader_slots
}

/// Returns `true` when the validator's entry for `epoch` already holds `total_leader_slots`.
fn leader_slots_uploaded(
    keeper_state: &KeeperState,
    vote_account: &Pubkey,
    epoch: u64,
    total_leader_slots: u32,
) -> bool {
    keeper_state
        .validator_history_map
        .get(vote_account)
        .and_then(|validator_history| {
            validator_history
                .history
                .arr
                .iter()
                .find(|entry| entry.epoch == epoch as u16)
        })
        .is_some_and(|entry| entry.total_leader_slots == total_leader_slots)
}
