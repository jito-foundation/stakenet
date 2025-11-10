//! Directed Stake Ticket Update
//!
//! This module provides functionality to update the directed stake ticket account in the
//! `jito_steward` program. The ticket allows specifying stake preferences across multiple
//! validators with custom stake share allocations.

use std::sync::Arc;

use anchor_lang::AccountDeserialize;
use anyhow::anyhow;
use clap::Parser;
use jito_steward::DirectedStakePreference;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};
use stakenet_sdk::utils::instructions::update_directed_stake_ticket;

use crate::{
    commands::command_args::{parse_pubkey, parse_u16, PermissionedParameters},
    utils::transactions::{configure_instruction, maybe_print_tx},
};

#[derive(Parser)]
#[command(about = "Updates directed stake ticket account")]
pub struct UpdateDirectedStakeTicket {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,

    /// Vote accounts of validators to direct stake to (comma-separated)
    ///
    /// Example: `--vote-pubkey Vote1111...,Vote2222...,Vote3333...`
    #[arg(long, value_delimiter = ',', value_parser = parse_pubkey)]
    pub vote_pubkey: Vec<Pubkey>,

    /// Stake share allocations of JitoSOL in basis points for each validator (comma-separated)
    ///
    /// Must have the same length as `vote_pubkey`. Each value represents the
    /// desired stake allocation for the corresponding validator.
    ///
    /// Example: `--stake-share-bps 5000,3000,2000` (50%, 30%, 20%)
    #[arg(long, env, value_delimiter = ',', value_parser = parse_u16)]
    pub stake_share_bps: Vec<u16>,
}

/// Updates the directed stake ticket with new validator stake preferences.
///
/// This function creates or updates a directed stake ticket that specifies how stake
/// should be distributed across multiple validators. Each validator is assigned a stake
/// share in basis points, allowing precise control over stake allocation.
pub(crate) async fn command_update_directed_stake_ticket(
    args: UpdateDirectedStakeTicket,
    client: Arc<RpcClient>,
    program_id: Pubkey,
) -> anyhow::Result<()> {
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

    if args.vote_pubkey.len().ne(&args.stake_share_bps.len()) {
        return Err(anyhow!(
            "Vote pubkeys and stake share bps should be same length"
        ));
    }

    let preferences = args
        .vote_pubkey
        .iter()
        .zip(args.stake_share_bps)
        .map(|(vote_pubkey, stake_share_bps)| {
            DirectedStakePreference::new(*vote_pubkey, stake_share_bps)
        })
        .collect();

    let ix = update_directed_stake_ticket(&program_id, &steward_config, &signer, preferences);

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

    println!("Signature: {signature}");

    Ok(())
}
