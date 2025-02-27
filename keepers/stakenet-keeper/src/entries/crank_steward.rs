use std::sync::Arc;

use anchor_lang::{AccountDeserialize, AnchorDeserialize, InstructionData, ToAccountMetas};
use jito_steward::utils::{StakePool, ValidatorList};
use jito_steward::StewardStateEnum;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;

use solana_sdk::stake::instruction::deactivate_delinquent_stake;
use solana_sdk::stake::state::StakeStateV2;
use solana_sdk::vote::state::VoteState;
use solana_sdk::{pubkey::Pubkey, signature::Keypair, stake, system_program};
use spl_stake_pool::instruction::{
    cleanup_removed_validator_entries, update_stake_pool_balance, update_validator_list_balance,
};
use spl_stake_pool::state::{StakeStatus, ValidatorStakeInfo};
use spl_stake_pool::{find_withdraw_authority_program_address, MAX_VALIDATORS_TO_UPDATE};
use stakenet_sdk::models::aggregate_accounts::{AllStewardAccounts, AllValidatorAccounts};
use stakenet_sdk::models::errors::{JitoSendTransactionError, JitoTransactionError};
use stakenet_sdk::models::submit_stats::SubmitStats;

use stakenet_sdk::utils::accounts::{
    get_cluster_history_address, get_stake_address, get_steward_state_account,
    get_transient_stake_address,
};
use stakenet_sdk::utils::helpers::{check_stake_accounts, get_unprogressed_validators};
use stakenet_sdk::utils::{
    accounts::get_validator_history_address,
    transactions::{
        configure_instruction, package_instructions, print_errors_if_any,
        submit_packaged_transactions,
    },
};
use validator_history::ValidatorHistory;

pub fn _get_update_stake_pool_ixs(
    program_id: &Pubkey,
    stake_pool: &StakePool,
    validator_list: &ValidatorList,
    stake_pool_address: &Pubkey,
    all_validator_accounts: &AllValidatorAccounts,
    no_merge: bool,
    epoch: u64,
) -> (Vec<Instruction>, Vec<Instruction>, Vec<Instruction>) {
    let (withdraw_authority, _) =
        find_withdraw_authority_program_address(program_id, stake_pool_address);

    let mut update_list_instructions: Vec<Instruction> = vec![];
    let mut start_index = 0;
    for validator_info_chunk in validator_list.validators.chunks(MAX_VALIDATORS_TO_UPDATE) {
        let should_update = validator_info_chunk
            .iter()
            .any(|info: &ValidatorStakeInfo| {
                if u64::from(info.last_update_epoch) < epoch {
                    true
                } else {
                    match StakeStatus::try_from(info.status).unwrap() {
                        StakeStatus::DeactivatingValidator => true,
                        _ => false,
                        // StakeStatus::DeactivatingAll => false,
                        // StakeStatus::Active => false,
                        // StakeStatus::DeactivatingTransient => false,
                        // StakeStatus::ReadyForRemoval => false,
                    }
                }
            });

        if should_update {
            let validator_vote_accounts = validator_info_chunk
                .iter()
                .map(|v| v.vote_account_address)
                .collect::<Vec<Pubkey>>();

            update_list_instructions.push(update_validator_list_balance(
                program_id,
                stake_pool_address,
                &withdraw_authority,
                &stake_pool.validator_list,
                &stake_pool.reserve_stake,
                validator_list,
                &validator_vote_accounts,
                start_index,
                no_merge,
            ));
        }
        // Advance no matter what
        start_index = start_index.saturating_add(MAX_VALIDATORS_TO_UPDATE as u32);
    }

    let mut deactivate_delinquent_instructions: Vec<Instruction> = vec![];
    let reference_vote_account = validator_list
        .validators
        .iter()
        .find(|validator_info| {
            let raw_vote_account = all_validator_accounts
                .all_vote_account_map
                .get(&validator_info.vote_account_address)
                .expect("Vote account not found");

            if raw_vote_account.is_none() {
                return false;
            }

            let vote_account = VoteState::deserialize(&raw_vote_account.clone().unwrap().data)
                .expect("Could not deserialize vote account");

            let latest_epoch = vote_account.epoch_credits.iter().last().unwrap().0;

            latest_epoch == epoch || latest_epoch == epoch - 1
        })
        .expect("Need at least one okay validator");

    for validator_info in validator_list.validators.iter() {
        let raw_vote_account = all_validator_accounts
            .all_vote_account_map
            .get(&validator_info.vote_account_address)
            .expect("Vote account not found");

        let raw_stake_account = all_validator_accounts
            .all_stake_account_map
            .get(&validator_info.vote_account_address)
            .expect("Stake account not found");

        let should_deactivate = match (raw_vote_account, raw_stake_account) {
            (None, Some(_)) => true,
            (Some(raw_vote_account), Some(raw_stake_account)) => {
                let stake_account =
                    StakeStateV2::deserialize(&mut raw_stake_account.data.as_slice())
                        .expect("Could not deserialize stake account");

                let vote_account = VoteState::deserialize(&raw_vote_account.data)
                    .expect("Could not deserialize vote account");

                if vote_account.epoch_credits.iter().last().is_none() {
                    println!(
                        "🆘 ⁉️ Error: Epoch credits has no entries? \nStake Account\n{:?}\nVote Account\n{:?}\n",
                        stake_account,
                        vote_account
                    );
                    false
                } else {
                    let latest_epoch = vote_account.epoch_credits.iter().last().unwrap().0;

                    match stake_account {
                        StakeStateV2::Stake(_meta, stake, _stake_flags) => {
                            if stake.delegation.deactivation_epoch != std::u64::MAX {
                                false
                            } else {
                                latest_epoch <= epoch - 5
                            }
                        }
                        _ => {
                            println!("🔶 Error: Stake account is not StakeStateV2::Stake");
                            false
                        }
                    }
                }
            }
            (_, None) => false,
        };

        if should_deactivate {
            let stake_account =
                get_stake_address(&validator_info.vote_account_address, stake_pool_address);

            let ix = deactivate_delinquent_stake(
                &stake_account,
                &validator_info.vote_account_address,
                &reference_vote_account.vote_account_address,
            );

            deactivate_delinquent_instructions.push(ix);
        }
    }

    let final_instructions = vec![
        update_stake_pool_balance(
            program_id,
            stake_pool_address,
            &withdraw_authority,
            &stake_pool.validator_list,
            &stake_pool.reserve_stake,
            &stake_pool.manager_fee_account,
            &stake_pool.pool_mint,
            &stake_pool.token_program_id,
        ),
        cleanup_removed_validator_entries(
            program_id,
            stake_pool_address,
            &stake_pool.validator_list,
        ),
    ];
    (
        update_list_instructions,
        deactivate_delinquent_instructions,
        final_instructions,
    )
}

