use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::UpdateParametersArgs;

use keeper_core::{get_multiple_accounts_batched, submit_instructions};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use spl_stake_pool::{find_stake_program_address, find_transient_stake_program_address};
use validator_history::id as validator_history_id;

use solana_sdk::{
    commitment_config::CommitmentConfig, compute_budget::ComputeBudgetInstruction, pubkey::Pubkey,
    signature::read_keypair_file, signer::Signer, stake, system_program, sysvar,
    transaction::Transaction,
};
use validator_history::id;

use crate::utils::accounts::{
    get_all_steward_accounts, get_validator_history_address, UsefulStewardAccounts,
};

use super::commands::RemoveBadValidators;

pub async fn command_remove_bad_validators(
    args: RemoveBadValidators,
    client: RpcClient,
    program_id: Pubkey,
) -> Result<()> {
    let arc_client = Arc::new(client);

    // Creates config account
    let payer =
        read_keypair_file(args.payer_keypair_path).expect("Failed reading keypair file ( Payer )");

    let validator_history_program_id = validator_history_id();
    let steward_config = args.steward_config;

    let steward_accounts =
        get_all_steward_accounts(&arc_client, &program_id, &steward_config).await?;

    let validators_to_run = (0..steward_accounts.validator_list_account.validators.len())
        .filter_map(|validator_index| {
            let has_been_scored = steward_accounts
                .state_account
                .state
                .progress
                .get(validator_index)
                .expect("Index is not in progress bitmask");
            if has_been_scored {
                return None;
            } else {
                let vote_account = steward_accounts.validator_list_account.validators
                    [validator_index]
                    .vote_account_address;
                let history_account =
                    get_validator_history_address(&vote_account, &validator_history_program_id);

                return Some((validator_index, vote_account, history_account));
            }
        })
        .collect::<Vec<(usize, Pubkey, Pubkey)>>();

    let history_accounts = validators_to_run
        .iter()
        .map(|(validator_index, vote_account, history_account)| *history_account)
        .collect::<Vec<Pubkey>>();

    let validator_history_accounts =
        get_multiple_accounts_batched(&history_accounts, &arc_client).await?;

    let bad_history_accounts = validator_history_accounts
        .iter()
        .zip(validators_to_run)
        .filter_map(
            |(account, (index, vote_account, history_account))| match account {
                Some(_) => None,
                None => Some((index, vote_account, history_account)),
            },
        )
        .collect::<Vec<(usize, Pubkey, Pubkey)>>();

    println!("Bad history accounts: {:?}", bad_history_accounts);

    let ixs_to_run = bad_history_accounts
        .iter()
        .map(|(validator_index, vote_account, history_account)| {
            println!(
                "index: {}, vote_account: {}, history_account: {}\n",
                validator_index, vote_account, history_account
            );

            let (stake_address, _) = find_stake_program_address(
                &spl_stake_pool::id(),
                &vote_account,
                &steward_accounts.stake_pool_address,
                None,
            );

            let (transient_stake_address, _) = find_transient_stake_program_address(
                &spl_stake_pool::id(),
                &vote_account,
                &steward_accounts.stake_pool_address,
                steward_accounts.validator_list_account.validators[*validator_index]
                    .transient_seed_suffix
                    .into(),
            );

            Instruction {
                program_id: program_id,
                accounts: jito_steward::accounts::RemoveValidatorFromPool {
                    signer: payer.pubkey(),
                    config: steward_config,
                    steward_state: steward_accounts.state_address,
                    stake_pool_program: spl_stake_pool::id(),
                    stake_pool: steward_accounts.stake_pool_address,
                    staker: steward_accounts.stake_pool_account.staker,
                    withdraw_authority: steward_accounts.stake_pool_withdraw_authority,
                    validator_list: steward_accounts.validator_list_address,
                    stake_account: stake_address,
                    transient_stake_account: transient_stake_address,
                    clock: sysvar::clock::id(),
                    system_program: system_program::id(),
                    stake_program: stake::program::id(),
                }
                .to_account_metas(None),
                data: jito_steward::instruction::RemoveValidatorFromPool {
                    validator_list_index: *validator_index,
                }
                .data(),
            }
        })
        .collect::<Vec<Instruction>>();

    println!("Submitting {} instructions", ixs_to_run.len());
    println!(
        "Validator List Length: {}",
        steward_accounts.validator_list_account.validators.len()
    );

    let (blockhash, _) = arc_client
        .get_latest_blockhash_with_commitment(CommitmentConfig::finalized())
        .await?;

    let tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(1_000_000),
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ixs_to_run[0].clone(),
        ],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );

    println!("Sending transaction");

    let result = arc_client.send_and_confirm_transaction(&tx).await;

    match result {
        Ok(signature) => println!("Signature: {:?}", signature),
        Err(e) => println!("Error: {:?}", e),
    }

    Ok(())
}
