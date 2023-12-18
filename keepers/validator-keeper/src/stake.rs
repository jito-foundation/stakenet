use std::{collections::HashMap, str::FromStr, sync::Arc};

use anchor_lang::{AccountDeserialize, Discriminator, InstructionData, ToAccountMetas};
use keeper_core::{
    build_create_and_update_instructions, get_vote_accounts_with_retry, submit_create_and_update,
    submit_instructions, Address, CreateTransaction, CreateUpdateStats, MultipleAccountsError,
    SubmitStats, TransactionExecutionError, UpdateInstruction,
};
use log::error;
use solana_client::{
    client_error::ClientError,
    nonblocking::rpc_client::RpcClient,
    rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    rpc_filter::{Memcmp, RpcFilterType},
    rpc_response::RpcVoteAccountInfo,
};
use solana_metrics::datapoint_info;
use solana_sdk::{
    commitment_config::CommitmentConfig, instruction::Instruction, pubkey::Pubkey,
    signature::Keypair, signer::Signer,
};
use thiserror::Error as ThisError;
use validator_history::{
    constants::{MAX_ALLOC_BYTES, MIN_VOTE_EPOCHS},
    state::{Config, ValidatorHistory},
};

#[derive(ThisError, Debug)]
pub enum StakeHistoryError {
    #[error(transparent)]
    ClientError(#[from] ClientError),
    #[error(transparent)]
    TransactionExecutionError(#[from] TransactionExecutionError),
    #[error(transparent)]
    MultipleAccountsError(#[from] MultipleAccountsError),
    #[error("Epoch mismatch")]
    EpochMismatch,
}

pub struct StakeHistoryEntry {
    pub stake: u64,
    pub rank: u32,
    pub is_superminority: bool,
    pub vote_account: Pubkey,
    pub address: Pubkey,
    pub config_address: Pubkey,
    pub signer: Pubkey,
    pub program_id: Pubkey,
    pub epoch: u64,
}

impl StakeHistoryEntry {
    pub fn new(
        vote_account: &RpcVoteAccountInfo,
        program_id: &Pubkey,
        signer: &Pubkey,
        epoch: u64,
        rank: u32,
        is_superminority: bool,
    ) -> StakeHistoryEntry {
        let vote_pubkey = Pubkey::from_str(&vote_account.vote_pubkey)
            .map_err(|e| {
                error!("Invalid vote account pubkey");
                e
            })
            .expect("Invalid vote account pubkey");
        let (address, _) = Pubkey::find_program_address(
            &[ValidatorHistory::SEED, &vote_pubkey.to_bytes()],
            program_id,
        );
        let (config_address, _) = Pubkey::find_program_address(&[Config::SEED], program_id);

        StakeHistoryEntry {
            stake: vote_account.activated_stake,
            rank,
            is_superminority,
            vote_account: vote_pubkey,
            address,
            config_address,
            signer: *signer,
            program_id: *program_id,
            epoch,
        }
    }
}

impl CreateTransaction for StakeHistoryEntry {
    fn create_transaction(&self) -> Vec<Instruction> {
        let mut ixs = vec![Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::InitializeValidatorHistoryAccount {
                validator_history_account: self.address,
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
                    validator_history_account: self.address,
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

impl Address for StakeHistoryEntry {
    fn address(&self) -> Pubkey {
        self.address
    }
}

impl UpdateInstruction for StakeHistoryEntry {
    fn update_instruction(&self) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: validator_history::accounts::UpdateStakeHistory {
                validator_history_account: self.address,
                vote_account: self.vote_account,
                config: self.config_address,
                stake_authority: self.signer,
            }
            .to_account_metas(None),
            data: validator_history::instruction::UpdateStakeHistory {
                lamports: self.stake,
                epoch: self.epoch,
                rank: self.rank,
                is_superminority: self.is_superminority,
            }
            .data(),
        }
    }
}

/*
Calculates ordering of validators by stake, assigning a 0..N rank (validator 0 has the most stake),
and returns the index at which all validators before are in the superminority. 0-indexed.
*/
fn get_stake_rank_map_and_superminority_count(
    vote_accounts: &[RpcVoteAccountInfo],
) -> (HashMap<String, u32>, u32) {
    let mut stake_vec = vote_accounts
        .iter()
        .map(|va| (va.vote_pubkey.clone(), va.activated_stake))
        .collect::<Vec<_>>();

    let total_stake = stake_vec.iter().map(|(_, stake)| *stake).sum::<u64>();
    stake_vec.sort_by(|a, b| b.1.cmp(&a.1));

    let mut cumulative_stake = 0;
    let mut superminority_threshold = 0;
    for (i, (_, stake)) in stake_vec.iter().enumerate() {
        cumulative_stake += stake;
        if cumulative_stake > total_stake / 3 {
            superminority_threshold = i as u32;
            break;
        }
    }
    let stake_rank_map = HashMap::from_iter(
        stake_vec
            .into_iter()
            .enumerate()
            .map(|(i, (vote_pubkey, _))| (vote_pubkey, i as u32)),
    );

    (stake_rank_map, superminority_threshold)
}

pub async fn update_stake_history(
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    program_id: &Pubkey,
) -> Result<CreateUpdateStats, (StakeHistoryError, CreateUpdateStats)> {
    let vote_accounts = get_vote_accounts_with_retry(
        &client,
        MIN_VOTE_EPOCHS,
        Some(CommitmentConfig::finalized()),
    )
    .await
    .map_err(|e| (e.into(), CreateUpdateStats::default()))?;

    // Need to ensure that the response contains update stake amounts for the current epoch,
    // so we find the largest epoch a validator has voted on to confirm the data is fresh
    let max_vote_account_epoch = vote_accounts
        .iter()
        .flat_map(|va| va.epoch_credits.clone())
        .map(|(epoch, _, _)| epoch)
        .max()
        .unwrap_or(0);

    let (stake_rank_map, superminority_threshold) =
        get_stake_rank_map_and_superminority_count(&vote_accounts);

    let epoch = client
        .get_epoch_info_with_commitment(CommitmentConfig::finalized())
        .await
        .map_err(|e| (e.into(), CreateUpdateStats::default()))?
        .epoch;

    if max_vote_account_epoch != epoch {
        return Err((
            StakeHistoryError::EpochMismatch,
            CreateUpdateStats::default(),
        ));
    }

    let stake_history_entries = vote_accounts
        .iter()
        .map(|va| {
            let rank = stake_rank_map[&va.vote_pubkey.clone()];
            let is_superminority = rank <= superminority_threshold;
            StakeHistoryEntry::new(
                va,
                program_id,
                &keypair.pubkey(),
                epoch,
                rank,
                is_superminority,
            )
        })
        .collect::<Vec<_>>();

    let (create_transactions, update_instructions) =
        build_create_and_update_instructions(&client, &stake_history_entries)
            .await
            .map_err(|e| (e.into(), CreateUpdateStats::default()))?;

    submit_create_and_update(&client, create_transactions, update_instructions, &keypair)
        .await
        .map_err(|(e, stats)| (e.into(), stats))
}

/*
    Utility to recompute the superminority and rank fields for all validators from start_epoch to end_epoch.
    Will over-write the on-chain data, so should only be used when the on-chain data is corrupted.
*/
pub async fn _recompute_superminority_and_rank(
    client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    program_id: &Pubkey,
    start_epoch: u64,
    end_epoch: u64,
) -> Result<(), (StakeHistoryError, SubmitStats)> {
    // Fetch every ValidatorHistory account
    let gpa_config = RpcProgramAccountsConfig {
        filters: Some(vec![RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            0,
            ValidatorHistory::discriminator().into(),
        ))]),
        account_config: RpcAccountInfoConfig {
            encoding: Some(solana_account_decoder::UiAccountEncoding::Base64),
            ..RpcAccountInfoConfig::default()
        },
        ..RpcProgramAccountsConfig::default()
    };
    let validator_history_accounts = client
        .get_program_accounts_with_config(&validator_history::id(), gpa_config)
        .await
        .expect("Failed to get validator history accounts");

    let validator_histories = validator_history_accounts
        .iter()
        .map(|(_, account)| {
            let validator_history = ValidatorHistory::try_deserialize(&mut account.data.as_slice())
                .expect("Failed to deserialize validator history account");
            validator_history
        })
        .collect::<Vec<_>>();

    for epoch in start_epoch..=end_epoch {
        // Get entry for each validator for this epoch
        let vote_accounts: Vec<RpcVoteAccountInfo> = validator_histories
            .iter()
            .filter_map(|validator| {
                validator
                    .history
                    .arr
                    .iter()
                    .find(|entry| {
                        entry.epoch == epoch as u16 && entry.activated_stake_lamports != u64::MAX
                    })
                    .map(|entry| {
                        // All values except vote_pubkey and activated_stake are unused
                        RpcVoteAccountInfo {
                            vote_pubkey: validator.vote_account.to_string(),
                            activated_stake: entry.activated_stake_lamports,
                            epoch_credits: vec![],
                            commission: 0,
                            root_slot: 0,
                            node_pubkey: "".to_string(),
                            epoch_vote_account: false,
                            last_vote: 0,
                        }
                    })
            })
            .collect();
        let (stake_rank_map, superminority_threshold) =
            get_stake_rank_map_and_superminority_count(&vote_accounts);

        let stake_history_entries = vote_accounts
            .iter()
            .map(|va| {
                let rank = stake_rank_map[&va.vote_pubkey.clone()];
                let is_superminority = rank <= superminority_threshold;
                StakeHistoryEntry::new(
                    va,
                    program_id,
                    &keypair.pubkey(),
                    epoch,
                    rank,
                    is_superminority,
                )
            })
            .collect::<Vec<_>>();

        let update_instructions = stake_history_entries
            .iter()
            .map(|entry| entry.update_instruction())
            .collect::<Vec<_>>();

        match submit_instructions(&client, update_instructions, &keypair).await {
            Ok(_) => println!("completed epoch {}", epoch),
            Err((e, stats)) => return Err((e.into(), stats)),
        };
    }

    Ok(())
}

pub fn emit_stake_history_datapoint(stats: CreateUpdateStats, runs_for_epoch: i64) {
    datapoint_info!(
        "stake-history-stats",
        ("num_creates_success", stats.creates.successes, i64),
        ("num_creates_error", stats.creates.errors, i64),
        ("num_updates_success", stats.updates.successes, i64),
        ("num_updates_error", stats.updates.errors, i64),
        ("runs_for_epoch", runs_for_epoch, i64),
    );
}