async fn _update_pool(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    epoch: u64,
    all_steward_accounts: &AllStewardAccounts,
    all_validator_accounts: &AllValidatorAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let mut stats = SubmitStats::default();

    let (update_ixs, deactivate_delinquent_ixs, cleanup_ixs) = _get_update_stake_pool_ixs(
        &spl_stake_pool::ID,
        &all_steward_accounts.stake_pool_account,
        &all_steward_accounts.validator_list_account,
        &all_steward_accounts.stake_pool_address,
        all_validator_accounts,
        false,
        epoch,
    );

    println!("Updating Pool");
    let update_txs_to_run =
        package_instructions(&update_ixs, 1, priority_fee, Some(1_400_000), None);
    let update_stats =
        submit_packaged_transactions(client, update_txs_to_run, payer, Some(50), None).await?;

    stats.combine(&update_stats);

    // TODO fix
    println!("Deactivating Delinquent");
    // for ix in deactivate_delinquent_ixs {
    //     let tx = Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[&payer]);
    //     let tx = client
    //         .send_and_confirm_transaction_with_spinner_and_config(&tx)
    //         .await?;
    //     stats.add_tx(&tx);
    // }
    let deactivate_txs_to_run = package_instructions(
        &deactivate_delinquent_ixs,
        1,
        priority_fee,
        Some(1_400_000),
        None,
    );
    let update_stats =
        submit_packaged_transactions(client, deactivate_txs_to_run, payer, Some(50), None).await?;

    stats.combine(&update_stats);

    println!("Cleaning Pool");
    let cleanup_txs_to_run =
        package_instructions(&cleanup_ixs, 1, priority_fee, Some(1_400_000), None);
    let cleanup_stats =
        submit_packaged_transactions(client, cleanup_txs_to_run, payer, Some(50), None).await?;

    stats.combine(&cleanup_stats);

    Ok(stats)
}

