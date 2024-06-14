use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::UpdateParametersArgs;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};

use crate::commands::commands::UpdateConfig;

pub async fn command_update_config(
    args: UpdateConfig,
    client: RpcClient,
    program_id: Pubkey,
) -> Result<()> {
    // Creates config account
    let authority = read_keypair_file(args.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let steward_config = args.steward_config;

    let update_parameters_args: UpdateParametersArgs =
        args.config_parameters.to_update_parameters_args();

    let update_ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::UpdateParameters {
            config: steward_config,
            authority: authority.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::UpdateParameters {
            update_parameters_args,
        }
        .data(),
    };

    let blockhash = client
        .get_latest_blockhash()
        .await
        .expect("Failed to get recent blockhash");

    let transaction = Transaction::new_signed_with_payer(
        &[update_ix],
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .await
        .expect("Failed to send transaction");
    println!("Signature: {}", signature);

    Ok(())
}
