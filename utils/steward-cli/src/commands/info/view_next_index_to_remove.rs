use std::sync::Arc;

use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;

use solana_sdk::pubkey::Pubkey;

use crate::{
    commands::command_args::ViewNextIndexToRemove,
    utils::accounts::{get_all_steward_accounts, UsefulStewardAccounts},
};

pub async fn command_view_next_index_to_remove(
    args: ViewNextIndexToRemove,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &args.view_parameters.steward_config).await?;

    _print_next_index_to_remove(&all_steward_accounts);

    Ok(())
}

fn _print_next_index_to_remove(steward_state_accounts: &UsefulStewardAccounts) {
    for i in 0..steward_state_accounts
        .state_account
        .state
        .num_pool_validators as usize
    {
        let value = steward_state_accounts
            .state_account
            .state
            .validators_to_remove
            .get_unsafe(i);

        if value {
            println!("Validator {} is marked for removal", i);
            return;
        }
    }
}
