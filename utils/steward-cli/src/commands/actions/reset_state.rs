use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;

use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};

use crate::{
    commands::command_args::ResetState,
    utils::{accounts::get_all_steward_accounts, transactions::configure_instruction},
};

pub async fn command_reset_state(
    args: ResetState,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    // Creates config account
    let authority = read_keypair_file(args.permissioned_parameters.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let all_steward_accounts = get_all_steward_accounts(
        client,
        &program_id,
        &args.permissioned_parameters.steward_config,
    )
    .await?;

    let ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::ResetStewardState {
            state_account: all_steward_accounts.state_address,
            config: args.permissioned_parameters.steward_config,
            stake_pool: all_steward_accounts.stake_pool_address,
            validator_list: all_steward_accounts.validator_list_address,
            authority: authority.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::ResetStewardState {}.data(),
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

    let signature = client
        .send_and_confirm_transaction_with_spinner(&transaction)
        .await?;

    println!("Signature: {}", signature);

    Ok(())
}