async fn _handle_instant_removal_validators(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let mut num_validators = all_steward_accounts.state_account.state.num_pool_validators;
    let mut validators_to_remove = all_steward_accounts
        .state_account
        .state
        .validators_for_immediate_removal;

    let mut stats = SubmitStats::default();

    while validators_to_remove.count() != 0 {
        let mut validator_index_to_remove = None;
        for i in 0..all_steward_accounts.validator_list_account.validators.len() as u64 {
            if validators_to_remove.get(i as usize).map_err(|e| {
                JitoTransactionError::Custom(format!(
                    "Error fetching bitmask index for immediate removed validator: {}/{} - {}",
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
            accounts: jito_steward::accounts::InstantRemoveValidator {
                config: all_steward_accounts.config_address,
                state_account: all_steward_accounts.state_address,
                validator_list: all_steward_accounts.validator_list_address,
                stake_pool: all_steward_accounts.stake_pool_address,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::InstantRemoveValidator {
                validator_index_to_remove: validator_index_to_remove.unwrap(),
            }
            .data(),
        };

        let configured_ix = configure_instruction(&[ix], priority_fee, Some(1_400_000), None);

        println!("Submitting Instant Removal");
        let new_stats =
            submit_packaged_transactions(client, vec![configured_ix], payer, Some(50), None)
                .await?;

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
        validators_to_remove = updated_state_account.state.validators_for_immediate_removal;
    }

    Ok(stats)
}

#[allow(clippy::too_many_arguments)]
async fn _handle_adding_validators(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    epoch: u64,
    all_steward_accounts: &AllStewardAccounts,
    all_steward_validator_accounts: &AllValidatorAccounts,
    all_validator_accounts: &AllValidatorAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let mut keys_to_add: Vec<&Pubkey> = vec![];
    all_validator_accounts
        .all_history_vote_account_map
        .keys()
        .for_each(|key| {
            if !all_steward_validator_accounts
                .all_history_vote_account_map
                .keys()
                .any(|k| k == key)
            {
                keys_to_add.push(key);
            }
        });

    let mut accounts_to_check: AllValidatorAccounts = AllValidatorAccounts::default();
    all_validator_accounts
        .all_history_vote_account_map
        .keys()
        .for_each(|key| {
            if keys_to_add.contains(&key) {
                accounts_to_check.all_history_vote_account_map.insert(
                    *key,
                    all_validator_accounts
                        .all_history_vote_account_map
                        .get(key)
                        .unwrap()
                        .clone(),
                );
                accounts_to_check.all_stake_account_map.insert(
                    *key,
                    all_validator_accounts
                        .all_stake_account_map
                        .get(key)
                        .unwrap()
                        .clone(),
                );
                accounts_to_check.all_vote_account_map.insert(
                    *key,
                    all_validator_accounts
                        .all_vote_account_map
                        .get(key)
                        .unwrap()
                        .clone(),
                );
            }
        });

    let checks = check_stake_accounts(&accounts_to_check, epoch);

    let good_vote_accounts = checks
        .iter()
        .filter_map(|(vote_address, check)| {
            if check.has_history && !check.has_stake_account {
                let raw_history_account = all_validator_accounts
                    .all_history_vote_account_map
                    .get(vote_address)
                    .unwrap();

                match raw_history_account {
                    Some(raw_history_account) => {
                        let history_account = ValidatorHistory::try_deserialize(
                            &mut raw_history_account.data.as_slice(),
                        )
                        .ok()
                        .unwrap();

                        let start_epoch = epoch.saturating_sub(
                            all_steward_accounts
                                .config_account
                                .parameters
                                .minimum_voting_epochs
                                .saturating_sub(1),
                        );
                        if let Some(entry) = history_account.history.last() {
                            // Steward requires that validators have been active for last minimum_voting_epochs epochs
                            if history_account
                                .history
                                .epoch_credits_range(start_epoch as u16, epoch as u16)
                                .iter()
                                .any(|entry| entry.is_none())
                            {
                                return None;
                            }
                            if entry.activated_stake_lamports
                                < all_steward_accounts
                                    .config_account
                                    .parameters
                                    .minimum_stake_lamports
                            {
                                return None;
                            }
                        } else {
                            println!("Validator {} below liveness minimum", vote_address);
                            return None;
                        }
                    }
                    _ => {
                        return None;
                    }
                }

                Some(*vote_address)
            } else {
                None
            }
        })
        .collect::<Vec<Pubkey>>();

    let ixs_to_run = good_vote_accounts
        .iter()
        .map(|vote_account| {
            let history_account =
                get_validator_history_address(vote_account, &validator_history::id());

            let stake_address =
                get_stake_address(vote_account, &all_steward_accounts.stake_pool_address);

            Instruction {
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
                data: jito_steward::instruction::AutoAddValidatorToPool {}.data(),
            }
        })
        .collect::<Vec<Instruction>>();

    let txs_to_run = package_instructions(&ixs_to_run, 1, priority_fee, Some(1_400_000), None);

    println!("Submitting {} instructions", ixs_to_run.len());
    println!("Submitting {} transactions", txs_to_run.len());

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(50), None).await?;
    // let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(1), None).await?;

    Ok(stats)
}

async fn _handle_delinquent_validators(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    epoch: u64,
    all_steward_accounts: &AllStewardAccounts,
    all_steward_validator_accounts: &AllValidatorAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let checks = check_stake_accounts(all_steward_validator_accounts, epoch);

    let bad_vote_accounts = checks
        .iter()
        .filter_map(|(vote_account, check)| {
            if !check.has_history || check.is_deactivated || !check.has_vote_account {
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

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(50), None).await?;
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
) -> Result<SubmitStats, JitoTransactionError> {
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
                JitoTransactionError::Custom(format!(
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
            submit_packaged_transactions(client, vec![configured_ix], payer, Some(50), None)
                .await?;

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
) -> Result<SubmitStats, JitoTransactionError> {
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

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(50), None).await?;

    Ok(stats)
}

async fn _handle_compute_delegations(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let ix = Instruction {
        program_id: *program_id,
        accounts: jito_steward::accounts::ComputeDelegations {
            config: all_steward_accounts.config_address,
            state_account: all_steward_accounts.state_address,
            validator_list: all_steward_accounts.validator_list_address,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::ComputeDelegations {}.data(),
    };

    let configured_ix = configure_instruction(&[ix], priority_fee, None, None);

    let stats =
        submit_packaged_transactions(client, vec![configured_ix], payer, Some(50), None).await?;

    Ok(stats)
}

async fn _handle_idle(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let ix = Instruction {
        program_id: *program_id,
        accounts: jito_steward::accounts::Idle {
            config: all_steward_accounts.config_address,
            state_account: all_steward_accounts.state_address,
            validator_list: all_steward_accounts.validator_list_address,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::Idle {}.data(),
    };

    let configured_ix = configure_instruction(&[ix], priority_fee, None, None);

    let stats =
        submit_packaged_transactions(client, vec![configured_ix], payer, Some(50), None).await?;

    Ok(stats)
}

async fn _handle_compute_instant_unstake(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
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

    let txs_to_run = package_instructions(&ixs_to_run, 1, priority_fee, Some(1_400_000), None);

    println!("Submitting {} instructions", ixs_to_run.len());
    println!("Submitting {} transactions", txs_to_run.len());

    let stats = submit_packaged_transactions(client, txs_to_run, payer, Some(50), None).await?;

    Ok(stats)
}

async fn _handle_rebalance(
    payer: &Arc<Keypair>,
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
    all_steward_accounts: &AllStewardAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
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

#[allow(clippy::too_many_arguments)]
pub async fn crank_steward(
    client: &Arc<RpcClient>,
    payer: &Arc<Keypair>,
    program_id: &Pubkey,
    epoch: u64,
    all_steward_accounts: &AllStewardAccounts,
    all_steward_validator_accounts: &AllValidatorAccounts,
    all_active_validator_accounts: &AllValidatorAccounts,
    priority_fee: Option<u64>,
) -> Result<SubmitStats, JitoTransactionError> {
    let mut return_stats = SubmitStats::default();
    let should_run_epoch_maintenance =
        all_steward_accounts.state_account.state.current_epoch != epoch;
    let should_crank_state = !should_run_epoch_maintenance;

    {
        // --------- UPDATE STAKE POOL -----------
        println!("Update Stake Pool");

        let stats = _update_pool(
            payer,
            client,
            epoch,
            all_steward_accounts,
            all_steward_validator_accounts,
            priority_fee,
        )
        .await?;

        return_stats.combine(&stats);
    }

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
        // --------- CHECK AND HANDLE INSTANT REMOVAL -----------
        println!("Checking and Handling Instant Removal...");

        let stats = _handle_instant_removal_validators(
            payer,
            client,
            program_id,
            all_steward_accounts,
            priority_fee,
        )
        .await?;

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

        if stats.successes > 0 {
            return Ok(return_stats);
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
                    JitoSendTransactionError::ExceededRetries => {
                        // Continue
                        println!("Exceeded Retries: {:?}", error);
                    }
                    JitoSendTransactionError::TransactionError(e) => {
                        // Flag
                        println!("Transaction: {:?}", e);
                    }
                    JitoSendTransactionError::RpcSimulateTransactionResult(e) => {
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
