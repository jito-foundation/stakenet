use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    str::FromStr,
    sync::Arc,
};

use anyhow::anyhow;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{account::Account, pubkey::Pubkey, signature::read_keypair_file, signer::Signer};
use stakenet_keeper::entries::copy_vote_account_entry::CopyVoteAccountEntry;
use stakenet_sdk::{
    models::entries::UpdateInstruction,
    utils::{
        accounts::get_all_validator_history_accounts,
        helpers::vote_account_uploaded_recently,
        transactions::{
            get_multiple_accounts_batched, get_vote_accounts_with_retry, submit_instructions,
        },
    },
};

#[derive(Parser)]
#[command(about = "Crank to copy vote account data to validator history accounts")]
pub struct CrankCopyVoteAccount {
    /// Path to keypair for transaction signing
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Only process validators returned by get_vote_accounts RPC (active validators)
    #[arg(long, env, default_value_t = false)]
    active_only: bool,
}

pub async fn run(args: CrankCopyVoteAccount, rpc_url: String) -> anyhow::Result<()> {
    let keypair = read_keypair_file(args.keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file: {e}"))?;
    let keypair = Arc::new(keypair);
    let client = RpcClient::new(rpc_url);
    let client = Arc::new(client);

    let epoch_info = client.get_epoch_info().await?;

    let validator_histories =
        get_all_validator_history_accounts(&client, validator_history::id()).await?;

    let validator_history_map = HashMap::from_iter(
        validator_histories
            .iter()
            .map(|vote_history| (vote_history.vote_account, *vote_history)),
    );

    // If active_only is set, fetch active vote accounts from get_vote_accounts RPC
    let active_vote_accounts: Option<HashSet<Pubkey>> = if args.active_only {
        let vote_accounts = get_vote_accounts_with_retry(&client, 0, None).await?;
        let active_set: HashSet<Pubkey> = vote_accounts
            .iter()
            .filter_map(|va| Pubkey::from_str(&va.vote_pubkey).ok())
            .collect();
        println!(
            "Filtering to {} active vote accounts from get_vote_accounts RPC",
            active_set.len()
        );
        Some(active_set)
    } else {
        None
    };

    let all_history_vote_account_pubkeys: Vec<Pubkey> =
        validator_history_map.keys().cloned().collect();

    let all_history_vote_accounts =
        get_multiple_accounts_batched(all_history_vote_account_pubkeys.as_slice(), &client).await?;

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

    // Filter to only active vote accounts if the flag is set
    if let Some(ref active_set) = active_vote_accounts {
        vote_accounts_to_update.retain(|vote_account| active_set.contains(vote_account));
    }

    vote_accounts_to_update.retain(|vote_account| {
        !vote_account_uploaded_recently(
            &validator_history_map,
            vote_account,
            epoch_info.epoch,
            epoch_info.absolute_slot,
        )
    });

    println!(
        "Found {} vote accounts to update",
        vote_accounts_to_update.len()
    );

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

    Ok(())
}
