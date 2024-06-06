use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::{Config, Staker, StewardStateAccount, StewardStateEnum, UpdateParametersArgs};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;

use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};

use crate::{
    commands::commands::CrankComputeScore,
    utils::{
        accounts::{get_all_steward_accounts, UsefulStewardAccounts},
        print::state_tag_to_string,
    },
};

pub async fn command_crank_compute_score(
    args: CrankComputeScore,
    client: RpcClient,
    program_id: Pubkey,
) -> Result<()> {
    // Creates config account
    let payer =
        read_keypair_file(args.payer_keypair_path).expect("Failed reading keypair file ( Payer )");

    let steward_config = args.steward_config;

    let UsefulStewardAccounts {
        config_account,
        state_account,
        state_address,
        stake_pool_account,
        stake_pool_address,
        validator_list_account,
        validator_list_address,
    } = get_all_steward_accounts(&client, &program_id, &steward_config).await?;

    match state_account.state.state_tag {
        StewardStateEnum::ComputeScores => { /* Continue */ }
        _ => {
            println!(
                "State account is not in ComputeScores state: {}",
                state_tag_to_string(state_account.state.state_tag)
            );
            return Ok(());
        }
    }

    println!(
        "Validator List Account: {:?}",
        validator_list_account.validators.len()
    );

    // let init_ix = Instruction {
    //     program_id: program_id,
    //     accounts: jito_steward::accounts::ComputeScore {
    //         config: steward_config,
    //         state_account: state_address,
    //         validator_history: todo!(),
    //         validator_list: todo!(),
    //         cluster_history: todo!(),
    //         signer: payer.pubkey(),
    //     }
    //     .to_account_metas(None),
    //     data: jito_steward::instruction::ComputeScore {
    //         validator_list_index: args.validator_list_index,
    //     }
    //     .data(),
    // };

    // let blockhash = client.get_latest_blockhash().await?;

    // let transaction =
    //     Transaction::new_signed_with_payer(&[init_ix], Some(&payer.pubkey()), &[&payer], blockhash);

    // let signature = client
    //     .send_and_confirm_transaction_with_spinner(&transaction)
    //     .await?;

    Ok(())
}
