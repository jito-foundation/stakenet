use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;

use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};

use crate::{
    commands::command_args::CrankEpochMaintenance, utils::accounts::get_all_steward_accounts,
};

pub async fn command_crank_epoch_maintenance(
    args: CrankEpochMaintenance,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let validator_index_to_remove = args.validator_index_to_remove;
    let args = args.permissionless_parameters;

    // Creates config account
    let payer =
        read_keypair_file(args.payer_keypair_path).expect("Failed reading keypair file ( Payer )");

    let steward_config = args.steward_config;

    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let epoch = client.get_epoch_info().await?.epoch;

    if epoch == all_steward_accounts.state_account.state.current_epoch {
        println!("Epoch is the same as the current epoch: {}", epoch);
        return Ok(());
    }

    let ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::EpochMaintenance {
            config: steward_config,
            state_account: all_steward_accounts.state_address,
            validator_list: all_steward_accounts.validator_list_address,
            stake_pool: all_steward_accounts.stake_pool_address,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::EpochMaintenance {
            validator_index_to_remove,
        }
        .data(),
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
