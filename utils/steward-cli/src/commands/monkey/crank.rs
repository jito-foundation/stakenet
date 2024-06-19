use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use jito_steward::StewardStateEnum;
use keeper_core::{MultipleAccountsError, SubmitStats, TransactionExecutionError};
use solana_client::{client_error::ClientError, nonblocking::rpc_client::RpcClient};
use solana_program::instruction::Instruction;
use thiserror::Error as ThisError;

use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair},
    signer::Signer,
    transaction::Transaction,
};

use crate::{
    commands::command_args::CrankMonkey,
    utils::{
        accounts::{
            get_all_steward_accounts, get_cluster_history_address, get_steward_state_account,
            get_validator_history_address, UsefulStewardAccounts,
        },
        transactions::{configure_instruction, package_instructions, submit_packaged_transactions},
    },
};

#[derive(ThisError, Debug)]
pub enum MonkeyCrankError {
    #[error(transparent)]
    ClientError(#[from] ClientError),
    #[error(transparent)]
    TransactionExecutionError(#[from] TransactionExecutionError),
    #[error(transparent)]
    MultipleAccountsError(#[from] MultipleAccountsError),
    #[error("Custom: {0}")]
    Custom(String),
}

async fn _handle_epoch_maintenance(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    epoch: u64,
    all_steward_accounts: &Box<UsefulStewardAccounts>,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    let mut state_epoch = all_steward_accounts.state_account.state.current_epoch;
    let mut num_validators = all_steward_accounts.state_account.state.num_pool_validators;
    let mut validators_to_remove = all_steward_accounts
        .state_account
        .state
        .validators_to_remove;

    let mut stats = SubmitStats::default();

    while state_epoch != epoch {
        let mut validator_index_to_remove = None;
        for i in 0..num_validators {
            if validators_to_remove.get(i).map_err(|e| {
                MonkeyCrankError::Custom(format!(
                    "Error fetching bitmask index for removed validator: {}/{} - {}",
                    i,
                    num_validators,
                    e.to_string()
                ))
            })? {
                validator_index_to_remove = Some(i);
                break;
            }
        }

        let ix = Instruction {
            program_id: *program_id,
            accounts: jito_steward::accounts::EpochMaintenance {
                config: all_steward_accounts.config_address,
                state_account: all_steward_accounts.state_address,
                validator_list: all_steward_accounts.validator_list_address,
                stake_pool: all_steward_accounts.stake_pool_address,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::EpochMaintenance {
                validator_index_to_remove,
            }
            .data(),
        };

        let configured_ix = configure_instruction(&[ix], priority_fee, None, None);

        let new_stats =
            submit_packaged_transactions(client, vec![configured_ix], &payer, None, None).await?;

        stats.combine(&new_stats);

        // NOTE: This is the only time an account is fetched
        // in any of these cranking functions
        let updated_state_account =
            get_steward_state_account(client, &program_id, &all_steward_accounts.config_address)
                .await
                .unwrap();

        num_validators = updated_state_account.state.num_pool_validators;
        validators_to_remove = updated_state_account.state.validators_to_remove;
        state_epoch = updated_state_account.state.current_epoch;
    }

    Ok(stats)
}

async fn _handle_compute_score(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &Box<UsefulStewardAccounts>,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    let validator_history_program_id = validator_history::id();
    let cluster_history: Pubkey = get_cluster_history_address(&validator_history_program_id);

    let validators_to_run = (0..all_steward_accounts.state_account.state.num_pool_validators)
        .filter_map(|validator_index| {
            let has_been_scored = all_steward_accounts
                .state_account
                .state
                .progress
                .get(validator_index)
                .expect("Index is not in progress bitmask");
            if has_been_scored {
                None
            } else {
                let vote_account = all_steward_accounts.validator_list_account.validators
                    [validator_index]
                    .vote_account_address;
                let history_account =
                    get_validator_history_address(&vote_account, &validator_history_program_id);

                Some((validator_index, vote_account, history_account))
            }
        })
        .collect::<Vec<(usize, Pubkey, Pubkey)>>();

    let ixs_to_run = validators_to_run
        .iter()
        .map(|(validator_index, _, history_account)| Instruction {
            program_id: *program_id,
            accounts: jito_steward::accounts::ComputeScore {
                config: all_steward_accounts.config_address,
                state_account: all_steward_accounts.state_address,
                validator_history: *history_account,
                validator_list: all_steward_accounts.validator_list_address,
                cluster_history,
                signer: payer.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ComputeScore {
                validator_list_index: *validator_index,
            }
            .data(),
        })
        .collect::<Vec<Instruction>>();

    let txs_to_run = package_instructions(&ixs_to_run, 11, priority_fee, Some(1_400_000), None);

    println!("Submitting {} instructions", ixs_to_run.len());
    println!("Submitting {} transactions", txs_to_run.len());

    let stats = submit_packaged_transactions(client, txs_to_run, &payer, None, None).await?;

    Ok(stats)
}

async fn _handle_compute_delegations(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &Box<UsefulStewardAccounts>,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    let ix = Instruction {
        program_id: *program_id,
        accounts: jito_steward::accounts::ComputeDelegations {
            config: all_steward_accounts.config_address,
            state_account: all_steward_accounts.state_address,
            signer: payer.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::ComputeDelegations {}.data(),
    };

    let configured_ix = configure_instruction(&[ix], priority_fee, None, None);

    let stats =
        submit_packaged_transactions(client, vec![configured_ix], &payer, None, None).await?;

    Ok(stats)
}

async fn _handle_idle(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &Box<UsefulStewardAccounts>,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    let ix = Instruction {
        program_id: *program_id,
        accounts: jito_steward::accounts::Idle {
            config: all_steward_accounts.config_address,
            state_account: all_steward_accounts.state_address,
            signer: payer.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::Idle {}.data(),
    };

    let configured_ix = configure_instruction(&[ix], priority_fee, None, None);

    let stats =
        submit_packaged_transactions(client, vec![configured_ix], &payer, None, None).await?;

    Ok(stats)
}

async fn _handle_compute_instant_unstake(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &Box<UsefulStewardAccounts>,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    // println!("IDLE: {:?}", signature);

    Ok(SubmitStats::default())
}

async fn _handle_rebalance(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &Box<UsefulStewardAccounts>,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    // println!("IDLE: {:?}", signature);

    Ok(SubmitStats::default())
}

pub async fn crank_monkey(
    client: &Arc<RpcClient>,
    payer: &Arc<Keypair>,
    program_id: &Pubkey,
    epoch: u64,
    all_steward_accounts: &Box<UsefulStewardAccounts>,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    {
        // --------- CHECK AND HANDLE EPOCH BOUNDARY -----------

        if all_steward_accounts.state_account.state.current_epoch != epoch {
            return _handle_epoch_maintenance(
                &payer,
                client,
                &program_id,
                epoch,
                &all_steward_accounts,
                priority_fee,
            )
            .await;
        }
    }

    {
        // --------- CHECK AND HANDLE STATE -----------

        // State
        match all_steward_accounts.state_account.state.state_tag {
            StewardStateEnum::ComputeScores => {
                return _handle_compute_score(
                    &payer,
                    client,
                    &program_id,
                    &all_steward_accounts,
                    priority_fee,
                )
                .await
            }
            StewardStateEnum::ComputeDelegations => {
                return _handle_compute_delegations(
                    &payer,
                    client,
                    &program_id,
                    &all_steward_accounts,
                    priority_fee,
                )
                .await
            }
            StewardStateEnum::Idle => {
                return _handle_idle(
                    &payer,
                    client,
                    &program_id,
                    &all_steward_accounts,
                    priority_fee,
                )
                .await
            }
            StewardStateEnum::ComputeInstantUnstake => {
                return _handle_compute_score(
                    &payer,
                    client,
                    &program_id,
                    &all_steward_accounts,
                    priority_fee,
                )
                .await
            }
            StewardStateEnum::Rebalance => {
                return _handle_rebalance(
                    &payer,
                    client,
                    &program_id,
                    &all_steward_accounts,
                    priority_fee,
                )
                .await
            }
        };
    }
}

// Only runs one set of commands per "crank"
pub async fn command_crank_monkey(
    args: CrankMonkey,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<(), anyhow::Error> {
    // ----------- Collect Accounts -------------
    let steward_config = args.permissionless_parameters.steward_config;
    let payer = Arc::new(
        read_keypair_file(args.permissionless_parameters.payer_keypair_path)
            .expect("Failed reading keypair file ( Payer )"),
    );

    let priority_fee = args
        .permissionless_parameters
        .transaction_parameters
        .priority_fee;

    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let epoch = client.get_epoch_info().await?.epoch;

    crank_monkey(
        client,
        &payer,
        &program_id,
        epoch,
        &all_steward_accounts,
        priority_fee,
    )
    .await?;

    Ok(())
}
