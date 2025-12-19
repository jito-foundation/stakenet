//! Copy Directed Stake Targets
//!
//! This module provides functionality to manually copy directed stake targets to the
//! `DirectedStakeMeta` account. This allows an authority to set specific stake target
//! lamports for validators directly.

use std::sync::Arc;

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use anyhow::anyhow;
use clap::Parser;
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig, pubkey::Pubkey, signature::read_keypair_file,
    signer::Signer, transaction::Transaction,
};
use stakenet_sdk::utils::accounts::{
    get_directed_stake_meta_address, get_stake_pool_account, get_steward_config_account,
    get_validator_list_account,
};

use crate::{
    commands::command_args::{parse_pubkey, parse_u64, PermissionedParameters},
    utils::transactions::{configure_instruction, maybe_print_tx},
};

#[derive(Parser)]
#[command(about = "Copies directed stake targets to DirectedStakeMeta account")]
pub struct CopyDirectedStakeTargets {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,

    /// Vote accounts of validators to set directed stake targets for (comma-separated)
    ///
    /// Example: `--vote-pubkey Vote1111...,Vote2222...,Vote3333...`
    #[arg(long, value_delimiter = ',', value_parser = parse_pubkey)]
    pub vote_pubkey: Vec<Pubkey>,

    /// Target lamports for each validator (comma-separated)
    ///
    /// Must have the same length as `vote_pubkey`. Each value represents the
    /// target stake lamports for the corresponding validator.
    ///
    /// Example: `--target-lamports 1000000000,2000000000,3000000000`
    #[arg(long, env, value_delimiter = ',', value_parser = parse_u64)]
    pub target_lamports: Vec<u64>,
}

/// Copies directed stake targets to the DirectedStakeMeta account.
///
/// This function allows an authority to manually set the target lamports for
/// validators in the directed stake system. Each validator is assigned a specific
/// target lamports value.
pub(crate) async fn command_copy_directed_stake_targets(
    args: CopyDirectedStakeTargets,
    client: Arc<RpcClient>,
    program_id: Pubkey,
) -> anyhow::Result<()> {
    let steward_config = args.permissioned_parameters.steward_config;

    // Determine authority pubkey for the instruction
    let authority_pubkey = if args.permissioned_parameters.transaction_parameters.print_tx
        || args
            .permissioned_parameters
            .transaction_parameters
            .print_gov_tx
    {
        if let Some(pubkey) = args.permissioned_parameters.authority_pubkey {
            pubkey
        } else {
            // Fallback to reading on-chain config to get directed_stake_meta_upload_authority
            let config_account = client.get_account(&steward_config).await?;
            let config =
                jito_steward::Config::try_deserialize(&mut config_account.data.as_slice())?;
            config.directed_stake_meta_upload_authority
        }
    } else {
        read_keypair_file(&args.permissioned_parameters.authority_keypair_path)
            .expect("Failed reading keypair file ( Authority )")
            .pubkey()
    };

    if args.vote_pubkey.len() != args.target_lamports.len() {
        return Err(anyhow!(
            "Vote pubkeys and target lamports should have the same length"
        ));
    }

    if args.vote_pubkey.is_empty() {
        return Err(anyhow!("At least one vote pubkey must be provided"));
    }

    // Fetch accounts needed for building instructions
    let config_account = get_steward_config_account(&client, &steward_config).await?;
    let stake_pool_account = get_stake_pool_account(&client, &config_account.stake_pool).await?;
    let validator_list_address = stake_pool_account.validator_list;
    let validator_list_account =
        get_validator_list_account(&client, &validator_list_address).await?;

    let directed_stake_meta_pda = get_directed_stake_meta_address(&steward_config, &program_id);

    // Build instructions for each vote pubkey
    let mut instructions = Vec::new();
    for (vote_pubkey, target_lamports) in args.vote_pubkey.iter().zip(args.target_lamports.iter()) {
        // Find the index of this vote_pubkey in the validator list
        let validator_list_index = validator_list_account
            .validators
            .iter()
            .position(|v| v.vote_account_address == *vote_pubkey)
            .ok_or_else(|| anyhow!("Vote pubkey {vote_pubkey} not found in validator list"))?;

        let ix = solana_sdk::instruction::Instruction {
            program_id,
            accounts: jito_steward::accounts::CopyDirectedStakeTargets {
                config: steward_config,
                directed_stake_meta: directed_stake_meta_pda,
                authority: authority_pubkey,
                clock: solana_sdk::sysvar::clock::id(),
                validator_list: validator_list_address,
            }
            .to_account_metas(None),
            data: jito_steward::instruction::CopyDirectedStakeTargets {
                vote_pubkey: *vote_pubkey,
                total_target_lamports: *target_lamports,
                validator_list_index: validator_list_index as u32,
            }
            .data(),
        };

        instructions.push(ix);
    }

    println!(
        "Building {} CopyDirectedStakeTargets instruction(s)",
        instructions.len()
    );

    let configured_ixs = configure_instruction(
        &instructions,
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
        &configured_ixs,
        &args.permissioned_parameters.transaction_parameters,
    ) {
        return Ok(());
    }

    // Otherwise, send transaction signed by the authority
    let authority = read_keypair_file(&args.permissioned_parameters.authority_keypair_path)
        .map_err(|e| anyhow!("Failed to read keypair file: {e}"))?;

    let blockhash = client.get_latest_blockhash().await?;

    let transaction = Transaction::new_signed_with_payer(
        &configured_ixs,
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

    println!("Signature: {signature}");

    Ok(())
}
