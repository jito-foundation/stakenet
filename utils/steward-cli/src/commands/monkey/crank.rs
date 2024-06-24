use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use jito_steward::StewardStateEnum;
use keeper_core::{
    MultipleAccountsError, SendTransactionError, SubmitStats, TransactionExecutionError,
};
use solana_client::{client_error::ClientError, nonblocking::rpc_client::RpcClient};
use solana_program::instruction::Instruction;
use solana_sdk::transaction::Transaction;
use std::fs::{File, OpenOptions};
use std::io::prelude::*;
use std::io::Error;

use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signature::Keypair, signer::Signer, stake,
    system_program,
};
use thiserror::Error as ThisError;

use crate::commands::info::view_state::format_state;
use crate::utils::transactions::print_errors_if_any;
use crate::{
    commands::command_args::CrankMonkey,
    utils::{
        accounts::{
            get_all_steward_accounts, get_cluster_history_address, get_stake_address,
            get_steward_state_account, get_transient_stake_address, get_validator_history_address,
            UsefulStewardAccounts,
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
    all_steward_accounts: &UsefulStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    let mut current_epoch = epoch;
    let mut state_epoch = all_steward_accounts.state_account.state.current_epoch;
    let mut num_validators = all_steward_accounts.state_account.state.num_pool_validators;
    let mut validators_to_remove = all_steward_accounts
        .state_account
        .state
        .validators_to_remove;

    let mut stats = SubmitStats::default();

    while state_epoch != current_epoch {
        let mut validator_index_to_remove = None;
        for i in 0..num_validators {
            if validators_to_remove.get(i).map_err(|e| {
                MonkeyCrankError::Custom(format!(
                    "Error fetching bitmask index for removed validator: {}/{} - {}",
                    i, num_validators, e
                ))
            })? {
                validator_index_to_remove = Some(i);
                break;
            }
        }

        println!("Validator Index to Remove: {:?}", validator_index_to_remove);

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

        let cu = match validator_index_to_remove {
            Some(_) => Some(1_400_000),
            None => None,
        };
        let configured_ix = configure_instruction(&[ix], priority_fee, cu, None);

        // let blockhash = client
        //     .get_latest_blockhash()
        //     .await
        //     .expect("Failed to get recent blockhash");
        // let transaction = Transaction::new_signed_with_payer(
        //     &configured_ix,
        //     Some(&payer.pubkey()),
        //     &[&payer],
        //     blockhash,
        // );

        // let signature = client
        //     .send_and_confirm_transaction_with_spinner(&transaction)
        //     .await
        //     .expect("Failed to send transaction");

        // println!("Signature: {}", signature);

        println!("Submitting Epoch Maintenance");
        let new_stats =
            submit_packaged_transactions(client, vec![configured_ix], payer, None, None).await?;

        stats.combine(&new_stats);
        print_errors_if_any(&stats);

        if stats.errors > 0 {
            return Ok(stats);
        }

        // NOTE: This is the only time an account is fetched
        // in any of these cranking functions
        let updated_state_account =
            get_steward_state_account(client, program_id, &all_steward_accounts.config_address)
                .await
                .unwrap();

        num_validators = updated_state_account.state.num_pool_validators;
        validators_to_remove = updated_state_account.state.validators_to_remove;
        state_epoch = updated_state_account.state.current_epoch;
        current_epoch = client.get_epoch_info().await?.epoch;

        println!(
            "State Epoch: {} | Current Epoch: {}",
            state_epoch, current_epoch
        );
    }

    Ok(stats)
}

async fn _handle_compute_score(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &UsefulStewardAccounts,
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

    let txs_to_run = package_instructions(&ixs_to_run, 10, priority_fee, Some(1_400_000), None);

    println!("Submitting {} instructions", ixs_to_run.len());
    println!("Submitting {} transactions", txs_to_run.len());

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(1), None).await?;

    Ok(stats)
}

async fn _handle_compute_delegations(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &UsefulStewardAccounts,
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
        submit_packaged_transactions(client, vec![configured_ix], payer, None, None).await?;

    Ok(stats)
}

async fn _handle_idle(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &UsefulStewardAccounts,
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
        submit_packaged_transactions(client, vec![configured_ix], payer, None, None).await?;

    Ok(stats)
}

async fn _handle_compute_instant_unstake(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &UsefulStewardAccounts,
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
            accounts: jito_steward::accounts::ComputeInstantUnstake {
                config: all_steward_accounts.config_address,
                state_account: all_steward_accounts.state_address,
                validator_history: *history_account,
                validator_list: all_steward_accounts.validator_list_address,
                cluster_history,
                signer: payer.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ComputeInstantUnstake {
                validator_list_index: *validator_index,
            }
            .data(),
        })
        .collect::<Vec<Instruction>>();

    let txs_to_run = package_instructions(&ixs_to_run, 1, priority_fee, Some(1_400_000), None);

    println!("Submitting {} instructions", ixs_to_run.len());
    println!("Submitting {} transactions", txs_to_run.len());

    let stats = submit_packaged_transactions(client, txs_to_run, payer, None, None).await?;

    Ok(stats)
}

