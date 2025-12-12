use std::sync::Arc;

use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig, pubkey::Pubkey, signature::read_keypair_file,
    signer::Signer, transaction::Transaction,
};
use stakenet_sdk::utils::accounts::get_all_steward_accounts;

use crate::commands::command_args::UpdateValidatorListBalance;
#[allow(deprecated)]
use spl_stake_pool::instruction::update_validator_list_balance;

pub async fn command_update_validator_list_balance(
    client: &Arc<RpcClient>,
    args: UpdateValidatorListBalance,
    program_id: Pubkey,
) -> Result<(), anyhow::Error> {
    let steward_config = args.permissionless_parameters.steward_config;
    let payer = Arc::new(
        read_keypair_file(args.permissionless_parameters.payer_keypair_path)
            .expect("Failed reading keypair file ( Payer )"),
    );

    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let stake_pool = all_steward_accounts.stake_pool_address;
    let validator_list = all_steward_accounts.validator_list_address;

    let target_vote_account = all_steward_accounts.validator_list_account.validators
        [args.validator_list_index as usize]
        .vote_account_address;

    #[allow(deprecated)]
    let instruction = update_validator_list_balance(
        &spl_stake_pool::id(),
        &stake_pool,
        &all_steward_accounts.stake_pool_withdraw_authority,
        &validator_list,
        &all_steward_accounts.stake_pool_account.reserve_stake,
        &all_steward_accounts.validator_list_account,
        &[target_vote_account],
        args.validator_list_index,
        false,
    );

    let recent_blockhash = client.get_latest_blockhash().await?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&payer.pubkey()),
        &[&*payer],
        recent_blockhash,
    );

    let signature = client
        .send_and_confirm_transaction_with_spinner_and_config(
            &transaction,
            CommitmentConfig::confirmed(),
            RpcSendTransactionConfig::default(),
        )
        .await?;

    println!("Transaction signature: {signature}");

    Ok(())
}
