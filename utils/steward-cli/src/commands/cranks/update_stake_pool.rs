use std::sync::Arc;

use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file};
#[allow(deprecated)]
use spl_stake_pool::{
    find_withdraw_authority_program_address,
    instruction::{cleanup_removed_validator_entries, update_stake_pool_balance, update_validator_list_balance},
    state::StakeStatus,
    MAX_VALIDATORS_TO_UPDATE,
};
use stakenet_sdk::utils::{
    accounts::get_all_steward_accounts,
    transactions::{print_base58_tx, submit_packaged_transactions, configure_instruction, print_errors_if_any},
};

use crate::commands::command_args::CrankUpdateStakePool;

pub async fn command_crank_update_stake_pool(
    args: CrankUpdateStakePool,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let payer = Arc::new(
        read_keypair_file(&args.permissionless_parameters.payer_keypair_path)
            .expect("Failed reading keypair file ( Payer )"),
    );

    let steward_config = args.permissionless_parameters.steward_config;
    let no_merge = args.no_merge;

    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let epoch = client.get_epoch_info().await?.epoch;

    let stake_pool = &all_steward_accounts.stake_pool_account;
    let validator_list = &all_steward_accounts.validator_list_account;
    let stake_pool_address = &all_steward_accounts.stake_pool_address;

    let (withdraw_authority, _) =
        find_withdraw_authority_program_address(&spl_stake_pool::ID, stake_pool_address);

    // Build update_validator_list_balance instructions
    let mut update_list_instructions: Vec<Instruction> = vec![];
    let mut start_index = 0;
    for validator_info_chunk in validator_list.validators.chunks(MAX_VALIDATORS_TO_UPDATE) {
        let should_update = validator_info_chunk.iter().any(|info| {
            if u64::from(info.last_update_epoch) < epoch {
                true
            } else {
                matches!(
                    StakeStatus::try_from(info.status).unwrap(),
                    StakeStatus::DeactivatingValidator
                )
            }
        });

        if should_update {
            let validator_vote_accounts = validator_info_chunk
                .iter()
                .map(|v| v.vote_account_address)
                .collect::<Vec<Pubkey>>();

            #[allow(deprecated)]
            update_list_instructions.push(update_validator_list_balance(
                &spl_stake_pool::ID,
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
        start_index = start_index.saturating_add(MAX_VALIDATORS_TO_UPDATE as u32);
    }

    // Build final cleanup instructions
    let final_instructions = vec![
        update_stake_pool_balance(
            &spl_stake_pool::ID,
            stake_pool_address,
            &withdraw_authority,
            &stake_pool.validator_list,
            &stake_pool.reserve_stake,
            &stake_pool.manager_fee_account,
            &stake_pool.pool_mint,
            &stake_pool.token_program_id,
        ),
        cleanup_removed_validator_entries(
            &spl_stake_pool::ID,
            stake_pool_address,
            &stake_pool.validator_list,
        ),
    ];

    let priority_fee = args.permissionless_parameters.transaction_parameters.priority_fee;
    let compute_limit = args.permissionless_parameters.transaction_parameters.compute_limit;

    if args.permissionless_parameters.transaction_parameters.print_tx {
        println!("=== Update Validator List Balance Instructions ===");
        for ix in &update_list_instructions {
            print_base58_tx(&[ix.clone()]);
        }
        println!("\n=== Update Stake Pool Balance & Cleanup Instructions ===");
        print_base58_tx(&final_instructions);
        return Ok(());
    }

    // Execute update_validator_list_balance transactions
    if !update_list_instructions.is_empty() {
        println!("Updating validator list balances ({} transactions)...", update_list_instructions.len());
        let update_txs: Vec<Vec<Instruction>> = update_list_instructions
            .into_iter()
            .map(|ix| configure_instruction(&[ix], priority_fee, compute_limit.or(Some(1_400_000)), None))
            .collect();

        let update_stats = submit_packaged_transactions(client, update_txs, &payer, Some(50), None).await?;
        println!(
            "Update validator list: {} succeeded, {} failed",
            update_stats.successes, update_stats.errors
        );
        print_errors_if_any(&update_stats);
    } else {
        println!("No validators need updating");
    }

    // Execute final cleanup transactions
    println!("Updating stake pool balance and cleaning up...");
    let cleanup_txs: Vec<Vec<Instruction>> = final_instructions
        .into_iter()
        .map(|ix| configure_instruction(&[ix], priority_fee, compute_limit.or(Some(1_400_000)), None))
        .collect();

    let cleanup_stats = submit_packaged_transactions(client, cleanup_txs, &payer, Some(50), None).await?;
    println!(
        "Cleanup: {} succeeded, {} failed",
        cleanup_stats.successes, cleanup_stats.errors
    );
    print_errors_if_any(&cleanup_stats);

    println!("Stake pool update complete!");
    Ok(())
}
