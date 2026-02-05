use std::sync::Arc;

use crate::commands::command_args::CrankRebalance;
use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::StewardStateEnum;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
#[allow(deprecated)]
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, stake, system_program};
use solana_sdk::{signer::Signer, stake::state::StakeStateV2};
use spl_associated_token_account::get_associated_token_address;
use spl_stake_pool::{
    find_stake_program_address, find_transient_stake_program_address, instruction::deposit_sol,
};
use stakenet_sdk::utils::{
    accounts::{
        get_all_steward_accounts, get_directed_stake_meta_address, get_validator_history_address,
    },
    transactions::{package_instructions, print_base58_tx, submit_packaged_transactions},
};
use validator_history::id as validator_history_id;

pub async fn command_crank_rebalance(
    args: CrankRebalance,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    // Creates config account
    let payer = Arc::new(
        read_keypair_file(args.permissionless_parameters.payer_keypair_path)
            .expect("Failed reading keypair file ( Payer )"),
    );

    let validator_history_program_id = validator_history_id();
    let steward_config = args.permissionless_parameters.steward_config;

    let steward_accounts = get_all_steward_accounts(client, &program_id, &steward_config).await?;
    let directed_stake_meta = get_directed_stake_meta_address(&steward_config, &program_id);

    match steward_accounts.state_account.state.state_tag {
        StewardStateEnum::Rebalance => { /* Continue */ }
        _ => {
            println!(
                "State account is not in Rebalance state: {}",
                steward_accounts.state_account.state.state_tag
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
                .get(validator_index as usize)
                .expect("Index is not in progress bitmask");
            if has_been_rebalanced {
                None
            } else {
                let vote_account = steward_accounts.validator_list_account.validators
                    [validator_index as usize]
                    .vote_account_address;
                let history_account =
                    get_validator_history_address(&vote_account, &validator_history_program_id);

                Some((validator_index as usize, vote_account, history_account))
            }
        })
        .collect::<Vec<(usize, Pubkey, Pubkey)>>();

    let stake_rent = client
        .get_minimum_balance_for_rent_exemption(StakeStateV2::size_of())
        .await?;

    let acc_token_address = get_associated_token_address(
        &payer.pubkey(),
        &steward_accounts.stake_pool_account.pool_mint,
    );

    let deposit_sol_ix = deposit_sol(
        &spl_stake_pool::id(),
        &steward_accounts.stake_pool_address,
        &steward_accounts.stake_pool_withdraw_authority,
        &steward_accounts.stake_pool_account.reserve_stake,
        &payer.pubkey(),
        &acc_token_address,
        &steward_accounts.stake_pool_account.manager_fee_account,
        &acc_token_address,
        &steward_accounts.stake_pool_account.pool_mint,
        &spl_token::id(),
        stake_rent,
    );

    let ixs_to_run: Vec<Instruction> = validators_to_run
        .iter()
        .flat_map(|(validator_index, vote_account, history_account)| {
            println!("vote_account ({}): {}", validator_index, vote_account);

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

            let rebalance_ix = Instruction {
                program_id,
                accounts: jito_steward::accounts::Rebalance {
                    config: steward_config,
                    state_account: steward_accounts.state_address,
                    validator_history: *history_account,
                    stake_pool_program: spl_stake_pool::id(),
                    stake_pool: steward_accounts.stake_pool_address,
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
                    stake_config: stake::config::ID,
                    directed_stake_meta,
                }
                .to_account_metas(None),
                data: jito_steward::instruction::Rebalance {
                    validator_list_index: *validator_index as u64,
                }
                .data(),
            };

            vec![deposit_sol_ix.clone(), rebalance_ix]
        })
        .collect();

    let txs_to_run = package_instructions(
        &ixs_to_run,
        args.permissionless_parameters
            .transaction_parameters
            .chunk_size
            .unwrap_or(2),
        args.permissionless_parameters
            .transaction_parameters
            .priority_fee,
        args.permissionless_parameters
            .transaction_parameters
            .compute_limit
            .or(Some(1_400_000)),
        None,
    );

    if args
        .permissionless_parameters
        .transaction_parameters
        .print_tx
    {
        txs_to_run.iter().for_each(|tx| print_base58_tx(tx));
    } else {
        println!("Submitting {} instructions", ixs_to_run.len());
        println!("Submitting {} transactions", txs_to_run.len());

        let submit_stats =
            submit_packaged_transactions(client, txs_to_run, &payer, None, None).await?;

        println!("Submit stats: {:?}", submit_stats);
    }

    Ok(())
}
