use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
#[allow(deprecated)]
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, stake, system_program, sysvar,
};
use spl_stake_pool::{find_stake_program_address, find_transient_stake_program_address};
use stakenet_sdk::utils::transactions::{package_instructions, submit_transactions};
use stakenet_sdk::utils::{accounts::get_all_steward_accounts, transactions::print_base58_tx};

use crate::commands::command_args::PermissionedParameters;

#[derive(Parser)]
#[command(about = "Removes validator from the pool")]
pub struct RemoveValidatorFromPool {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,

    /// Vote account pubkey that want to remove from the pool
    #[arg(long)]
    vote_pubkey: Pubkey,
}

pub async fn command_remove_validator_from_pool(
    args: RemoveValidatorFromPool,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let payer = read_keypair_file(args.permissioned_parameters.authority_keypair_path)
        .expect("Failed reading keypair file ( Payer )");
    let arc_payer = Arc::new(payer);

    let steward_config = args.permissioned_parameters.steward_config;
    let steward_accounts = get_all_steward_accounts(client, &program_id, &steward_config).await?;

    // Find the validator
    let validator_entry = steward_accounts
        .validator_list_account
        .validators
        .iter()
        .enumerate()
        .find(|(_, v)| v.vote_account_address == args.vote_pubkey);

    let (validator_index, validator) = validator_entry
        .ok_or_else(|| anyhow::anyhow!("Validator {} not found in pool", args.vote_pubkey))?;

    let active_stake_lamports: u64 = validator.active_stake_lamports.into();
    let transient_stake_lamports: u64 = validator.transient_stake_lamports.into();

    // Show current state
    println!("=== Validator to Remove ===");
    println!("Index: {validator_index}");
    println!("Vote Account: {}", validator.vote_account_address);
    println!("Active Stake Lamports: {active_stake_lamports}");
    println!("Transient Stake Lamports: {transient_stake_lamports}");
    println!("Status: {:?}", validator.status);

    // Check if validator has any stake
    if active_stake_lamports > 0 || transient_stake_lamports > 0 {
        println!("\n⚠️  WARNING: Validator still has stake! This may fail or require multiple epochs to complete.");
    }

    // Confirm before proceeding
    if !args.permissioned_parameters.transaction_parameters.print_tx {
        println!("\n❓ Proceed with removal? This cannot be undone. (Ctrl+C to cancel)");
        std::thread::sleep(std::time::Duration::from_secs(3));
    }

    let (stake_address, _) = find_stake_program_address(
        &spl_stake_pool::id(),
        &validator.vote_account_address,
        &steward_accounts.stake_pool_address,
        None,
    );

    let (transient_stake_address, _) = find_transient_stake_program_address(
        &spl_stake_pool::id(),
        &validator.vote_account_address,
        &steward_accounts.stake_pool_address,
        validator.transient_seed_suffix.into(),
    );

    let ix = Instruction {
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
            validator_list_index: validator_index as u64,
        }
        .data(),
    };

    let ixs = vec![ix];

    let txs_to_run = package_instructions(
        &ixs,
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
        println!("\n📤 Submitting removal transaction...");
        let submit_stats = submit_transactions(client, txs_to_run, &arc_payer, 20, 30).await?;
        println!("✅ Submit stats: {:?}", submit_stats);
    }

    Ok(())
}
