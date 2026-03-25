use std::{collections::HashSet, path::PathBuf, str::FromStr, sync::Arc};

use anyhow::anyhow;
use clap::Parser;
use futures_util::future::join_all;
use kobe_client::client_builder::KobeApiClientBuilder;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};
use stakenet_keeper::entries::is_bam_connected_entry::IsBamConnectedEntry;
use stakenet_sdk::{
    models::entries::UpdateInstruction, utils::accounts::get_all_validator_history_accounts,
};
use validator_history::ValidatorHistoryEntry;

#[derive(Parser)]
#[command(about = "Crank to copy is_bam_connected data to validator history accounts")]
pub struct CrankCopyIsBamConnected {
    /// Path to keypair for transaction signing
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Kobe API base URL
    #[arg(long)]
    kobe_api_base_url: String,

    /// Epoch number
    #[arg(long)]
    epoch: Option<u64>,
}

/// Maximum number of accounts per `get_multiple_accounts` RPC call.
const GET_MULTIPLE_ACCOUNTS_BATCH_SIZE: usize = 100;

pub async fn run(args: CrankCopyIsBamConnected, rpc_url: String) -> anyhow::Result<()> {
    let keypair = read_keypair_file(args.keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file: {e}"))?;
    let keypair = Arc::new(keypair);
    let client = RpcClient::new(rpc_url.clone());
    let client = Arc::new(client);

    let epoch_info = client.get_epoch_info().await?;
    let epoch = args.epoch.unwrap_or(epoch_info.epoch);
    let program_id = validator_history::id();

    println!("Target epoch: {epoch}");

    // Fetch validator history accounts
    let validator_histories = get_all_validator_history_accounts(&client, program_id).await?;

    // Filter to accounts that haven't had is_bam_connected set for the target epoch
    let candidates: Vec<Pubkey> = validator_histories
        .iter()
        .filter(|vh| {
            !vh.history.arr.iter().any(|entry| {
                entry.epoch == epoch as u16
                    && entry.is_bam_connected != ValidatorHistoryEntry::default().is_bam_connected
            })
        })
        .map(|vh| vh.vote_account)
        .collect();

    println!("Found {} candidate vote accounts", candidates.len());

    if candidates.is_empty() {
        println!("All accounts already up to date");
        return Ok(());
    }

    // Filter out vote accounts that no longer exist on-chain (closed/reassigned)
    // to avoid ConstraintOwner errors
    let mut live_vote_accounts: HashSet<Pubkey> = HashSet::new();
    for chunk in candidates.chunks(GET_MULTIPLE_ACCOUNTS_BATCH_SIZE) {
        let accounts = client.get_multiple_accounts(chunk).await?;
        for (pubkey, account) in chunk.iter().zip(accounts.iter()) {
            if account.is_some() {
                live_vote_accounts.insert(*pubkey);
            }
        }
    }

    let vote_accounts_to_update: Vec<Pubkey> = candidates
        .into_iter()
        .filter(|pubkey| live_vote_accounts.contains(pubkey))
        .collect();

    println!(
        "{} vote accounts exist on-chain, {} to update",
        live_vote_accounts.len(),
        vote_accounts_to_update.len()
    );

    if vote_accounts_to_update.is_empty() {
        println!("No live vote accounts need updating");
        return Ok(());
    }

    // Fetch BAM validators from Kobe API
    let kobe_client = KobeApiClientBuilder::new()
        .base_url(args.kobe_api_base_url)
        .build();
    let bam_validators = kobe_client
        .get_bam_validators(epoch)
        .await
        .map_err(|e| anyhow!("Failed to fetch BAM validators: {e}"))?
        .bam_validators;

    println!(
        "Fetched {} BAM validators from Kobe API",
        bam_validators.len()
    );

    // Pre-compute BAM pubkeys into a HashSet for O(1) lookup
    let bam_pubkeys: HashSet<Pubkey> = bam_validators
        .iter()
        .filter_map(|bam_v| Pubkey::from_str(&bam_v.vote_account).ok())
        .collect();

    // Build instructions for each validator
    let instructions: Vec<_> = vote_accounts_to_update
        .iter()
        .map(|vote_account| {
            let is_bam_connected = bam_pubkeys.contains(vote_account);

            IsBamConnectedEntry::new(
                *vote_account,
                &program_id,
                &keypair.pubkey(),
                epoch,
                is_bam_connected,
            )
            .update_instruction()
        })
        .collect();

    // Batch instructions into transactions (multiple ixs per tx)
    let recent_blockhash = client.get_latest_blockhash().await?;
    let txs: Vec<Transaction> = instructions
        .chunks(1)
        .map(|ix_batch| {
            Transaction::new_signed_with_payer(
                ix_batch,
                Some(&keypair.pubkey()),
                &[&*keypair],
                recent_blockhash,
            )
        })
        .collect();

    let total_txs = txs.len();
    println!("Sending {total_txs} transactions concurrently...");

    // Send all transactions concurrently in batches of 20
    let mut success_count = 0u64;
    let mut error_count = 0u64;
    let concurrent_batch_size = 20;

    for (batch_idx, tx_batch) in txs.chunks(concurrent_batch_size).enumerate() {
        let futures: Vec<_> = tx_batch
            .iter()
            .enumerate()
            .map(|(i, tx)| {
                let tx_idx = batch_idx * concurrent_batch_size + i + 1;
                let client = client.clone();
                let tx = tx.clone();
                async move {
                    match client.send_transaction(&tx).await {
                        Ok(sig) => {
                            println!("[{tx_idx}/{total_txs}] Sent: {sig}");
                            Ok(())
                        }
                        Err(e) => {
                            println!("[{tx_idx}/{total_txs}] Error: {e}");
                            Err(e)
                        }
                    }
                }
            })
            .collect();

        let results = join_all(futures).await;
        for result in results {
            match result {
                Ok(()) => success_count += 1,
                Err(_) => error_count += 1,
            }
        }
    }

    println!(
        "Done. {} sent, {} failed out of {} transactions ({} instructions)",
        success_count,
        error_count,
        total_txs,
        vote_accounts_to_update.len()
    );

    Ok(())
}
