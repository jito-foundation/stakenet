/*
This program starts several threads to manage the creation of validator history accounts,
and the updating of the various data feeds within the accounts.
It will emits metrics for each data feed, if env var SOLANA_METRICS_CONFIG is set to a valid influx server.
*/

use crate::state::keeper_state::KeeperState;
use crate::{derive_validator_history_config_address, KeeperError, PRIORITY_FEE};
use anchor_lang::{InstructionData, ToAccountMetas};
use jito_tip_distribution::sdk::derive_tip_distribution_account_address;
use keeper_core::{
    get_multiple_accounts_batched, submit_instructions, Address, MultipleAccountsError,
    SubmitStats, TransactionExecutionError, UpdateInstruction,
};
use log::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_response::RpcVoteAccountInfo;
use solana_metrics::datapoint_error;
use solana_metrics::datapoint_info;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::{collections::HashMap, str::FromStr, sync::Arc};
use validator_history::ValidatorHistory;
use validator_history::ValidatorHistoryEntry;

use super::keeper_operations::KeeperOperations;

fn _get_operation() -> KeeperOperations {
    return KeeperOperations::MevCommission;
}

fn _should_run() -> bool {
    true
}

async fn _process(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    tip_distribution_program_id: &Pubkey,
    keeper_state: &KeeperState,
) -> Result<SubmitStats, KeeperError> {
    update_mev_commission(
        client,
        keypair,
        program_id,
        tip_distribution_program_id,
        keeper_state,
    )
    .await
}

fn _emit(stats: &SubmitStats, runs_for_epoch: i64, errors_for_epoch: i64) {
    datapoint_info!(
        "mev-commission-stats",
        ("num_updates_success", stats.successes, i64),
        ("num_updates_error", stats.errors, i64),
        ("runs_for_epoch", runs_for_epoch, i64),
        ("errors_for_epoch", errors_for_epoch, i64),
    );
}

pub async fn fire_and_emit(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    tip_distribution_program_id: &Pubkey,
    keeper_state: &KeeperState,
) -> (KeeperOperations, u64, u64) {
    let operation = _get_operation();
    let (mut runs_for_epoch, mut errors_for_epoch) =
        keeper_state.copy_runs_and_errors_for_epoch(operation.clone());

    let stats = match _process(
        client,
        keypair,
        program_id,
        tip_distribution_program_id,
        keeper_state,
    )
    .await
    {
        Ok(stats) => {
            for message in stats.results.iter().chain(stats.results.iter()) {
                if let Err(e) = message {
                    datapoint_error!("vote-account-error", ("error", e.to_string(), String),);
                }
            }
            runs_for_epoch += 1;
            stats
        }
        Err(e) => {
            let mut stats = SubmitStats::default();
            if let KeeperError::TransactionExecutionError(
                TransactionExecutionError::TransactionClientError(_, results),
            ) = &e
            {
                stats.successes = results.iter().filter(|r| r.is_ok()).count() as u64;
                stats.errors = results.iter().filter(|r| r.is_err()).count() as u64;
            }
            datapoint_error!("mev-earned-error", ("error", e.to_string(), String),);
            errors_for_epoch += 1;
            stats
        }
    };

    _emit(&stats, runs_for_epoch as i64, errors_for_epoch as i64);

    (operation, runs_for_epoch, errors_for_epoch)
}

// ----------------- OPERATION SPECIFIC FUNCTIONS -----------------

//TODO Move this to keeper_core?
#[derive(Clone)]
pub struct ValidatorMevCommissionEntry {
    pub vote_account: Pubkey,
    pub tip_distribution_account: Pubkey,
    pub validator_history_account: Pubkey,
    pub config: Pubkey,
    pub program_id: Pubkey,
    pub signer: Pubkey,
    pub epoch: u64,
}

