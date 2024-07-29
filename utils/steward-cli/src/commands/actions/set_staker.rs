use std::sync::Arc;

use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;

use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};
use spl_stake_pool::instruction::set_staker;

use crate::{
    commands::command_args::SetStaker,
    utils::{
        accounts::get_all_steward_accounts,
        transactions::{configure_instruction, print_base58_tx},
    },
};

pub async fn command_set_staker(
    args: SetStaker,
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

    let set_staker_ix = set_staker(
        &spl_stake_pool::id(),
        &all_steward_accounts.stake_pool_address,
        &authority.pubkey(),
        &all_steward_accounts.state_address,
    );

    let blockhash = client.get_latest_blockhash().await?;

    let configured_ix = configure_instruction(
        &[set_staker_ix],
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

        println!("Signature: {}", signature);
    }

    Ok(())
}
