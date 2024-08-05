use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use spl_stake_pool::{find_stake_program_address, find_transient_stake_program_address};
use stakenet_sdk::utils::transactions::{
    get_multiple_accounts_batched, package_instructions, submit_transactions,
};
use validator_history::id as validator_history_id;

use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, stake, system_program, sysvar,
};

use crate::commands::command_args::RemoveBadValidators;

use stakenet_sdk::utils::{
    accounts::{get_all_steward_accounts, get_validator_history_address},
    transactions::print_base58_tx,
};

pub async fn command_remove_bad_validators(
    args: RemoveBadValidators,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    // Creates config account
    let payer = read_keypair_file(args.permissioned_parameters.authority_keypair_path)
        .expect("Failed reading keypair file ( Payer )");
    let arc_payer = Arc::new(payer);

    let validator_history_program_id = validator_history_id();
    let steward_config = args.permissioned_parameters.steward_config;

    let steward_accounts = get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let validators_to_run = (0..steward_accounts.validator_list_account.validators.len())
        .filter_map(|validator_index| {
            let has_been_scored = steward_accounts
                .state_account
                .state
                .progress
                .get(validator_index)
                .expect("Index is not in progress bitmask");
            if has_been_scored {
                None
            } else {
                let vote_account = steward_accounts.validator_list_account.validators
                    [validator_index]
                    .vote_account_address;
                let history_account =
                    get_validator_history_address(&vote_account, &validator_history_program_id);

                Some((validator_index, vote_account, history_account))
            }
        })
        .collect::<Vec<(usize, Pubkey, Pubkey)>>();

    let history_accounts = validators_to_run
        .iter()
        .map(|(_, _, history_account)| *history_account)
        .collect::<Vec<Pubkey>>();

    let validator_history_accounts =
        get_multiple_accounts_batched(&history_accounts, client).await?;

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
                vote_account,
                &steward_accounts.stake_pool_address,
                None,
            );

            let (transient_stake_address, _) = find_transient_stake_program_address(
                &spl_stake_pool::id(),
                vote_account,
                &steward_accounts.stake_pool_address,
                steward_accounts.validator_list_account.validators[*validator_index]
                    .transient_seed_suffix
                    .into(),
            );

            Instruction {
                program_id,
                accounts: jito_steward::accounts::RemoveValidatorFromPool {
                    admin: arc_payer.pubkey(),
                    config: steward_config,
                    state_account: steward_accounts.state_address,
                    stake_pool_program: spl_stake_pool::id(),
                    stake_pool: steward_accounts.stake_pool_address,
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
                    validator_list_index: *validator_index as u64,
                }
                .data(),
            }
        })
        .collect::<Vec<Instruction>>();

    let txs_to_run = package_instructions(
        &ixs_to_run,
        args.permissioned_parameters
            .transaction_parameters
            .chunk_size
            .unwrap_or(1),
        args.permissioned_parameters
            .transaction_parameters
            .priority_fee,
        args.permissioned_parameters
            .transaction_parameters
            .compute_limit
            .or(Some(1_400_000)),
        args.permissioned_parameters
            .transaction_parameters
            .heap_size
            .or(Some(256 * 1024)),
    );

    if args.permissioned_parameters.transaction_parameters.print_tx {
        txs_to_run.iter().for_each(|tx| print_base58_tx(tx));
    } else {
        println!("Submitting {} instructions", ixs_to_run.len());

        let submit_stats = submit_transactions(client, txs_to_run, &arc_payer).await?;

        println!("Submit stats: {:?}", submit_stats);
    }

    Ok(())
}
