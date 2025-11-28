use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

use clap::{arg, command, Parser};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{account::Account, pubkey::Pubkey, signature::read_keypair_file, signer::Signer};
use stakenet_keeper::entries::copy_vote_account_entry::CopyVoteAccountEntry;
use stakenet_sdk::{
    models::entries::UpdateInstruction,
    utils::{
        accounts::get_all_validator_history_accounts,
        transactions::{get_multiple_accounts_batched, submit_instructions},
    },
};
use validator_history::{ValidatorHistory, ValidatorHistoryEntry};

#[derive(Parser)]
#[command(about = "Copy vote account")]
pub struct CrankCopyVoteAccount {
    /// Path to oracle authority keypair
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,
}

pub async fn command_crank_copy_vote_account(args: CrankCopyVoteAccount, rpc_url: String) {
    let keypair = read_keypair_file(args.keypair_path).expect("Failed reading keypair file");
    let keypair = Arc::new(keypair);
    let client = RpcClient::new(rpc_url);
    let client = Arc::new(client);

    let epoch_info = client.get_epoch_info().await.unwrap();

    let validator_histories = get_all_validator_history_accounts(&client, validator_history::id())
        .await
        .expect("Failed to get all validator history accounts");

    let validator_history_map = HashMap::from_iter(
        validator_histories
            .iter()
            .map(|vote_history| (vote_history.vote_account, *vote_history)),
    );

    let all_history_vote_account_pubkeys: Vec<Pubkey> =
        validator_history_map.keys().cloned().collect();

    let all_history_vote_accounts =
        get_multiple_accounts_batched(all_history_vote_account_pubkeys.as_slice(), &client)
            .await
            .unwrap();

    let all_history_vote_account_map = all_history_vote_account_pubkeys
        .into_iter()
        .zip(all_history_vote_accounts)
        .collect::<HashMap<Pubkey, Option<Account>>>();

    let mut vote_accounts_to_update: HashSet<&Pubkey> = all_history_vote_account_map
        .iter()
        .filter_map(|(vote_address, vote_account)| match vote_account {
            Some(account) => {
                if account.owner == solana_sdk::vote::program::id() {
                    Some(vote_address)
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    vote_accounts_to_update.retain(|vote_account| {
        !vote_account_uploaded_recently(
            &validator_history_map,
            vote_account,
            epoch_info.epoch,
            epoch_info.absolute_slot,
        )
    });

    let entries = vote_accounts_to_update
        .iter()
        .map(|vote_account| {
            CopyVoteAccountEntry::new(vote_account, &validator_history::id(), &keypair.pubkey())
        })
        .collect::<Vec<_>>();

    let update_instructions = entries
        .iter()
        .map(|copy_vote_account_entry| copy_vote_account_entry.update_instruction())
        .collect::<Vec<_>>();

    let submit_result = submit_instructions(
        &client,
        update_instructions,
        &keypair,
        0,
        50,
        0,
        Some(300_000),
        false,
    )
    .await;

    println!("Submit Result: {submit_result:?}");
}

fn vote_account_uploaded_recently(
    validator_history_map: &HashMap<Pubkey, ValidatorHistory>,
    vote_account: &Pubkey,
    epoch: u64,
    slot: u64,
) -> bool {
    if let Some(validator_history) = validator_history_map.get(vote_account) {
        if let Some(entry) = validator_history.history.last() {
            if entry.epoch == epoch as u16
                && entry.vote_account_last_update_slot
                    != ValidatorHistoryEntry::default().vote_account_last_update_slot
                && entry.vote_account_last_update_slot > slot - 50000
            {
                return true;
            }
        }
    }
    false
}
