//! Directed Stake Whitelist Account Reallocation
//!
//! This module provides functionality to reallocate the [`DirectedStakeWhitelist`] account
//! in the `jito_steward` program. The reallocation process incrementally grows the account
//! to its required size by batching multiple reallocation instructions into transactions.

use std::sync::Arc;

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use anyhow::Result;
use clap::Parser;
use jito_steward::{constants::MAX_ALLOC_BYTES, DirectedStakeWhitelist};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature},
    signer::Signer,
    transaction::Transaction,
};
use stakenet_sdk::utils::{
    accounts::{
        get_directed_stake_whitelist_address, get_stake_pool_account, get_steward_config_account,
    },
    transactions::{configure_instruction, print_base58_tx},
};

use crate::commands::{command_args::PermissionedParameters, init::REALLOCS_PER_TX};

#[derive(Parser)]
#[command(about = "Reallocate Directed Stake Whitelist account")]
pub struct ReallocDirectedStakeWhitelist {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,
}

/// Reallocates the [`DirectedStakeWhitelist`] account to its target size.
pub async fn command_realloc_directed_stake_whitelist(
    args: ReallocDirectedStakeWhitelist,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let authority = read_keypair_file(args.permissioned_parameters.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let steward_config = args.permissioned_parameters.steward_config;
    let steward_config_account =
        get_steward_config_account(client, &args.permissioned_parameters.steward_config).await?;

    let stake_pool_account =
        get_stake_pool_account(client, &steward_config_account.stake_pool).await?;

    let validator_list = stake_pool_account.validator_list;

    let directed_staking_whitelist =
        get_directed_stake_whitelist_address(&steward_config, &program_id);
    let directed_stake_whitelist_account_raw =
        client.get_account(&directed_staking_whitelist).await?;
    if directed_stake_whitelist_account_raw
        .data
        .len()
        .eq(&DirectedStakeWhitelist::SIZE)
    {
        match DirectedStakeWhitelist::try_deserialize(
            &mut directed_stake_whitelist_account_raw.data.as_slice(),
        ) {
            Ok(_) => {
                println!("Directed Stake Whitelist account already exists");
                return Ok(());
            }
            Err(_) => { /* Account is not initialized, continue */ }
        };
    }

    let data_length = directed_stake_whitelist_account_raw.data.len();
    let whats_left = DirectedStakeWhitelist::SIZE - data_length.min(DirectedStakeWhitelist::SIZE);

    let mut reallocs_left_to_run = (whats_left + MAX_ALLOC_BYTES - 1) / MAX_ALLOC_BYTES;

    let reallocs_to_run = reallocs_left_to_run;
    let mut reallocs_ran = 0;

    while reallocs_left_to_run > 0 {
        let reallocs_per_transaction = reallocs_left_to_run.min(REALLOCS_PER_TX);

        let signature = _realloc_x_times(
            client,
            &program_id,
            &authority,
            directed_staking_whitelist,
            &steward_config,
            &validator_list,
            reallocs_per_transaction,
            args.permissioned_parameters
                .transaction_parameters
                .priority_fee,
            args.permissioned_parameters
                .transaction_parameters
                .compute_limit,
            args.permissioned_parameters
                .transaction_parameters
                .heap_size,
            args.permissioned_parameters.transaction_parameters.print_tx,
        )
        .await?;

        reallocs_left_to_run -= reallocs_per_transaction;
        reallocs_ran += reallocs_per_transaction;

        println!(
            "{}/{}: Signature: {}",
            reallocs_ran, reallocs_to_run, signature
        );
    }

    Ok(())
}

/// Creates and submits a transaction containing multiple reallocation instructions.
#[allow(clippy::too_many_arguments)]
async fn _realloc_x_times(
    client: &RpcClient,
    program_id: &Pubkey,
    authority: &Keypair,
    directed_stake_whitelist: Pubkey,
    steward_config: &Pubkey,
    validator_list: &Pubkey,
    count: usize,
    priority_fee: Option<u64>,
    compute_limit: Option<u32>,
    heap_size: Option<u32>,
    print_tx: bool,
) -> Result<Signature> {
    let ixs = vec![
        Instruction {
            program_id: *program_id,
            accounts: jito_steward::accounts::ReallocDirectedStakeWhitelist {
                directed_stake_whitelist,
                config: *steward_config,
                validator_list: *validator_list,
                system_program: anchor_lang::solana_program::system_program::id(),
                signer: authority.pubkey(),
            }
            .to_account_metas(None),
            data: jito_steward::instruction::ReallocDirectedStakeWhitelist {}.data(),
        };
        count
    ];

    let blockhash = client.get_latest_blockhash().await?;

    let configured_ix = configure_instruction(&ixs, priority_fee, compute_limit, heap_size);

    let transaction = Transaction::new_signed_with_payer(
        &configured_ix,
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    let mut signature = Signature::default();
    if print_tx {
        print_base58_tx(&configured_ix);
    } else {
        signature = client
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await?;

        println!("Signature: {}", signature);
    }

    Ok(signature)
}
