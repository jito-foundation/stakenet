use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use keeper_core::{
    build_create_and_update_instructions, get_multiple_accounts_batched,
    get_vote_accounts_with_retry, submit_create_and_update, Address, CreateTransaction,
    CreateUpdateStats, UpdateInstruction,
};
use log::error;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::vote;
use solana_program::{instruction::Instruction, pubkey::Pubkey};
use solana_sdk::{signature::Keypair, signer::Signer};

use validator_history::constants::{MAX_ALLOC_BYTES, MIN_VOTE_EPOCHS};
use validator_history::state::ValidatorHistory;
use validator_history::{Config, ValidatorHistoryEntry};

use crate::{get_validator_history_accounts_with_retry, KeeperError, PRIORITY_FEE};

pub struct CopyVoteAccountEntry {
    pub vote_account: Pubkey,
    pub validator_history_account: Pubkey,
    pub config_address: Pubkey,
    pub program_id: Pubkey,
    pub signer: Pubkey,
}

impl CopyVoteAccountEntry {
    pub fn new(vote_account: &Pubkey, program_id: &Pubkey, signer: &Pubkey) -> Self {
        let (validator_history_account, _) = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, &vote_account.to_bytes()],
            program_id,
        );
        let (config_address, _) = Pubkey::find_program_address(&[Config::SEED], program_id);
        Self {
            vote_account: *vote_account,
            validator_history_account,
            config_address,
            program_id: *program_id,
            signer: *signer,
        }
    }
}

impl Address for CopyVoteAccountEntry {
    fn address(&self) -> Pubkey {
        self.validator_history_account
    }
}

impl CreateTransaction for CopyVoteAccountEntry {
    fn create_transaction(&self) -> Vec<Instruction> {
        let mut ixs = vec![Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::InitializeValidatorHistoryAccount {
                validator_history_account: self.validator_history_account,
                vote_account: self.vote_account,
                system_program: solana_program::system_program::id(),
                signer: self.signer,
            }
            .to_account_metas(None),
            data: validator_history::instruction::InitializeValidatorHistoryAccount {}.data(),
        }];
        let num_reallocs = (ValidatorHistory::SIZE - MAX_ALLOC_BYTES) / MAX_ALLOC_BYTES + 1;
        ixs.extend(vec![
            Instruction {
                program_id: self.program_id,
                accounts: validator_history::accounts::ReallocValidatorHistoryAccount {
                    validator_history_account: self.validator_history_account,
                    vote_account: self.vote_account,
                    config: self.config_address,
                    system_program: solana_program::system_program::id(),
                    signer: self.signer,
                }
                .to_account_metas(None),
                data: validator_history::instruction::ReallocValidatorHistoryAccount {}.data(),
            };
            num_reallocs
        ]);
        ixs
    }
}

impl UpdateInstruction for CopyVoteAccountEntry {
    fn update_instruction(&self) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::CopyVoteAccount {
                validator_history_account: self.validator_history_account,
                vote_account: self.vote_account,
                signer: self.signer,
            }
            .to_account_metas(None),
            data: validator_history::instruction::CopyVoteAccount {}.data(),
        }
    }
}

pub async fn update_vote_accounts(
    rpc_client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    validator_history_program_id: Pubkey,
) -> Result<CreateUpdateStats, KeeperError> {
    let rpc_vote_accounts =
        get_vote_accounts_with_retry(&rpc_client, MIN_VOTE_EPOCHS, None).await?;

    let validator_histories =
        get_validator_history_accounts_with_retry(&rpc_client, validator_history_program_id)
            .await?;

    let validator_history_map =
        HashMap::from_iter(validator_histories.iter().map(|vh| (vh.vote_account, vh)));
    let vote_account_pubkeys = validator_history_map
        .clone()
        .into_keys()
        .collect::<Vec<_>>();

    let vote_accounts = get_multiple_accounts_batched(&vote_account_pubkeys, &rpc_client).await?;
    let closed_vote_accounts: HashSet<Pubkey> = vote_accounts
        .iter()
        .enumerate()
        .filter_map(|(i, account)| match account {
            Some(account) => {
                if account.owner != vote::program::id() {
                    Some(vote_account_pubkeys[i])
                } else {
                    None
                }
            }
            None => Some(vote_account_pubkeys[i]),
        })
        .collect();

    // Merges new and active RPC vote accounts with all validator history accounts, and dedupes
    let mut all_vote_accounts = rpc_vote_accounts
        .iter()
        .filter_map(|rpc_va| {
            Pubkey::from_str(&rpc_va.vote_pubkey)
                .map_err(|e| {
                    error!("Invalid vote account pubkey");
                    e
                })
                .ok()
        })
        .chain(validator_histories.iter().map(|vh| vh.vote_account))
        .collect::<HashSet<_>>();

    let epoch_info = rpc_client.get_epoch_info().await?;

    // Remove closed vote accounts from all vote accounts
    // Remove vote accounts for which this instruction has been called within 50,000 slots
    all_vote_accounts.retain(|va| {
        !closed_vote_accounts.contains(va)
            && !vote_account_uploaded_recently(
                &validator_history_map,
                va,
                epoch_info.epoch,
                epoch_info.absolute_slot,
            )
    });

    let entries = all_vote_accounts
        .iter()
        .map(|va| CopyVoteAccountEntry::new(va, &validator_history_program_id, &keypair.pubkey()))
        .collect::<Vec<_>>();

    let (create_transactions, update_instructions) =
        build_create_and_update_instructions(&rpc_client, &entries).await?;

    let submit_result = submit_create_and_update(
        &rpc_client,
        create_transactions,
        update_instructions,
        &keypair,
        PRIORITY_FEE,
    )
    .await;

    submit_result.map_err(|e| e.into())
}

fn vote_account_uploaded_recently(
    validator_history_map: &HashMap<Pubkey, &ValidatorHistory>,
    vote_account: &Pubkey,
    epoch: u64,
    slot: u64,
) -> bool {
    if let Some(validator_history) = validator_history_map.get(vote_account) {
        if let Some(entry) = validator_history.history.last() {
            if entry.epoch == epoch as u16
                && entry.vote_account_last_update_slot
                    != ValidatorHistoryEntry::default().vote_account_last_update_slot
                && entry.vote_account_last_update_slot > slot - 50000
            {
                return true;
            }
        }
    }
    false
}
