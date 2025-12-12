use std::sync::Arc;

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::UpdateParametersArgs;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};

use crate::commands::command_args::UpdateConfig;
use crate::utils::transactions::maybe_print_tx;
use stakenet_sdk::utils::transactions::configure_instruction;

pub async fn command_update_config(
    args: UpdateConfig,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.permissioned_parameters.steward_config;

    let update_parameters_args: UpdateParametersArgs = args.config_parameters.into();

    // Determine authority pubkey for the instruction. When printing, allow using provided flag or derive from on-chain config.
    let authority_pubkey = if args.permissioned_parameters.transaction_parameters.print_tx
        || args
            .permissioned_parameters
            .transaction_parameters
            .print_gov_tx
    {
        if let Some(pubkey) = args.permissioned_parameters.authority_pubkey {
            pubkey
        } else {
            // Fallback to reading on-chain config to get parameters_authority
            let config_account = client.get_account(&steward_config).await?;
            let config =
                jito_steward::Config::try_deserialize(&mut config_account.data.as_slice())?;
            config.parameters_authority
        }
    } else {
        // We will load the keypair, so we can use its pubkey
        read_keypair_file(&args.permissioned_parameters.authority_keypair_path)
            .expect("Failed reading keypair file ( Authority )")
            .pubkey()
    };

    let ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::UpdateParameters {
            config: steward_config,
            authority: authority_pubkey,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::UpdateParameters {
            update_parameters_args,
        }
        .data(),
    };

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

    // If we are printing, do so and return early without requiring the authority keypair
    if maybe_print_tx(
        &configured_ix,
        &args.permissioned_parameters.transaction_parameters,
    ) {
        return Ok(());
    }

    // Otherwise, send transaction signed by the authority
    let authority = read_keypair_file(&args.permissioned_parameters.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let blockhash = client
        .get_latest_blockhash()
        .await
        .expect("Failed to get recent blockhash");

    let transaction = Transaction::new_signed_with_payer(
        &configured_ix,
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .await?;

    println!("Signature: {signature}");

    Ok(())
}
