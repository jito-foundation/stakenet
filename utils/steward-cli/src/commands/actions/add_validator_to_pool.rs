use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::sysvar::{rent, stake_history};
#[allow(deprecated)]
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, stake, system_program, sysvar,
    transaction::Transaction,
};
use spl_stake_pool::find_stake_program_address;
use stakenet_sdk::utils::{
    accounts::get_all_steward_accounts,
    transactions::{configure_instruction, print_base58_tx},
};

use crate::commands::command_args::PermissionedParameters;

#[derive(Parser)]
#[command(about = "Add validator to pool")]
pub struct AddValidatorToPool {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,

    /// Vote account
    #[arg(long, env)]
    pub vote_account: Pubkey,
}

pub async fn command_add_validator_to_pool(
    args: AddValidatorToPool,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    // Creates config account
    let authority = Arc::new(
        read_keypair_file(args.permissioned_parameters.authority_keypair_path)
            .expect("Failed reading keypair file ( Payer )"),
    );

    let steward_config = args.permissioned_parameters.steward_config;

    let steward_accounts = get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let vote_account = args.vote_account;

    let (stake_address, _) = find_stake_program_address(
        &spl_stake_pool::id(),
        &vote_account,
        &steward_accounts.stake_pool_address,
        None,
    );

    let ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::AddValidatorToPool {
            config: steward_config,
            state_account: steward_accounts.state_address,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: steward_accounts.stake_pool_address,
            reserve_stake: steward_accounts.stake_pool_account.reserve_stake,
            withdraw_authority: steward_accounts.stake_pool_withdraw_authority,
            validator_list: steward_accounts.validator_list_address,
            stake_account: stake_address,
            vote_account,
            admin: authority.pubkey(),
            rent: rent::id(),
            clock: sysvar::clock::id(),
            stake_history: stake_history::id(),
            stake_config: stake::config::ID,
            system_program: system_program::id(),
            stake_program: stake::program::id(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AddValidatorToPool {
            validator_seed: None,
        }
        .data(),
    };

    let blockhash = client.get_latest_blockhash().await?;

    let configured_ix = configure_instruction(
        &[ix],
        args.permissioned_parameters
            .transaction_parameters
            .priority_fee,
        args.permissioned_parameters
            .transaction_parameters
            .compute_limit,
        args.permissioned_parameters
            .transaction_parameters
            .heap_size,
    );

    let transaction = Transaction::new_signed_with_payer(
        &configured_ix,
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    if args.permissioned_parameters.transaction_parameters.print_tx {
        print_base58_tx(&configured_ix)
    } else {
        let signature = client
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await?;

        println!("Signature: {}", signature);
    }

    Ok(())
}
