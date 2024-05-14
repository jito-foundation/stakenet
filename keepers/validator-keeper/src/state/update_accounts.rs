use std::{collections::HashMap, error::Error, str::FromStr, sync::Arc};

use anchor_lang::AccountDeserialize;
use keeper_core::{
    get_multiple_accounts_batched, get_vote_accounts_with_retry, submit_transactions,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey, signature::Keypair};
use validator_history::{constants::MIN_VOTE_EPOCHS, ValidatorHistory};

use crate::{derive_validator_history_address, get_create_validator_history_instructions};

use super::keeper_state::KeeperState;

pub async fn update_validator_history_map(
    client: &Arc<RpcClient>,
    keypair: &Arc<Keypair>,
    program_id: &Pubkey,
    loop_state: &mut KeeperState,
) -> Result<(), Box<dyn Error>> {
    // Fetch all validator history accounts

    let active_vote_accounts = get_vote_accounts_with_retry(client, MIN_VOTE_EPOCHS, None)
        .await?
        .iter()
        .map(|vote_account_info| {
            Pubkey::from_str(vote_account_info.vote_pubkey.as_str())
                .expect("Could not parse vote pubkey")
        })
        .collect::<Vec<Pubkey>>();

    let all_history_addresses = &active_vote_accounts
        .iter()
        .map(|vote_pubkey| derive_validator_history_address(vote_pubkey, program_id))
        .collect::<Vec<Pubkey>>();

    let history_accounts = get_multiple_accounts_batched(&all_history_addresses, client).await?;

    assert!(active_vote_accounts.len() == history_accounts.len());

    let create_transactions = active_vote_accounts
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

    // Update the validator history map
    let validator_history_map = get_multiple_accounts_batched(&all_history_addresses, client)
        .await?
        .iter()
        .filter_map(|account| match account {
            Some(account) => {
                let validator_history =
                    ValidatorHistory::try_deserialize(&mut account.data.as_slice())
                        .expect("Failed to deserialize validator history account");
                Some((validator_history.vote_account, validator_history))
            }
            None => None,
        })
        .collect::<HashMap<Pubkey, ValidatorHistory>>();

    loop_state.validator_history_map = validator_history_map;

    Ok(())
}
