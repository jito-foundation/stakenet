use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::{anyhow, Result};
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};

use crate::{
    commands::command_args::PermissionedParameters,
    utils::{
        accounts::{
            get_steward_config_account, get_steward_state_address, get_validator_list_account,
        },
        transactions::configure_instruction,
    },
};

#[derive(Parser)]
#[command(about = "Mark the correct validator for removal")]
pub struct AdminMarkForRemoval {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,

    /// Set to true to mark the validator for removal, false to unmark
    #[arg(long)]
    pub mark_for_removal: bool,

    /// If true, remove the validator immediately
    #[arg(long)]
    pub immediate: bool,

    /// The vote account address of the validator to mark for removal
    #[arg(long)]
    pub validator_vote_account: Pubkey,

    /// If true, submit the transaction to the network. If false, only print the details
    #[arg(long, default_value = "false")]
    pub submit_ix: bool,
}

pub async fn command_admin_mark_for_removal(
    args: AdminMarkForRemoval,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let mark_for_removal: u8 = {
        if args.mark_for_removal {
            0xFF // TRUE
        } else {
            0x00 // FALSE
        }
    };
    let immediate: u8 = {
        if args.immediate {
            0xFF // TRUE
        } else {
            0x00 // FALSE
        }
    };

    let authority = read_keypair_file(args.permissioned_parameters.authority_keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file ( Authority ): {e}"))?;

    let steward_config_address = args.permissioned_parameters.steward_config;
    let steward_state_address = get_steward_state_address(&program_id, &steward_config_address);

    let steward_config_account =
        get_steward_config_account(client, &steward_config_address).await?;
    let validator_list_account =
        get_validator_list_account(client, &steward_config_account.validator_list).await?;

    println!("Submit: {}", args.submit_ix);

    let validator_list_index = validator_list_account
        .validators
        .iter()
        .position(|v| v.vote_account_address == args.validator_vote_account)
        .map(|i| i as u64)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Validator {} not found in validator list",
                args.validator_vote_account
            )
        })?;

    println!("Validator list index: {validator_list_index}");
    println!("Mark for removal: {mark_for_removal}");
    println!("Immediate: {immediate}");
    println!("Validator to mark: {:?}", args.validator_vote_account);

    if args.submit_ix {
        let ix = Instruction {
            program_id,
            accounts: jito_steward::accounts::AdminMarkForRemoval {
                state_account: steward_state_address,
                config: steward_config_address,
                authority: authority.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::AdminMarkForRemoval {
                validator_list_index,
                mark_for_removal,
                immediate,
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

        let signature = client
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await?;

        println!("Signature: {}", signature);
    }

    Ok(())
}
