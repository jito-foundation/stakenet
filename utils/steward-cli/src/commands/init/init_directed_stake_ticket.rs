//! Directed Stake Ticket Account Initialization
//!
//! This command provides functionality to initialize the [`DirectedStakeTicket`] account
//! in the `jito_steward` program. This account stores metadata for managing directed
//! stake operations within the steward system.

use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::state::directed_stake::{DirectedStakeTicket, DirectedStakeWhitelist};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::read_keypair_file;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use stakenet_sdk::utils::transactions::{configure_instruction, print_base58_tx};

use crate::commands::command_args::InitDirectedStakeTicket;

pub async fn command_init_directed_stake_ticket(
    args: InitDirectedStakeTicket,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let authority_keypair = read_keypair_file(&args.authority_keypair_path)
        .map_err(|e| anyhow::anyhow!("Failed to read keypair: {}", e))?;
    let authority_pubkey = authority_keypair.pubkey();

    let (directed_stake_whitelist_pda, _bump) = Pubkey::find_program_address(
        &[DirectedStakeWhitelist::SEED, args.steward_config.as_ref()],
        &program_id,
    );

    let (directed_stake_ticket_pda, _bump) = Pubkey::find_program_address(
        &[DirectedStakeTicket::SEED, authority_pubkey.as_ref()],
        &program_id,
    );

    println!("Initializing DirectedStakeTicket...");
    println!("  Authority: {}", authority_pubkey);
    println!("  Steward Config: {}", args.steward_config);
    println!(
        "  Ticket Update Authority: {}",
        args.ticket_update_authority
    );
    println!(
        "  Ticket Holder Is Protocol: {}",
        args.ticket_holder_is_protocol
    );
    println!("  DirectedStakeTicket PDA: {}", directed_stake_ticket_pda);

    let instruction = Instruction {
        program_id,
        accounts: jito_steward::accounts::InitializeDirectedStakeTicket {
            config: args.steward_config,
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
        args.transaction_parameters.priority_fee,
        args.transaction_parameters.compute_limit,
        args.transaction_parameters.heap_size,
    );

    let transaction = Transaction::new_signed_with_payer(
        &configured_ix,
        Some(&authority_pubkey),
        &[&authority_keypair],
        blockhash,
    );

    if args.transaction_parameters.print_tx {
        print_base58_tx(&configured_ix)
    } else {
        let signature = client
            .send_and_confirm_transaction_with_spinner_and_config(
                &transaction,
                CommitmentConfig::processed(),
                RpcSendTransactionConfig::default(),
            )
            .await?;

        println!("✅ DirectedStakeTicket initialized successfully!");
        println!("  Transaction signature: {}", signature);
        println!(
            "  DirectedStakeTicket account: {}",
            directed_stake_ticket_pda
        );
    }

    Ok(())
}
