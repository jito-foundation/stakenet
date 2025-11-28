//! Directed Stake Metadata Computation
//!
//! This module provides functionality to compute directed stake metadata in the
//! `jito_steward` program. This computation aggregates information from directed stake tickets
//! and JitoSOL token balances to update the system's stake distribution metadata.
//!
//! # Overview
//!
//! The directed stake metadata computation is an operation that:
//! - Aggregates all directed stake tickets from validators
//! - Computes JitoSOL token balances for relevant accounts
//! - Updates the DirectedStakeMeta account with aggregated information
//! - Ensures stake distribution reflects current preferences and holdings
//!
//! # Process
//!
//! 1. Load or determine the authority (keypair for execution, pubkey for printing)
//! 2. Fetch all steward-related accounts (config, state, stake pool, etc.)
//! 3. Generate computation instructions via the SDK utility
//! 4. Configure transaction parameters (priority fee, compute limits)
//! 5. Either print the transaction or submit it on-chain

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anchor_lang::AccountDeserialize;
use anyhow::{anyhow, Result};
use clap::Parser;
use jito_steward::DirectedStakeTicket;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    native_token::LAMPORTS_PER_SOL, pubkey::Pubkey, signature::read_keypair_file, signer::Signer,
    transaction::Transaction,
};
use stakenet_sdk::utils::{
    accounts::{get_all_steward_accounts, get_directed_stake_meta, get_directed_stake_tickets},
    helpers::get_token_balance,
    instructions::compute_directed_stake_meta,
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

/// Computes directed stake metadata by aggregating tickets and token balances.
///
/// This function executes a computation that updates the [`DirectedStakeMeta`] account
/// with current information from all directed stake tickets and JitoSOL token
/// balances. This metadata is essential for the steward system to accurately
/// distribute stake according to validator preferences and token holder weights.
pub async fn command_crank_compute_directed_stake_meta(
    args: ComputeDirectedStakeMeta,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.permissioned_parameters.steward_config;

    // Fetch directed stake tickets to show summary stats
    let ticket_map = get_directed_stake_tickets(client.clone(), &program_id).await?;
    let tickets: Vec<DirectedStakeTicket> = ticket_map.values().copied().collect();
    let num_tickets = tickets.len();

    // Count preferences in tickets
    let mut ticket_validators = HashSet::new();
    let mut total_preferences = 0;
    let mut tickets_with_balance = 0;
    let mut total_jitosol_directed = 0u64;

    // Get JitoSOL balances for all ticket holders
    let mut jitosol_balances: HashMap<Pubkey, u64> = HashMap::new();
    for ticket in &tickets {
        total_preferences += ticket.num_preferences as usize;
        for i in 0..ticket.num_preferences as usize {
            let vote_pubkey = ticket.staker_preferences[i].vote_pubkey;
            if vote_pubkey != Pubkey::default() {
                ticket_validators.insert(vote_pubkey);
            }
        }

        let balance = get_token_balance(
            client.clone(),
            &args.token_mint,
            &ticket.ticket_update_authority,
        )
        .await
        .unwrap_or(0);

        if balance > 0 {
            jitosol_balances.insert(ticket.ticket_update_authority, balance);
            tickets_with_balance += 1;
            total_jitosol_directed = total_jitosol_directed.saturating_add(balance);
        }
    }

    // Simulate aggregation to count final targets
    // Note: You'll need to get the actual conversion rate from the stake pool
    // This is a simplified version - adjust based on your actual implementation
    let mut final_targets = HashSet::new();
    for ticket in &tickets {
        let jitosol_balance = jitosol_balances
            .get(&ticket.ticket_update_authority)
            .copied()
            .unwrap_or(0);
        if jitosol_balance == 0 {
            continue;
        }
        for i in 0..ticket.num_preferences as usize {
            let pref = &ticket.staker_preferences[i];
            if pref.vote_pubkey != Pubkey::default() {
                final_targets.insert(pref.vote_pubkey);
            }
        }
    }

    let num_final_targets = final_targets.len();
    let num_validators_in_tickets = ticket_validators.len();

    println!("=== Directed Stake Metadata Computation Summary ===");
    println!("Total Directed Stake Tickets: {num_tickets}");
    println!("Tickets with JitoSOL Balance: {tickets_with_balance}");
    println!("Total Stake Preferences: {total_preferences}");
    println!("Unique Validators in Tickets: {num_validators_in_tickets}");
    println!("Final Aggregated Targets: {num_final_targets} (after filtering by balance)");

    if tickets_with_balance < num_tickets {
        println!(
            "\n⚠️  {} tickets have no JitoSOL balance and will be excluded",
            num_tickets - tickets_with_balance
        );
    }

    println!("====================================================\n");

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
        .send_and_confirm_transaction_with_spinner_and_config(
            &transaction,
            CommitmentConfig::processed(),
            RpcSendTransactionConfig::default(),
        )
        .await?;

    println!("\n=== Transaction Successful ===");
    println!("Signature: {}", signature);
    println!("Updated metadata:");
    println!("  - {num_tickets} tickets processed");
    println!("  - {tickets_with_balance} tickets with balance");
    println!("  - {num_final_targets} final validator targets");

    let directed_stake_meta =
        get_directed_stake_meta(client.clone(), &steward_config, &program_id).await?;

    println!("\nValidator Targets:");
    for i in 0..directed_stake_meta.total_stake_targets as usize {
        let target = &directed_stake_meta.targets[i];
        if target.vote_pubkey != Pubkey::default() {
            println!("  Vote Pubkey: {}", target.vote_pubkey);
            println!(
                "    Target: {} lamports ({:.2} SOL)",
                target.total_target_lamports,
                target.total_target_lamports as f64 / LAMPORTS_PER_SOL as f64
            );
        }
    }

    Ok(())
}
