use std::sync::Arc;

use solana_client::nonblocking::rpc_client::RpcClient;

use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file};

use stakenet_keeper::entries::crank_steward::crank_steward;
use stakenet_sdk::utils::accounts::get_all_steward_accounts;
use stakenet_sdk::utils::accounts::{
    get_all_steward_validator_accounts, get_all_validator_accounts,
};
use stakenet_sdk::utils::transactions::get_vote_accounts_with_retry;

use crate::commands::command_args::CrankSteward;

// Only runs one set of commands per "crank"
pub async fn command_crank_steward(
    args: CrankSteward,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<(), anyhow::Error> {
    // ----------- Collect Accounts -------------
    let steward_config = args.permissionless_parameters.steward_config;
    let payer = Arc::new(
        read_keypair_file(args.permissionless_parameters.payer_keypair_path)
            .expect("Failed reading keypair file ( Payer )"),
    );

    let priority_fee = args
        .permissionless_parameters
        .transaction_parameters
        .priority_fee;

    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let all_steward_validator_accounts =
        get_all_steward_validator_accounts(client, &all_steward_accounts, &validator_history::id())
            .await?;

    let all_active_vote_accounts = get_vote_accounts_with_retry(client, 5, None).await?;

    let all_active_validator_accounts =
        get_all_validator_accounts(client, &all_active_vote_accounts, &validator_history::id())
            .await?;

    let epoch = client.get_epoch_info().await?.epoch;

    let _ = crank_steward(
        client,
        &payer,
        &program_id,
        epoch,
        &all_steward_accounts,
        &all_steward_validator_accounts,
        &all_active_validator_accounts,
        priority_fee,
        &args.token_mint,
    )
    .await?;

    Ok(())
}