impl ValidatorMevCommissionEntry {
    pub fn new(
        vote_account: &RpcVoteAccountInfo,
        epoch: u64,
        program_id: &Pubkey,
        tip_distribution_program_id: &Pubkey,
        signer: &Pubkey,
    ) -> Self {
        let vote_account = Pubkey::from_str(&vote_account.vote_pubkey)
            .map_err(|e| {
                error!("Invalid vote account pubkey");
                e
            })
            .expect("Invalid vote account pubkey");
        let (validator_history_account, _) = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, &vote_account.to_bytes()],
            program_id,
        );
        let (tip_distribution_account, _) = derive_tip_distribution_account_address(
            tip_distribution_program_id,
            &vote_account,
            epoch,
        );
        let config = derive_validator_history_config_address(program_id);
        Self {
            vote_account,
            tip_distribution_account,
            validator_history_account,
            config,
            program_id: *program_id,
            signer: *signer,
            epoch,
        }
    }
}

impl Address for ValidatorMevCommissionEntry {
    fn address(&self) -> Pubkey {
        self.validator_history_account
    }
}

impl UpdateInstruction for ValidatorMevCommissionEntry {
    fn update_instruction(&self) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::CopyTipDistributionAccount {
                validator_history_account: self.validator_history_account,
                vote_account: self.vote_account,
                tip_distribution_account: self.tip_distribution_account,
                config: self.config,
                signer: self.signer,
            }
            .to_account_metas(None),
            data: validator_history::instruction::CopyTipDistributionAccount { epoch: self.epoch }
                .data(),
        }
    }
}

pub async fn update_mev_commission(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    validator_history_program_id: &Pubkey,
    tip_distribution_program_id: &Pubkey,
    keeper_state: &KeeperState,
) -> Result<SubmitStats, KeeperError> {
    let epoch_info = &keeper_state.epoch_info;
    let vote_accounts = &keeper_state.vote_account_map.values().collect::<Vec<_>>();
    let validator_history_map = &keeper_state.validator_history_map;

    let entries = vote_accounts
        .iter()
        .map(|vote_account| {
            ValidatorMevCommissionEntry::new(
                vote_account,
                epoch_info.epoch,
                validator_history_program_id,
                tip_distribution_program_id,
                &keypair.pubkey(),
            )
        })
        .collect::<Vec<ValidatorMevCommissionEntry>>();

    let existing_entries = get_existing_entries(client.clone(), &entries).await?;

    let entries_to_update = existing_entries
        .into_iter()
        .filter(|entry| {
            !mev_commission_uploaded(&validator_history_map, entry.address(), epoch_info.epoch)
        })
        .collect::<Vec<ValidatorMevCommissionEntry>>();

    let update_instructions = entries_to_update
        .iter()
        .map(|validator_mev_commission_entry| validator_mev_commission_entry.update_instruction())
        .collect::<Vec<_>>();

    let submit_result =
        submit_instructions(client, update_instructions, keypair, PRIORITY_FEE).await;

    submit_result.map_err(|e| e.into())
}

async fn get_existing_entries(
    client: Arc<RpcClient>,
    entries: &[ValidatorMevCommissionEntry],
) -> Result<Vec<ValidatorMevCommissionEntry>, MultipleAccountsError> {
    /* Filters tip distribution tuples to the addresses, then fetches accounts to see which ones exist */
    let tip_distribution_addresses = entries
        .iter()
        .map(|entry| entry.tip_distribution_account)
        .collect::<Vec<Pubkey>>();

    let accounts = get_multiple_accounts_batched(&tip_distribution_addresses, &client).await?;
    let result = accounts
        .iter()
        .enumerate()
        .filter_map(|(i, account_data)| {
            if account_data.is_some() {
                Some(entries[i].clone())
            } else {
                None
            }
        })
        .collect::<Vec<ValidatorMevCommissionEntry>>();
    // Fetch existing tip distribution accounts for this epoch
    Ok(result)
}

fn mev_commission_uploaded(
    validator_history_map: &HashMap<Pubkey, ValidatorHistory>,
    vote_account: Pubkey,
    epoch: u64,
) -> bool {
    if let Some(validator_history) = validator_history_map.get(&vote_account) {
        if let Some(latest_entry) = validator_history.history.last() {
            return latest_entry.epoch == epoch as u16
                && latest_entry.mev_commission != ValidatorHistoryEntry::default().mev_commission;
        }
    }
    false
}
