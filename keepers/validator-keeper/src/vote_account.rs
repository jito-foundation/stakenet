use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use keeper_core::{
    build_create_and_update_instructions, get_vote_accounts_with_retry, submit_create_and_update,
    Address, CreateTransaction, CreateUpdateStats, MultipleAccountsError,
    TransactionExecutionError, UpdateInstruction,
};
use log::error;
use solana_client::rpc_response::RpcVoteAccountInfo;
use solana_client::{client_error::ClientError, nonblocking::rpc_client::RpcClient};
use solana_program::{instruction::Instruction, pubkey::Pubkey};
use solana_sdk::{signature::Keypair, signer::Signer};
use thiserror::Error as ThisError;
use validator_history::constants::{MAX_ALLOC_BYTES, MIN_VOTE_EPOCHS};
use validator_history::state::ValidatorHistory;
use validator_history::Config;

use crate::get_validator_history_accounts_with_retry;

#[derive(ThisError, Debug)]
pub enum UpdateCommissionError {
    #[error(transparent)]
    ClientError(#[from] ClientError),
    #[error(transparent)]
    TransactionExecutionError(#[from] TransactionExecutionError),
    #[error(transparent)]
    MultipleAccountsError(#[from] MultipleAccountsError),
}

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
) -> Result<CreateUpdateStats, (UpdateCommissionError, CreateUpdateStats)> {
    let stats = CreateUpdateStats::default();

    let rpc_vote_accounts = get_vote_accounts_with_retry(&rpc_client, MIN_VOTE_EPOCHS, None)
        .await
        .map_err(|e| (e.into(), stats))?;

    let validator_histories =
        get_validator_history_accounts_with_retry(&rpc_client, validator_history_program_id)
            .await
            .map_err(|e| (e.into(), stats))?;

    // Merges new and active RPC vote accounts with all validator history accounts, and dedupes
    let vote_accounts = rpc_vote_accounts
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

    let entries = vote_accounts
        .iter()
        .map(|va| CopyVoteAccountEntry::new(va, &validator_history_program_id, &keypair.pubkey()))
        .collect::<Vec<_>>();

    let (create_transactions, update_instructions) =
        build_create_and_update_instructions(&rpc_client, &entries)
            .await
            .map_err(|e| (e.into(), stats))?;

    let submit_result = submit_create_and_update(
        &rpc_client,
        create_transactions,
        update_instructions,
        &keypair,
    )
    .await;

    submit_result.map_err(|(e, stats)| (e.into(), stats))
}
