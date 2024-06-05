use anchor_lang::{InstructionData, ToAccountMetas};
use jito_steward::UpdateParametersArgs;
use solana_client::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{signature::read_keypair_file, signer::Signer, transaction::Transaction};

use super::commands::UpdateConfig;

pub fn command_update_config(args: UpdateConfig, client: RpcClient) {
    // Creates config account
    let authority = read_keypair_file(args.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let steward_config = args.steward_config;

    let update_parameters_args: UpdateParametersArgs =
        args.config_parameters.to_update_parameters_args();

    let update_ix = Instruction {
        program_id: jito_steward::id(),
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
        .expect("Failed to get recent blockhash");

    let transaction = Transaction::new_signed_with_payer(
        &[update_ix],
        Some(&authority.pubkey()),
        &[&authority],
        blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .expect("Failed to send transaction");
    println!("Signature: {}", signature);
}
