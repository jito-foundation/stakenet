use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    str::FromStr,
    sync::Arc,
};

use anyhow::anyhow;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};
use stakenet_keeper::entries::copy_vote_account_entry::CopyVoteAccountEntry;
use stakenet_sdk::{
    models::entries::UpdateInstruction,
    utils::{
        accounts::{get_all_validator_history_accounts, get_validator_list_account},
        helpers::vote_account_uploaded_recently,
        transactions::submit_instructions,
    },
};

#[derive(Parser)]
#[command(about = "Crank to copy vote account data to validator history accounts")]
pub struct CrankCopyVoteAccount {
    /// Path to keypair for transaction signing
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Validator list account pubkey (for filtering to pool validators), e.g. 3R3nGZpQs2aZo5FDQvd2MUQ6R7KhAPainds6uT6uE2mn
    /// If not provided, processes all validator history accounts.
    #[arg(long, env)]
    validator_list_pubkey: Option<String>,
}

pub async fn run(args: CrankCopyVoteAccount, rpc_url: String) -> anyhow::Result<()> {
    let keypair = read_keypair_file(args.keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file: {e}"))?;
    let keypair = Arc::new(keypair);
    let client = RpcClient::new(rpc_url.clone());
    let client = Arc::new(client);

    let epoch_info = client.get_epoch_info().await?;

    // Fetch validator history accounts
    let validator_histories =
        get_all_validator_history_accounts(&client, validator_history::id()).await?;

    let validator_history_map = HashMap::from_iter(
        validator_histories
            .iter()
            .map(|vote_history| (vote_history.vote_account, *vote_history)),
    );

    // Optionally filter to pool validators if validator list is provided
    let pool_vote_accounts: Option<HashSet<Pubkey>> =
        if let Some(validator_list_pubkey_str) = &args.validator_list_pubkey {
            let validator_list_pubkey = Pubkey::from_str(validator_list_pubkey_str)
                .map_err(|e| anyhow!("Failed to parse validator list pubkey: {e}"))?;

            let validator_list = get_validator_list_account(&client, &validator_list_pubkey)
                .await
                .map_err(|e| anyhow!("Failed to fetch validator list: {e}"))?;

            let pool_vote_accounts: HashSet<Pubkey> = validator_list
                .validators
                .iter()
                .map(|v| v.vote_account_address)
                .collect();

            Some(pool_vote_accounts)
        } else {
            None
        };

    // Filter to accounts that haven't been updated recently, optionally restricted to pool validators
    let vote_accounts_to_update: Vec<&Pubkey> = validator_histories
        .iter()
        .filter(|vote_history| {
            // If pool filtering is enabled, must be in the pool
            if let Some(ref pool) = pool_vote_accounts {
                if !pool.contains(&vote_history.vote_account) {
                    return false;
                }
            }

            // Must not have been uploaded recently
            !vote_account_uploaded_recently(
                &validator_history_map,
                &vote_history.vote_account,
                epoch_info.epoch,
                epoch_info.absolute_slot,
            )
        })
        .map(|vote_history| &vote_history.vote_account)
        .collect();

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
