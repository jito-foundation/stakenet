use std::{
    collections::{HashMap, HashSet},
    error::Error,
    str::FromStr,
    sync::Arc,
};

use keeper_core::{
    get_multiple_accounts_batched, get_vote_accounts_with_retry, submit_transactions,
};
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_response::RpcVoteAccountInfo};
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    vote,
};
use validator_history::{constants::MIN_VOTE_EPOCHS, ValidatorHistory};

use crate::{
    derive_validator_history_address, get_balance_with_retry,
    get_create_validator_history_instructions, get_validator_history_accounts_with_retry,
    operations::keeper_operations::KeeperOperations,
};

use super::keeper_state::KeeperState;

pub async fn update_state(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    keeper_state: &mut KeeperState,
) -> Result<(), Box<dyn Error>> {
    // Update Epoch
    match client.get_epoch_info().await {
        Ok(current_epoch) => {
            if current_epoch.epoch != keeper_state.epoch_info.epoch {
                keeper_state.runs_for_epoch = [0; KeeperOperations::LEN];
                keeper_state.errors_for_epoch = [0; KeeperOperations::LEN];
                keeper_state.epoch_info = current_epoch.clone();
            }
        }
        Err(e) => {
            return Err(Box::new(e));
        }
    }

    // Fetch Vote Accounts
    match get_vote_account_map(client).await {
        Ok(vote_account_map) => {
            keeper_state.vote_account_map = vote_account_map;
        }
        Err(e) => {
            return Err(e);
        }
    }

    // Set Closed Vote Accounts
    match get_closed_vote_accounts(client, keeper_state).await {
        Ok(closed_vote_accounts) => {
            keeper_state.closed_vote_accounts = closed_vote_accounts;
        }
        Err(e) => {
            return Err(e);
        }
    }

    // Create Missing Accounts
    create_missing_validator_history_accounts(client, keypair, program_id, &keeper_state).await?;

    // Fetch Validator History Accounts
    match get_validator_history_map(client, program_id).await {
        Ok(validator_history_map) => {
            keeper_state.validator_history_map = validator_history_map;
        }
        Err(e) => {
            return Err(e);
        }
    }

    // Update Keeper Balance
    match get_balance_with_retry(client, keypair.pubkey()).await {
        Ok(keeper_balance) => {
            keeper_state.keeper_balance = keeper_balance;
        }
        Err(e) => {
            return Err(Box::new(e));
        }
    }

    Ok(())
}

async fn get_vote_account_map(
    client: &Arc<RpcClient>,
) -> Result<HashMap<Pubkey, RpcVoteAccountInfo>, Box<dyn Error>> {
    let active_vote_accounts = HashMap::from_iter(
        get_vote_accounts_with_retry(client, MIN_VOTE_EPOCHS, None)
            .await?
            .iter()
            .map(|vote_account_info| {
                (
                    Pubkey::from_str(vote_account_info.vote_pubkey.as_str())
                        .expect("Could not parse vote pubkey"),
                    vote_account_info.clone(),
                )
            }),
    );

    Ok(active_vote_accounts)
}

async fn get_closed_vote_accounts(
    client: &Arc<RpcClient>,
    keeper_state: &KeeperState,
) -> Result<HashSet<Pubkey>, Box<dyn Error>> {
    let vote_account_pubkeys = &keeper_state
        .validator_history_map
        .clone()
        .into_values()
        .map(|validator_history| validator_history.vote_account)
        .collect::<Vec<_>>();

    let vote_accounts = get_multiple_accounts_batched(&vote_account_pubkeys, client).await?;
    let closed_vote_accounts: HashSet<Pubkey> = vote_accounts
        .iter()
        .enumerate()
        .filter_map(|(i, account)| match account {
            Some(account) => {
                if account.owner != vote::program::id() {
                    Some(vote_account_pubkeys[i])
                } else {
                    None
                }
            }
            None => Some(vote_account_pubkeys[i]),
        })
        .collect();

    Ok(closed_vote_accounts)
}

async fn get_validator_history_map(
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
) -> Result<HashMap<Pubkey, ValidatorHistory>, Box<dyn Error>> {
    let validator_histories =
        get_validator_history_accounts_with_retry(&client, program_id.clone()).await?;

    let validator_history_map = HashMap::from_iter(
        validator_histories
            .iter()
            .map(|vote_history| (vote_history.vote_account, vote_history.clone())),
    );

    Ok(validator_history_map)
}

async fn create_missing_validator_history_accounts(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    keeper_state: &KeeperState,
) -> Result<(), Box<dyn Error>> {
    let vote_accounts = &keeper_state
        .vote_account_map
        .keys()
        .collect::<Vec<&Pubkey>>();

    let all_history_addresses = &vote_accounts
        .iter()
        .map(|vote_pubkey| derive_validator_history_address(vote_pubkey, program_id))
        .collect::<Vec<Pubkey>>();

    let history_accounts = get_multiple_accounts_batched(&all_history_addresses, client).await?;

    assert!(vote_accounts.len() == history_accounts.len());

    let create_transactions = vote_accounts
        .iter()
        .zip(history_accounts)
        .filter_map(|(vote_pubkey, history_account)| {
            match history_account {
                Some(_) => None,
                None => {
                    // Create accounts that don't exist
                    let ix =
                        get_create_validator_history_instructions(vote_pubkey, program_id, keypair);
                    Some(ix)
                }
            }
        })
        .collect::<Vec<Vec<Instruction>>>();

    submit_transactions(client, create_transactions, keypair).await?;

    Ok(())
}
