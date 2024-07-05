use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use jito_steward::StewardStateEnum;
use keeper_core::{
    get_vote_accounts_with_retry, MultipleAccountsError, SendTransactionError, SubmitStats, TransactionExecutionError
};
use solana_client::{client_error::ClientError, nonblocking::rpc_client::RpcClient};
use solana_program::instruction::Instruction;

use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signature::Keypair, stake, system_program,
};
use thiserror::Error as ThisError;

use crate::utils::accounts::{
    check_stake_accounts, get_all_steward_validator_accounts, get_all_validator_accounts, get_unprogressed_validators, AllValidatorAccounts
};
use crate::utils::transactions::print_errors_if_any;
use crate::{
    commands::command_args::CrankMonkey,
    utils::{
        accounts::{
            get_all_steward_accounts, get_cluster_history_address, get_stake_address,
            get_steward_state_account, get_transient_stake_address, get_validator_history_address,
            AllStewardAccounts,
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

async fn _handle_adding_validators(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    epoch: u64,
    all_steward_accounts: &AllStewardAccounts,
    all_validator_accounts: &AllValidatorAccounts,
    all_vote_accounts: &AllValidatorAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {

    let mut keys_to_add: Vec<&Pubkey> = vec![];
    all_vote_accounts.all_history_vote_account_map.keys().for_each(|key|{
        if all_validator_accounts.all_history_vote_account_map.keys().find(|k|
            *k == key
        ).is_none() {
            keys_to_add.push(key);
        }
    });

    let mut accounts_to_check: AllValidatorAccounts = AllValidatorAccounts::default(); 
    all_vote_accounts.all_history_vote_account_map.keys().for_each(|key|{
        if keys_to_add.contains(&key) {
            accounts_to_check.all_history_vote_account_map.insert(*key, all_vote_accounts.all_history_vote_account_map.get(key).unwrap().clone());
            accounts_to_check.all_stake_account_map.insert(*key, all_vote_accounts.all_stake_account_map.get(key).unwrap().clone());
        }
    });

    let checks = check_stake_accounts(&accounts_to_check, epoch);

    let good_vote_accounts = checks
        .iter()
        .filter_map(|(vote_account, check)| {

            if check.has_history && !check.has_stake_account {
                Some(*vote_account)
            } else {
                None
            }
        })
        .collect::<Vec<Pubkey>>();

        let ixs_to_run = good_vote_accounts
        .iter()
        .filter_map(|vote_account| {
                        let history_account =
                get_validator_history_address(vote_account, &validator_history::id());

            let stake_address =
                get_stake_address(vote_account, &all_steward_accounts.stake_pool_address);

            Some(Instruction {
                program_id: *program_id,
                accounts: jito_steward::accounts::AutoAddValidator {
                    config: all_steward_accounts.config_address,
                    steward_state: all_steward_accounts.state_address,
                    stake_pool_program: spl_stake_pool::id(),
                    stake_pool: all_steward_accounts.stake_pool_address,
                    validator_history_account: history_account,
                    withdraw_authority: all_steward_accounts.stake_pool_withdraw_authority,
                    validator_list: all_steward_accounts.validator_list_address,
                    reserve_stake: all_steward_accounts.stake_pool_account.reserve_stake,
                    stake_account: stake_address,
                    vote_account: *vote_account,
                    system_program: system_program::id(),
                    stake_program: stake::program::id(),
                    rent: solana_sdk::sysvar::rent::id(),
                    clock: solana_sdk::sysvar::clock::id(),
                    stake_history: solana_sdk::sysvar::stake_history::id(),
                    stake_config: stake::config::ID,
                }
                .to_account_metas(None),
                data: jito_steward::instruction::AutoAddValidatorToPool {
                }
                .data(),
            })
        })
        .collect::<Vec<Instruction>>();

    let txs_to_run = package_instructions(&ixs_to_run, 1, priority_fee, Some(1_400_000), None);

    println!("Submitting {} instructions", ixs_to_run.len());
    println!("Submitting {} transactions", txs_to_run.len());

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(10), None).await?;
    // let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(1), None).await?;

    Ok(stats) 

}


async fn _handle_delinquent_validators(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    epoch: u64,
    all_steward_accounts: &AllStewardAccounts,
    all_validator_accounts: &AllValidatorAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    let checks = check_stake_accounts(all_validator_accounts, epoch);

    let bad_vote_accounts = checks
        .iter()
        .filter_map(|(vote_account, check)| {
            if !check.has_history || check.is_deactivated {
                Some(*vote_account)
            } else {
                None
            }
        })
        .collect::<Vec<Pubkey>>();

    let ixs_to_run = bad_vote_accounts
        .iter()
        .filter_map(|vote_account| {
            let validator_index = all_steward_accounts
                .validator_list_account
                .validators
                .iter()
                .position(|v| v.vote_account_address == *vote_account)
                .expect("Cannot find vote account in Validator List");

            let history_account =
                get_validator_history_address(vote_account, &validator_history::id());

            let stake_address =
                get_stake_address(vote_account, &all_steward_accounts.stake_pool_address);

            let transient_stake_address = get_transient_stake_address(
                vote_account,
                &all_steward_accounts.stake_pool_address,
                &all_steward_accounts.validator_list_account,
                validator_index,
            );

            if all_steward_accounts
                .state_account
                .state
                .validators_to_remove
                .get(validator_index)
                .expect("Could not find validator index in validators_to_remove")
            {
                return None;
            }

            Some(Instruction {
                program_id: *program_id,
                accounts: jito_steward::accounts::AutoRemoveValidator {
                    config: all_steward_accounts.config_address,
                    state_account: all_steward_accounts.state_address,
                    stake_pool_program: spl_stake_pool::id(),
                    stake_pool: all_steward_accounts.stake_pool_address,
                    validator_history_account: history_account,
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
                }
                .to_account_metas(None),
                data: jito_steward::instruction::AutoRemoveValidatorFromPool {
                    validator_list_index: validator_index as u64,
                }
                .data(),
            })
        })
        .collect::<Vec<Instruction>>();

    let txs_to_run = package_instructions(&ixs_to_run, 1, priority_fee, Some(1_400_000), None);

    println!("Submitting {} instructions", ixs_to_run.len());
    println!("Submitting {} transactions", txs_to_run.len());

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(10), None).await?;
    // let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(1), None).await?;

    Ok(stats)
}

async fn _handle_epoch_maintenance(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    epoch: u64,
    all_steward_accounts: &AllStewardAccounts,
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
            if validators_to_remove.get(i as usize).map_err(|e| {
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

        let cu = validator_index_to_remove.map(|_| 1_400_000);
        let configured_ix = configure_instruction(&[ix], priority_fee, cu, None);

        println!("Submitting Epoch Maintenance");
        let new_stats =
            submit_packaged_transactions(client, vec![configured_ix], payer, Some(10), None).await?;

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
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    let validator_history_program_id = validator_history::id();
    let cluster_history: Pubkey = get_cluster_history_address(&validator_history_program_id);

    let validators_to_run =
        get_unprogressed_validators(all_steward_accounts, &validator_history_program_id);

    let ixs_to_run = validators_to_run
        .iter()
        .map(|validator_info| Instruction {
            program_id: *program_id,
            accounts: jito_steward::accounts::ComputeScore {
                config: all_steward_accounts.config_address,
                state_account: all_steward_accounts.state_address,
                validator_history: validator_info.history_account,
                validator_list: all_steward_accounts.validator_list_address,
                cluster_history,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ComputeScore {
                validator_list_index: validator_info.index as u64,
            }
            .data(),
        })
        .collect::<Vec<Instruction>>();

    let txs_to_run = package_instructions(&ixs_to_run, 10, priority_fee, Some(1_400_000), None);

    println!("Submitting {} instructions", ixs_to_run.len());
    println!("Submitting {} transactions", txs_to_run.len());

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(10), None).await?;

    Ok(stats)
}

async fn _handle_compute_delegations(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    let ix = Instruction {
        program_id: *program_id,
        accounts: jito_steward::accounts::ComputeDelegations {
            config: all_steward_accounts.config_address,
            state_account: all_steward_accounts.state_address,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::ComputeDelegations {}.data(),
    };

    let configured_ix = configure_instruction(&[ix], priority_fee, None, None);

    let stats =
        submit_packaged_transactions(client, vec![configured_ix], payer, Some(10), None).await?;

    Ok(stats)
}

async fn _handle_idle(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    let ix = Instruction {
        program_id: *program_id,
        accounts: jito_steward::accounts::Idle {
            config: all_steward_accounts.config_address,
            state_account: all_steward_accounts.state_address,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::Idle {}.data(),
    };

    let configured_ix = configure_instruction(&[ix], priority_fee, None, None);

    let stats =
        submit_packaged_transactions(client, vec![configured_ix], payer, Some(10), None).await?;

    Ok(stats)
}

async fn _handle_compute_instant_unstake(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    let validator_history_program_id = validator_history::id();
    let cluster_history: Pubkey = get_cluster_history_address(&validator_history_program_id);

    let validators_to_run =
        get_unprogressed_validators(all_steward_accounts, &validator_history_program_id);

    let ixs_to_run = validators_to_run
        .iter()
        .map(|validator_info| Instruction {
            program_id: *program_id,
            accounts: jito_steward::accounts::ComputeInstantUnstake {
                config: all_steward_accounts.config_address,
                state_account: all_steward_accounts.state_address,
                validator_history: validator_info.history_account,
                validator_list: all_steward_accounts.validator_list_address,
                cluster_history,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ComputeInstantUnstake {
                validator_list_index: validator_info.index as u64,
            }
            .data(),
        })
        .collect::<Vec<Instruction>>();

    // let test_tx =
    //     debug_send_single_transaction(client, payer, &[ixs_to_run[0].clone()], Some(true)).await?;

    let txs_to_run = package_instructions(&ixs_to_run, 1, priority_fee, Some(1_400_000), None);

    println!("Submitting {} instructions", ixs_to_run.len());
    println!("Submitting {} transactions", txs_to_run.len());

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(10), None).await?;

    Ok(stats)
}

async fn _handle_rebalance(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    let validator_history_program_id = validator_history::id();

    let validators_to_run =
        get_unprogressed_validators(all_steward_accounts, &validator_history_program_id);

    let ixs_to_run = validators_to_run
        .iter()
        .map(|validator_info| {
            let validator_index = validator_info.index;
            let vote_account = &validator_info.vote_account;
            let history_account = validator_info.history_account;

            let stake_address =
                get_stake_address(vote_account, &all_steward_accounts.stake_pool_address);

            let transient_stake_address = get_transient_stake_address(
                vote_account,
                &all_steward_accounts.stake_pool_address,
                &all_steward_accounts.validator_list_account,
                validator_index,
            );

            Instruction {
                program_id: *program_id,
                accounts: jito_steward::accounts::Rebalance {
                    config: all_steward_accounts.config_address,
                    state_account: all_steward_accounts.state_address,
                    validator_history: history_account,
                    stake_pool_program: spl_stake_pool::id(),
                    stake_pool: all_steward_accounts.stake_pool_address,
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
                }
                .to_account_metas(None),
                data: jito_steward::instruction::Rebalance {
                    validator_list_index: validator_index as u64,
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
    all_steward_accounts: &AllStewardAccounts,
    all_steward_validator_accounts: &AllValidatorAccounts,
    all_active_validator_accounts: &AllValidatorAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, MonkeyCrankError> {
    let mut return_stats = SubmitStats::default();
    let should_run_epoch_maintenance =
        all_steward_accounts.state_account.state.current_epoch != epoch;
    let should_crank_state = !should_run_epoch_maintenance;

    {
        // --------- CHECK AND HANDLE EPOCH BOUNDARY -----------

        if should_run_epoch_maintenance {
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
        // --------- CHECK VALIDATORS TO ADD -----------
        println!("Adding good validators...");
        // Any validator that has new history account
        // Anything that would pass the benchmark
        // Find any validators that that are not in pool
        let stats = _handle_adding_validators(
            payer,
            client,
            program_id,
            epoch,
            all_steward_accounts,
            all_steward_validator_accounts,
            all_active_validator_accounts,
            priority_fee,
        ).await?;

        return_stats.combine(&stats);
    }

    {
        // --------- CHECK VALIDATORS TO REMOVE -----------
        println!("Finding and Removing Bad Validators...");

        let stats = _handle_delinquent_validators(
            payer,
            client,
            program_id,
            epoch,
            all_steward_accounts,
            all_steward_validator_accounts,
            priority_fee,
        )
        .await?;

        return_stats.combine(&stats);
    }

    {
        // --------- CHECK AND HANDLE STATE -----------
        if should_crank_state {
            let stats = match all_steward_accounts.state_account.state.state_tag {
                StewardStateEnum::ComputeScores => {
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

    {
        // --------- RECOVER FROM ERROR -----------
        return_stats.results.iter().for_each(|result| {
            if let Err(error) = result {
                // Access and print the error
                match error {
                    SendTransactionError::ExceededRetries => {
                        // Continue
                        println!("Exceeded Retries: {:?}", error);
                    }
                    SendTransactionError::TransactionError(e) => {
                        // Flag
                        println!("Transaction: {:?}", e);
                    }
                    SendTransactionError::RpcSimulateTransactionResult(e) => {
                        // Recover
                        println!("\n\nERROR: ");
                        e.logs.iter().for_each(|log| {
                            log.iter().enumerate().for_each(|(i, log)| {
                                println!("{}: {:?}", i, log);
                            });
                        });
                    }
                }
            }
        });
    }

    Ok(return_stats)
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

    let all_steward_validator_accounts =
        get_all_steward_validator_accounts(client, &all_steward_accounts, &validator_history::id())
            .await?;


    let all_active_vote_accounts = get_vote_accounts_with_retry(client, 5, None).await?;

    let all_active_validator_accounts = get_all_validator_accounts(client, &all_active_vote_accounts, &validator_history::id()).await?;

    let epoch = client.get_epoch_info().await?.epoch;

    let _ = crank_monkey(
        client,
        &payer,
        &program_id,
        epoch,
        &all_steward_accounts,
        &all_steward_validator_accounts,
        &all_active_validator_accounts,
        priority_fee,
    )
    .await?;

    Ok(())
}

// Notes on handling errors
// Try lowering the number of instructions per transaction
//
