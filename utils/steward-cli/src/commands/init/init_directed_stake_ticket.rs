//! Directed Stake Ticket Account Initialization
//!
//! This command provides functionality to initialize the [`DirectedStakeTicket`] account
//! in the `jito_steward` program. This account stores metadata for managing directed
//! stake operations within the steward system.

use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::read_keypair_file;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use stakenet_sdk::utils::{
    accounts::{get_directed_stake_ticket_address, get_directed_stake_whitelist_address},
    transactions::{configure_instruction, print_base58_tx},
};

use crate::{
    commands::command_args::PermissionedParameters, utils::accounts::get_steward_config_account,
};

#[derive(Parser)]
#[command(about = "Initialize DirectedStakeTicket account")]
pub struct InitDirectedStakeTicket {
    #[command(flatten)]
    permissioned_parameters: PermissionedParameters,

    /// Ticket update authority pubkey
    #[arg(long, env)]
    ticket_update_authority: Pubkey,

    /// Whether the ticket holder is a protocol
    #[arg(long, env)]
    ticket_holder_is_protocol: bool,
}

pub async fn command_init_directed_stake_ticket(
    args: InitDirectedStakeTicket,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_config_pubkey = args.permissioned_parameters.steward_config;
    let authority_keypair = read_keypair_file(&args.permissioned_parameters.authority_keypair_path)
        .map_err(|e| anyhow::anyhow!("Failed to read keypair: {e}"))?;

    let authority_pubkey = if args.permissioned_parameters.transaction_parameters.print_tx {
        let config = get_steward_config_account(client, &steward_config_pubkey).await?;
        config.directed_stake_ticket_override_authority
    } else {
        authority_keypair.pubkey()
    };

    let directed_stake_whitelist_pda =
        get_directed_stake_whitelist_address(&steward_config_pubkey, &program_id);

    let directed_stake_ticket_pda = get_directed_stake_ticket_address(
        &steward_config_pubkey,
        &args.ticket_update_authority,
        &program_id,
    );

    println!("Initializing DirectedStakeTicket...");
    println!("  Authority: {authority_pubkey}");
    println!("  Steward Config: {}", steward_config_pubkey);
    println!(
        "  Ticket Update Authority: {}",
        args.ticket_update_authority
    );
    println!(
        "  Ticket Holder Is Protocol: {}",
        args.ticket_holder_is_protocol
    );
    println!("  DirectedStakeTicket PDA: {directed_stake_ticket_pda}");

    let instruction = Instruction {
        program_id,
        accounts: jito_steward::accounts::InitializeDirectedStakeTicket {
            config: steward_config_pubkey,
            whitelist_account: directed_stake_whitelist_pda,
            ticket_account: directed_stake_ticket_pda,
            system_program: solana_sdk::system_program::ID,
            signer: authority_pubkey,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::InitializeDirectedStakeTicket {
            ticket_update_authority: args.ticket_update_authority,
            ticket_holder_is_protocol: args.ticket_holder_is_protocol,
        }
        .data(),
    };

    let blockhash = client.get_latest_blockhash().await?;

    let configured_ix = configure_instruction(
        &[instruction],
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
        Some(&authority_pubkey),
        &[&authority_keypair],
        blockhash,
    );

    if args.permissioned_parameters.transaction_parameters.print_tx {
        print_base58_tx(&configured_ix)
    } else {
        let signature = client
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await?;

        println!("✅ DirectedStakeTicket initialized successfully!");
        println!("  Transaction signature: {signature}");
        println!("  DirectedStakeTicket account: {directed_stake_ticket_pda}");
    }

    Ok(())
}
