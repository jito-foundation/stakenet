use std::sync::Arc;

use anchor_lang::AccountDeserialize;
use anyhow::{anyhow, Result};
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};
use stakenet_sdk::utils::{
    accounts::get_all_steward_accounts, instructions::compute_directed_stake_meta,
};

use crate::{
    commands::command_args::PermissionedParameters,
    utils::transactions::{configure_instruction, maybe_print_tx},
};

#[derive(Parser)]
#[command(about = "Compute directed stake metadata including tickets and JitoSOL balances")]
pub struct ComputeDirectedStakeMeta {
    #[command(flatten)]
    permissioned_parameters: PermissionedParameters,

    // Jito SOL Token mint address
    #[arg(long, env)]
    pub token_mint: Pubkey,
}

pub async fn command_compute_directed_stake_meta(
    args: ComputeDirectedStakeMeta,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.permissioned_parameters.steward_config;
    // Determine authority pubkey for the instruction. When printing, allow using provided flag or derive from on-chain config.
    let signer = if args.permissioned_parameters.transaction_parameters.print_tx
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
        read_keypair_file(&args.permissioned_parameters.authority_keypair_path)
            .expect("Failed reading keypair file ( Authority )")
            .pubkey()
    };

    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;
    let ixs = compute_directed_stake_meta(
        client.clone(),
        &args.token_mint,
        &all_steward_accounts.stake_pool_address,
        &all_steward_accounts.config_address,
        &signer,
        &program_id,
    )
    .await
    .map_err(|e| anyhow!(e.to_string()))?;

    let configured_ix = configure_instruction(
        &ixs,
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
        .map_err(|e| anyhow!("Failed to read keypair file: {e}"))?;

    let blockhash = client.get_latest_blockhash().await?;

    let transaction = Transaction::new_signed_with_payer(
        &configured_ix,
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .await?;

    println!("Signature: {}", signature);

    Ok(())
}
