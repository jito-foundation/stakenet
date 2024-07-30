use std::sync::Arc;

use anyhow::Result;
use jito_steward::StewardStateAccount;
use solana_client::nonblocking::rpc_client::RpcClient;

use solana_sdk::pubkey::Pubkey;

use crate::commands::command_args::ViewNextIndexToRemove;
use stakenet_sdk::utils::accounts::get_steward_state_account;

pub async fn command_view_next_index_to_remove(
    args: ViewNextIndexToRemove,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_state_account =
        get_steward_state_account(client, &program_id, &args.view_parameters.steward_config)
            .await?;

    _print_next_index_to_remove(&steward_state_account);

    Ok(())
}

fn _print_next_index_to_remove(state_account: &StewardStateAccount) {
    for i in 0..state_account.state.num_pool_validators {
        let value = state_account
            .state
            .validators_to_remove
            .get_unsafe(i as usize);

        if value {
            println!("Validator {} is marked for removal", i);
            return;
        }
    }
}
