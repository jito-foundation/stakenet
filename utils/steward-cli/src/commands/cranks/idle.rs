use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::StewardStateEnum;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;

use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};

use crate::{commands::command_args::CrankIdle, utils::accounts::get_all_steward_accounts};

pub async fn command_crank_idle(
    args: CrankIdle,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let args = args.permissionless_parameters;

    // Creates config account
    let payer =
        read_keypair_file(args.payer_keypair_path).expect("Failed reading keypair file ( Payer )");

    let steward_config = args.steward_config;

    let steward_accounts = get_all_steward_accounts(client, &program_id, &steward_config).await?;

    match steward_accounts.state_account.state.state_tag {
        StewardStateEnum::Idle => { /* Continue */ }
        _ => {
            println!(
                "State account is not in Idle state: {}",
                steward_accounts.state_account.state.state_tag
            );
            return Ok(());
        }
    }

    let ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::Idle {
            config: steward_config,
            state_account: steward_accounts.state_address,
            validator_list: steward_accounts.validator_list_address,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::Idle {}.data(),
    };

    let blockhash = client.get_latest_blockhash().await?;

    let transaction =
        Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[&payer], blockhash);

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .await?;

    println!("Signature: {}", signature);

    Ok(())
}
