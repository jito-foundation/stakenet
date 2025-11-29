use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig};
use solana_program::instruction::Instruction;

use solana_sdk::{
    commitment_config::CommitmentConfig, pubkey::Pubkey, signature::read_keypair_file,
    signer::Signer, transaction::Transaction,
};

use crate::commands::command_args::InstantRemoveValidator;
use stakenet_sdk::utils::{
    accounts::{get_all_steward_accounts, get_directed_stake_meta_address},
    transactions::{configure_instruction, print_base58_tx},
};

pub async fn command_instant_remove_validator(
    args: InstantRemoveValidator,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let payer = read_keypair_file(args.permissionless_parameters.payer_keypair_path)
        .expect("Failed reading keypair file (Payer)");

    let steward_config = args.permissionless_parameters.steward_config;

    let steward_accounts = get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let directed_stake_meta = get_directed_stake_meta_address(&steward_config, &program_id);

    let ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::InstantRemoveValidator {
            config: args.permissionless_parameters.steward_config,
            state_account: steward_accounts.state_address,
            validator_list: steward_accounts.validator_list_address,
            stake_pool: steward_accounts.stake_pool_address,
            directed_stake_meta,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::InstantRemoveValidator {
            validator_index_to_remove: args.validator_index_to_remove,
        }
        .data(),
    };

    let blockhash = client.get_latest_blockhash().await?;

    let configured_ix = configure_instruction(
        &[ix],
        args.permissionless_parameters
            .transaction_parameters
            .priority_fee,
        args.permissionless_parameters
            .transaction_parameters
            .compute_limit,
        args.permissionless_parameters
            .transaction_parameters
            .heap_size,
    );

    let transaction = Transaction::new_signed_with_payer(
        &configured_ix,
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );

    if args
        .permissionless_parameters
        .transaction_parameters
        .print_tx
    {
        print_base58_tx(&configured_ix)
    } else {
        let signature = client
            .send_and_confirm_transaction_with_spinner_and_config(
                &transaction,
                CommitmentConfig::default(),
                RpcSendTransactionConfig::default(),
            )
            .await?;

        println!("Signature: {}", signature);
    }

    Ok(())
}
