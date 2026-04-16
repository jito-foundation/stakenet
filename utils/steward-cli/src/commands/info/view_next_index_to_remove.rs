use std::sync::Arc;

use anyhow::Result;
use jito_steward::stake_pool_utils::ValidatorList;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use stakenet_sdk::{
    models::aggregate_accounts::AllStewardAccounts, utils::accounts::get_all_steward_accounts,
};

use crate::commands::command_args::ViewNextIndexToRemove;

pub async fn command_view_next_index_to_remove(
    args: ViewNextIndexToRemove,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &args.view_parameters.steward_config).await?;

    let state = &accounts.state_account.state;
    let validator_list = &accounts.validator_list_account;

    let mut found = false;
    for i in 0..state.num_pool_validators {
        let value = state.validators_to_remove.get_unsafe(i as usize);

        if value {
            let vote_account = get_vote_account(validator_list, i as usize);
            match vote_account {
                Some(pubkey) => {
                    println!("Validator {i} is marked for removal (vote account: {pubkey})")
                }
                None => println!("Validator {i} is marked for removal (vote account not found)"),
            }
            found = true;
        }
    }

    if !found {
        println!("No validators marked for removal");
    }

    Ok(())
}

fn get_vote_account(validator_list: &ValidatorList, index: usize) -> Option<Pubkey> {
    validator_list
        .validators
        .get(index)
        .map(|v| v.vote_account_address)
}
