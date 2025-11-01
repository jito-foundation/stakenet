use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};
use stakenet_sdk::utils::{accounts::get_steward_state_address, transactions::print_base58_tx};

use crate::commands::command_args::PermissionedParameters;

#[derive(Parser)]
#[command(about = "Initialize state account")]
pub struct MigrateStateToV2 {
    #[command(flatten)]
    pub permissioned_parameters: PermissionedParameters,
}

pub async fn command_migrate_state_to_v2(
    args: MigrateStateToV2,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let authority = read_keypair_file(args.permissioned_parameters.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let steward_config = args.permissioned_parameters.steward_config;

    let steward_state = get_steward_state_address(&program_id, &steward_config);

    let ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::MigrateStateToV2 {
            state_account: steward_state,
            config: steward_config,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::MigrateStateToV2 {}.data(),
    };

    let blockhash = client.get_latest_blockhash().await?;

    let transaction = Transaction::new_signed_with_payer(
        &[ix.clone()],
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    if args.permissioned_parameters.transaction_parameters.print_tx {
        print_base58_tx(&[ix])
    } else {
        let signature = client
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await?;

        println!("Signature: {}", signature);
    }

    Ok(())
}
