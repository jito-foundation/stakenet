use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::UpdateParametersArgs;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};

use crate::utils::accounts::get_all_steward_accounts;

use super::commands::SetStaker;

pub async fn command_set_staker(
    args: SetStaker,
    client: RpcClient,
    program_id: Pubkey,
) -> Result<()> {
    // Creates config account
    let authority = read_keypair_file(args.authority_keypair_path)
        .expect("Failed reading keypair file ( Authority )");

    let all_steward_accounts =
        get_all_steward_accounts(&client, &program_id, &args.steward_config).await?;

    let set_staker_ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::SetStaker {
            config: args.steward_config,
            stake_pool_program: spl_stake_pool::id(),
            stake_pool: all_steward_accounts.stake_pool_address,
            staker: all_steward_accounts.staker_address,
            new_staker: authority.pubkey(),
            signer: authority.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::SetStaker {}.data(),
    };
    let close_steward_state_ix = Instruction {
        program_id,
        accounts: jito_steward::accounts::CloseStewardAccounts {
            config: args.steward_config,
            staker: all_steward_accounts.staker_address,
            state_account: all_steward_accounts.state_address,
            authority: authority.pubkey(),
        }
        .to_account_metas(None),
        data: jito_steward::instruction::CloseStewardAccounts {}.data(),
    };

    let blockhash = client
        .get_latest_blockhash()
        .await
        .expect("Failed to get recent blockhash");

    let transaction = Transaction::new_signed_with_payer(
        &[set_staker_ix, close_steward_state_ix],
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
