use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};

use crate::commands::command_args::UpdateAuthority;

use stakenet_sdk::utils::transactions::{configure_instruction, print_base58_tx};

pub async fn command_update_authority(
    args: UpdateAuthority,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let (permissioned_parameters, new_authority, authority_type) = match args.command {
        crate::commands::command_args::AuthoritySubcommand::Blacklist {
            permissioned_parameters,
            new_authority,
        } => (
            permissioned_parameters,
            new_authority,
            jito_steward::instructions::set_new_authority::AuthorityType::SetBlacklistAuthority,
        ),
        crate::commands::command_args::AuthoritySubcommand::Admin {
            permissioned_parameters,
            new_authority,
        } => (
            permissioned_parameters,
            new_authority,
            jito_steward::instructions::set_new_authority::AuthorityType::SetAdmin,
        ),
        crate::commands::command_args::AuthoritySubcommand::Parameters {
            permissioned_parameters,
            new_authority,
        } => (
            permissioned_parameters,
            new_authority,
            jito_steward::instructions::set_new_authority::AuthorityType::SetParametersAuthority,
        ),
    };

    // Creates config account
    let authority = read_keypair_file(permissioned_parameters.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let steward_config = permissioned_parameters.steward_config;

    let ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::SetNewAuthority {
            config: steward_config,
            new_authority,
            admin: authority.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetNewAuthority { authority_type }.data(),
    };

    let blockhash = client
        .get_latest_blockhash()
        .await
        .expect("Failed to get recent blockhash");

    let configured_ix = configure_instruction(
        &[ix],
        permissioned_parameters.transaction_parameters.priority_fee,
        permissioned_parameters.transaction_parameters.compute_limit,
        permissioned_parameters.transaction_parameters.heap_size,
    );

    let transaction = Transaction::new_signed_with_payer(
        &configured_ix,
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    if permissioned_parameters.transaction_parameters.print_tx {
        print_base58_tx(&configured_ix)
    } else {
        let signature = client
            .send_and_confirm_transaction_with_spinner(&transaction)
            .await?;

        println!("Signature: {}", signature);
    }

    Ok(())
}
