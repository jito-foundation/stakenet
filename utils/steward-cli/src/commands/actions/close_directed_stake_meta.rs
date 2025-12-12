use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};
use stakenet_sdk::utils::{
    accounts::{get_all_steward_accounts, get_directed_stake_meta_address},
    transactions::{configure_instruction, print_base58_tx},
};

use crate::commands::command_args::PermissionedParameters;

#[derive(Parser)]
#[command(about = "Closes the directed stake meta account")]
pub struct CloseDirectedStakeMeta {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,
}

pub async fn command_close_directed_stake_meta(
    args: CloseDirectedStakeMeta,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let authority = read_keypair_file(args.permissioned_parameters.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let all_steward_accounts = get_all_steward_accounts(
        client,
        &program_id,
        &args.permissioned_parameters.steward_config,
    )
    .await?;

    let directed_stake_meta =
        get_directed_stake_meta_address(&all_steward_accounts.config_address, &program_id);

    let ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::CloseDirectedStakeMeta {
            config: all_steward_accounts.config_address,
            directed_stake_meta,
            authority: authority.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::CloseDirectedStakeMeta {}.data(),
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

    if args.permissioned_parameters.transaction_parameters.print_tx {
        print_base58_tx(&configured_ix)
    } else {
        let signature = client
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await?;

        println!("Signature: {signature}");
    }

    Ok(())
}
