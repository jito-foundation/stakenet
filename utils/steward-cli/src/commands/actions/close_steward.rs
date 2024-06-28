use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;

use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};

use crate::{
    commands::command_args::CloseSteward,
    utils::{accounts::get_all_steward_accounts, transactions::configure_instruction},
};

pub async fn command_close_steward(
    args: CloseSteward,
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

    let set_staker_ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::SetStaker {
            config: all_steward_accounts.config_address,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: all_steward_accounts.stake_pool_address,
            staker: all_steward_accounts.staker_address,
            new_staker: authority.pubkey(),
            signer: authority.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetStaker {}.data(),
    };

    let close_steward_ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::CloseStewardAccounts {
            config: all_steward_accounts.config_address,
            authority: authority.pubkey(),
            staker: all_steward_accounts.staker_address,
            state_account: all_steward_accounts.state_address,
        }
        .to_account_metas(None),
        data: jito_steward::instruction::CloseStewardAccounts {}.data(),
    };

    let blockhash = client.get_latest_blockhash().await?;

    let configured_ix = configure_instruction(
        &[set_staker_ix, close_steward_ix],
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