async fn _handle_rebalance(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &UsefulStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    let validator_history_program_id = validator_history::id();

    let validators_to_run = (0..all_steward_accounts.state_account.state.num_pool_validators)
        .filter_map(|validator_index| {
            let has_been_rebalanced = all_steward_accounts
                .state_account
                .state
                .progress
                .get(validator_index)
                .expect("Index is not in progress bitmask");
            if has_been_rebalanced {
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
        .map(|(validator_index, vote_account, history_account)| {
            let stake_address =
                get_stake_address(vote_account, &all_steward_accounts.stake_pool_address);

            let transient_stake_address = get_transient_stake_address(
                vote_account,
                &all_steward_accounts.stake_pool_address,
                &all_steward_accounts.validator_list_account,
                *validator_index,
            );

            Instruction {
                program_id: *program_id,
                accounts: jito_steward::accounts::Rebalance {
                    config: all_steward_accounts.config_address,
                    state_account: all_steward_accounts.state_address,
                    validator_history: *history_account,
                    stake_pool_program: spl_stake_pool::id(),
                    stake_pool: all_steward_accounts.stake_pool_address,
                    staker: all_steward_accounts.staker_address,
                    withdraw_authority: all_steward_accounts.stake_pool_withdraw_authority,
                    validator_list: all_steward_accounts.validator_list_address,
                    reserve_stake: all_steward_accounts.stake_pool_account.reserve_stake,
                    stake_account: stake_address,
                    transient_stake_account: transient_stake_address,
                    vote_account: *vote_account,
                    system_program: system_program::id(),
                    stake_program: stake::program::id(),
                    rent: solana_sdk::sysvar::rent::id(),
                    clock: solana_sdk::sysvar::clock::id(),
                    stake_history: solana_sdk::sysvar::stake_history::id(),
                    stake_config: stake::config::ID,
                    signer: payer.pubkey(),
                }
                .to_account_metas(None),
                data: jito_steward::instruction::Rebalance {
                    validator_list_index: *validator_index,
                }
                .data(),
            }
        })
        .collect::<Vec<Instruction>>();

    let txs_to_run = package_instructions(&ixs_to_run, 1, priority_fee, Some(1_400_000), None);

    println!("Submitting {} instructions", ixs_to_run.len());
    println!("Submitting {} transactions", txs_to_run.len());

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(30), None).await?;

    Ok(stats)
}

pub async fn crank_monkey(
    client: &Arc<RpcClient>,
    payer: &Arc<Keypair>,
    program_id: &Pubkey,
    epoch: u64,
    all_steward_accounts: &UsefulStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    let mut return_stats = SubmitStats::default();
    let should_run_epoch_maintenance =
        all_steward_accounts.state_account.state.current_epoch != epoch;
    let should_crank_state = !should_run_epoch_maintenance;

    let mut log_string: String = "\n--------- NEW LOG ---------\n".to_string();
    log_string += &format_state(
        &all_steward_accounts.config_address,
        &all_steward_accounts.state_address,
        &all_steward_accounts.state_account,
    );
    log_string += "\n";

    {
        // --------- CHECK AND HANDLE EPOCH BOUNDARY -----------

        if should_run_epoch_maintenance {
            log_string += "Cranking Epoch Maintenance\n";
            println!("Cranking Epoch Maintenance...");

            let stats = _handle_epoch_maintenance(
                payer,
                client,
                program_id,
                epoch,
                all_steward_accounts,
                priority_fee,
            )
            .await?;

            return_stats.combine(&stats);
        }
    }

    {
        // --------- CHECK AND HANDLE STATE -----------

        if should_crank_state {
            let stats = match all_steward_accounts.state_account.state.state_tag {
                StewardStateEnum::ComputeScores => {
                    log_string += "Cranking Compute Score\n";
                    println!("Cranking Compute Score...");

                    _handle_compute_score(
                        payer,
                        client,
                        program_id,
                        all_steward_accounts,
                        priority_fee,
                    )
                    .await?
                }
                StewardStateEnum::ComputeDelegations => {
                    log_string += "Cranking Compute Delegations\n";
                    println!("Cranking Compute Delegations...");

                    _handle_compute_delegations(
                        payer,
                        client,
                        program_id,
                        all_steward_accounts,
                        priority_fee,
                    )
                    .await?
                }
                StewardStateEnum::Idle => {
                    log_string += "Cranking Idle\n";
                    println!("Cranking Idle...");

                    _handle_idle(
                        payer,
                        client,
                        program_id,
                        all_steward_accounts,
                        priority_fee,
                    )
                    .await?
                }
                StewardStateEnum::ComputeInstantUnstake => {
                    log_string += "Cranking Compute Instant Unstake\n";
                    println!("Cranking Compute Instant Unstake...");

                    _handle_compute_instant_unstake(
                        payer,
                        client,
                        program_id,
                        all_steward_accounts,
                        priority_fee,
                    )
                    .await?
                }
                StewardStateEnum::Rebalance => {
                    log_string += "Cranking Rebalance\n";
                    println!("Cranking Rebalance...");

                    _handle_rebalance(
                        payer,
                        client,
                        program_id,
                        all_steward_accounts,
                        priority_fee,
                    )
                    .await?
                }
            };

            return_stats.combine(&stats);
        }
    }

    log_string += &format!(
        "\nSuccesses: {}\nErrors: {:?}\n\n",
        return_stats.successes, return_stats.errors
    );

    return_stats.results.iter().for_each(|result| {
        if let Err(error) = result {
            log_string += &format!("HAS_ERROR\n");
            // Access and print the error
            match error {
                SendTransactionError::ExceededRetries => {
                    // Continue
                    log_string += &format!("Exceeded Retries: {:?}\n", error);
                    println!("Exceeded Retries: {:?}", error);
                }
                SendTransactionError::TransactionError(e) => {
                    // Flag
                    log_string += &format!("Transaction: {:?}\n", e);
                    println!("Transaction: {:?}", e);
                }
                SendTransactionError::RpcSimulateTransactionResult(e) => {
                    // Recover
                    println!("\n\nERROR: ");
                    e.logs.iter().for_each(|log| {
                        log.iter().enumerate().for_each(|(i, log)| {
                            log_string += &format!("{}: {:?}\n", i, log);
                            println!("{}: {:?}", i, log);
                        });
                    });
                }
            }
        }
    });

    {
        // Debug write to file

        let write_result = append_to_file("crank_monkey.log", &log_string);

        match write_result {
            Ok(_) => {
                println!("Wrote logging info");
            }
            Err(e) => {
                println!("Error writing to file: {:?}", e);
            }
        }
    }

    Ok(return_stats)
}

fn append_to_file(filename: &str, text: &str) -> Result<(), Error> {
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(filename)?;

    writeln!(file, "{}", text)?;
    Ok(())
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

    let _ = crank_monkey(
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

// Notes on handling errors
// Try lowering the number of instructions per transaction
//
