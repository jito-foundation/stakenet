use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::StewardStateEnum;
use keeper_core::{submit_instructions, submit_transactions};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use spl_stake_pool::{find_stake_program_address, find_transient_stake_program_address};
use validator_history::id as validator_history_id;

use solana_sdk::{
    pubkey::Pubkey,
    signature::read_keypair_file,
    signer::Signer,
    stake, system_program,
    sysvar::{self, rent},
    transaction::Transaction,
};

use crate::{
    commands::commands::CrankRebalance,
    utils::{
        accounts::{
            get_all_steward_accounts, get_cluster_history_address, get_steward_state_account,
            get_validator_history_address, UsefulStewardAccounts,
        },
        print::state_tag_to_string,
        transactions::{debug_send_single_transaction, package_instructions},
    },
};

pub async fn command_crank_rebalance(
    args: CrankRebalance,
    client: RpcClient,
    program_id: Pubkey,
) -> Result<()> {
    let args = args.permissionless_parameters;
    // Creates config account
    let payer =
        read_keypair_file(args.payer_keypair_path).expect("Failed reading keypair file ( Payer )");

    let validator_history_program_id = validator_history_id();
    let steward_config = args.steward_config;

    let steward_accounts = get_all_steward_accounts(&client, &program_id, &steward_config).await?;

    match steward_accounts.state_account.state.state_tag {
        StewardStateEnum::Rebalance => { /* Continue */ }
        _ => {
            println!(
                "State account is not in Rebalance state: {}",
                state_tag_to_string(steward_accounts.state_account.state.state_tag)
            );
            return Ok(());
        }
    }

    let validators_to_run = (0..steward_accounts.state_account.state.num_pool_validators)
        .filter_map(|validator_index| {
            let has_been_rebalanced = steward_accounts
                .state_account
                .state
                .progress
                .get(validator_index)
                .expect("Index is not in progress bitmask");
            if has_been_rebalanced {
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

    let ixs_to_run = validators_to_run
        .iter()
        .map(|(validator_index, vote_account, history_account)| {
            println!("vote_account ({}): {}", validator_index, vote_account);

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
                accounts: jito_steward::accounts::Rebalance {
                    config: steward_config,
                    state_account: steward_accounts.state_address,
                    validator_history: *history_account,
                    stake_pool_program: spl_stake_pool::id(),
                    stake_pool: steward_accounts.stake_pool_address,
                    staker: steward_accounts.staker_address,
                    withdraw_authority: steward_accounts.stake_pool_withdraw_authority,
                    validator_list: steward_accounts.validator_list_address,
                    reserve_stake: steward_accounts.stake_pool_account.reserve_stake,
                    stake_account: stake_address,
                    transient_stake_account: transient_stake_address,
                    vote_account: *vote_account,
                    system_program: system_program::id(),
                    stake_program: stake::program::id(),
                    rent: solana_sdk::sysvar::rent::id(),
                    clock: solana_sdk::sysvar::clock::id(),
                    stake_history: solana_sdk::sysvar::stake_history::id(),
                    stake_config: stake::config::id(),
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

    let txs_to_run = package_instructions(
        &ixs_to_run,
        1,
        Some(args.priority_fee),
        Some(1_400_000),
        None,
    );

    println!("Submitting {} instructions", ixs_to_run.len());
    println!("Submitting {} transactions", txs_to_run.len());

    debug_send_single_transaction(
        &Arc::new(client),
        &Arc::new(payer),
        &txs_to_run[0],
        Some(true),
    )
    .await;

    // let submit_stats = submit_transactions(&Arc::new(client), txs_to_run, &Arc::new(payer)).await?;

    // println!("Submit stats: {:?}", submit_stats);

    Ok(())
}
