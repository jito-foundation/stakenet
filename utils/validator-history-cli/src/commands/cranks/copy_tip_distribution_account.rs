use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::anyhow;
use clap::Parser;
use jito_tip_distribution_sdk::derive_tip_distribution_account_address;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};
use stakenet_keeper::entries::mev_commission_entry::ValidatorMevCommissionEntry;
use stakenet_sdk::{
    models::entries::UpdateInstruction,
    utils::{
        accounts::get_all_validator_history_accounts,
        transactions::{get_multiple_accounts_batched, submit_instructions},
    },
};
use validator_history::{ValidatorHistory, ValidatorHistoryEntry};

#[derive(Parser)]
#[command(
    about = "Crank to copy tip distribution account data (MEV commission) to validator history accounts"
)]
pub struct CrankCopyTipDistributionAccount {
    /// Path to keypair for transaction signing
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Tip distribution program ID
    #[arg(long, env)]
    tip_distribution_program_id: Pubkey,

    /// Epoch to copy tip distribution data for (defaults to current epoch)
    #[arg(short, long, env)]
    epoch: Option<u64>,
}

pub async fn run(args: CrankCopyTipDistributionAccount, rpc_url: String) -> anyhow::Result<()> {
    let keypair = read_keypair_file(args.keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file: {e}"))?;
    let keypair = Arc::new(keypair);
    let client = RpcClient::new(rpc_url);
    let client = Arc::new(client);

    let epoch_info = client.get_epoch_info().await?;
    let epoch = args.epoch.unwrap_or(epoch_info.epoch);

    println!("Processing MEV commission for epoch {epoch}");

    let validator_histories =
        get_all_validator_history_accounts(&client, validator_history::id()).await?;

    let validator_history_map: HashMap<Pubkey, ValidatorHistory> = HashMap::from_iter(
        validator_histories
            .iter()
            .map(|vote_history| (vote_history.vote_account, *vote_history)),
    );

    println!(
        "Found {} validator history accounts",
        validator_history_map.len()
    );

    // Derive tip distribution account addresses for all validators
    let vote_accounts: Vec<Pubkey> = validator_history_map.keys().cloned().collect();

    let tip_distribution_addresses: Vec<Pubkey> = vote_accounts
        .iter()
        .map(|vote_pubkey| {
            let (pubkey, _) = derive_tip_distribution_account_address(
                &args.tip_distribution_program_id,
                vote_pubkey,
                epoch,
            );
            pubkey
        })
        .collect();

    // Fetch tip distribution accounts to see which ones exist and are owned by the TDA program.
    // Uninitialized or closed TDAs are owned by System Program.
    let tip_distribution_accounts =
        get_multiple_accounts_batched(&tip_distribution_addresses, &client).await?;

    let tip_distribution_map: HashMap<Pubkey, bool> = vote_accounts
        .iter()
        .zip(tip_distribution_accounts)
        .map(|(vote_pubkey, account)| {
            let is_valid_tda = account
                .as_ref()
                .map(|acc| acc.owner == args.tip_distribution_program_id)
                .unwrap_or(false);
            (*vote_pubkey, is_valid_tda)
        })
        .collect();

    // Filter to validators that have tip distribution accounts but haven't been updated
    let vote_accounts_to_update: Vec<&Pubkey> = tip_distribution_map
        .iter()
        .filter_map(|(vote_account, has_tip_dist)| {
            if *has_tip_dist
                && !mev_commission_uploaded(&validator_history_map, vote_account, epoch)
            {
                Some(vote_account)
            } else {
                None
            }
        })
        .collect();

    println!(
        "Found {} vote accounts to update with MEV commission",
        vote_accounts_to_update.len()
    );

    for vote_account in &vote_accounts_to_update {
        let (tda_address, _) = derive_tip_distribution_account_address(
            &args.tip_distribution_program_id,
            vote_account,
            epoch,
        );
        println!("  - {} (TDA: {})", vote_account, tda_address);
    }

    if vote_accounts_to_update.is_empty() {
        println!("No accounts need updating");
        return Ok(());
    }

    println!("Building {} instructions...", vote_accounts_to_update.len());

    let entries: Vec<ValidatorMevCommissionEntry> = vote_accounts_to_update
        .iter()
        .map(|vote_account| {
            ValidatorMevCommissionEntry::new(
                vote_account,
                epoch,
                &validator_history::id(),
                &args.tip_distribution_program_id,
                &keypair.pubkey(),
            )
        })
        .collect();

    let update_instructions = entries
        .iter()
        .map(|entry| entry.update_instruction())
        .collect::<Vec<_>>();

    println!(
        "Submitting {} transactions (this may take a while)...",
        update_instructions.len()
    );

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

fn mev_commission_uploaded(
    validator_history_map: &HashMap<Pubkey, ValidatorHistory>,
    vote_account: &Pubkey,
    epoch: u64,
) -> bool {
    if let Some(validator_history) = validator_history_map.get(vote_account) {
        if let Some(latest_entry) = validator_history.history.last() {
            return latest_entry.epoch == epoch as u16
                && latest_entry.mev_commission != ValidatorHistoryEntry::default().mev_commission;
        }
    }
    false
}
