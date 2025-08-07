use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::{
    derive_steward_state_address, UpdateParametersArgs, UpdatePriorityFeeParametersArgs,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;

use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair},
    signer::Signer,
    transaction::Transaction,
};

use crate::commands::command_args::InitSteward;
use stakenet_sdk::utils::transactions::{configure_instruction, print_base58_tx};

pub async fn command_init_steward(
    args: InitSteward,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    // Creates config account
    let authority = read_keypair_file(args.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let staker_keypair = {
        if let Some(staker_keypair_path) = args.staker_keypair_path {
            read_keypair_file(staker_keypair_path).expect("Failed reading keypair file ( Staker )")
        } else {
            authority.insecure_clone()
        }
    };

    let steward_config = {
        if let Some(steward_config_keypair_path) = args.steward_config_keypair_path {
            read_keypair_file(steward_config_keypair_path)
                .expect("Failed reading keypair file ( Steward Config )")
        } else {
            Keypair::new()
        }
    };

    let (state_account, _) = derive_steward_state_address(&steward_config.pubkey());

    let update_parameters_args: UpdateParametersArgs = args.config_parameters.into();
    let update_priority_fee_parameters_args: UpdatePriorityFeeParametersArgs =
        args.config_priority_fee_parameters.into();

    // Check if already created
    match client.get_account(&steward_config.pubkey()).await {
        Ok(config_account) => {
            if config_account.owner == program_id {
                println!("Config account already exists");
                return Ok(());
            }
        }
        Err(_) => { /* Account does not exist, continue */ }
    }

    let init_ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::InitializeSteward {
            config: steward_config.pubkey(),
            stake_pool: args.stake_pool,
            state_account,
            stake_pool_program: spl_stake_pool::id(),
            system_program: anchor_lang::solana_program::system_program::id(),
            current_staker: staker_keypair.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::InitializeSteward {
            update_parameters_args,
            update_priority_fee_parameters_args,
        }
        .data(),
    };

    let blockhash = client.get_latest_blockhash().await?;

    let configured_ix = configure_instruction(
        &[init_ix],
        args.transaction_parameters.priority_fee,
        args.transaction_parameters.compute_limit,
        args.transaction_parameters.heap_size,
    );

    let transaction = Transaction::new_signed_with_payer(
        &configured_ix,
        Some(&authority.pubkey()),
        &[&authority, &steward_config, &staker_keypair],
        blockhash,
    );

    if args.transaction_parameters.print_tx {
        print_base58_tx(&configured_ix)
    } else {
        let signature = client
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await?;

        println!("Signature: {}", signature);
        println!("Steward Config: {}", steward_config.pubkey());
    }

    Ok(())
}
