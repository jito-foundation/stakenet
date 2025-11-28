use std::{path::PathBuf, sync::Arc};

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::{anyhow, Result};
use clap::Parser;
use jito_steward::DirectedStakeRecordType;
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig};
use solana_program::instruction::Instruction;
use solana_sdk::{
    commitment_config::CommitmentConfig, pubkey::Pubkey, signature::read_keypair_file,
    signer::Signer, transaction::Transaction,
};
use stakenet_sdk::utils::{
    accounts::get_directed_stake_whitelist_address,
    transactions::{configure_instruction, print_base58_tx},
};

use crate::commands::command_args::TransactionParameters;

#[derive(Parser)]
#[command(about = "Add to Directed stake whitelist")]
pub struct AddToDirectedStakeWhitelist {
    /// Steward config account
    #[arg(long)]
    pub steward_config: Pubkey,

    /// Record type
    #[arg(long)]
    pub record_type: String,

    /// Record
    #[arg(long)]
    pub record: Pubkey,

    /// Authority keypair path, also used as payer
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub authority_keypair_path: PathBuf,

    #[command(flatten)]
    pub transaction_parameters: TransactionParameters,
}

pub async fn command_add_to_directed_stake_whitelist(
    args: AddToDirectedStakeWhitelist,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let authority_keypair = read_keypair_file(&args.authority_keypair_path)
        .map_err(|e| anyhow::anyhow!("Failed to read keypair: {}", e))?;
    let authority_pubkey = authority_keypair.pubkey();

    let directed_stake_whitelist =
        get_directed_stake_whitelist_address(&args.steward_config, &program_id);

    let record_type = match args.record_type.as_str() {
        "validator" => DirectedStakeRecordType::Validator,
        "protocol" => DirectedStakeRecordType::Protocol,
        "user" => DirectedStakeRecordType::User,
        record_type => return Err(anyhow!("Failed to read record type: {record_type}")),
    };

    let instruction = Instruction {
        program_id,
        accounts: jito_steward::accounts::AddToDirectedStakeWhitelist {
            config: args.steward_config,
            directed_stake_whitelist,
            authority: authority_pubkey,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::AddToDirectedStakeWhitelist {
            record_type,
            record: args.record,
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

        println!("✅ Added to directed stake whitelist successfully!");
        println!("  Transaction signature: {signature}");
    }

    Ok(())
}
