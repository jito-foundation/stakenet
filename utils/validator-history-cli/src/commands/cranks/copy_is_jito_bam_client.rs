use std::{collections::HashSet, path::PathBuf, str::FromStr, sync::Arc};

use anyhow::anyhow;
use clap::Parser;
use kobe_client::client_builder::KobeApiClientBuilder;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};
use stakenet_keeper::entries::is_jito_bam_client_entry::IsJitoBamClientEntry;
use stakenet_sdk::{
    models::entries::UpdateInstruction, utils::accounts::get_all_validator_history_accounts,
};
use validator_history::ValidatorHistoryEntry;

#[derive(Parser)]
#[command(about = "Crank to copy is_jito_bam_client data to validator history accounts")]
pub struct CrankCopyIsJitoBamClient {
    /// Path to keypair for transaction signing
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Kobe API base URL
    #[arg(long, env)]
    kobe_api_base_url: String,
}

/// Maximum number of accounts per `get_multiple_accounts` RPC call.
const GET_MULTIPLE_ACCOUNTS_BATCH_SIZE: usize = 100;

pub async fn run(args: CrankCopyIsJitoBamClient, rpc_url: String) -> anyhow::Result<()> {
    let keypair = read_keypair_file(args.keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file: {e}"))?;
    let keypair = Arc::new(keypair);
    let client = RpcClient::new(rpc_url.clone());
    let client = Arc::new(client);

    let epoch_info = client.get_epoch_info().await?;
    let program_id = validator_history::id();

    // Check epoch progress is at least 90%
    let epoch_progress = epoch_info.slot_index as f64 / epoch_info.slots_in_epoch as f64;
    println!(
        "Epoch {} progress: {:.2}% (slot {}/{})",
        epoch_info.epoch,
        epoch_progress * 100.0,
        epoch_info.slot_index,
        epoch_info.slots_in_epoch
    );
    if epoch_progress < 0.9 {
        println!("Epoch progress is below 90%, skipping. Run again later.");
        return Ok(());
    }

    // Fetch validator history accounts
    let validator_histories = get_all_validator_history_accounts(&client, program_id).await?;

    // Filter to accounts that haven't had is_jito_bam_client set this epoch
    let candidates: Vec<Pubkey> = validator_histories
        .iter()
        .filter(|vh| {
            if let Some(latest_entry) = vh.history.last() {
                !(latest_entry.epoch == epoch_info.epoch as u16
                    && latest_entry.is_jito_bam_client
                        != ValidatorHistoryEntry::default().is_jito_bam_client)
            } else {
                true
            }
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
        .get_bam_validators(epoch_info.epoch)
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
            let is_jito_bam_client = bam_pubkeys.contains(vote_account);

            IsJitoBamClientEntry::new(
                *vote_account,
                &program_id,
                &keypair.pubkey(),
                epoch_info.epoch,
                is_jito_bam_client,
            )
            .update_instruction()
        })
        .collect();

    // Batch instructions into transactions (multiple ixs per tx) and fire-and-forget
    let tx_chunks: Vec<_> = instructions.chunks(5).collect();
    let total_txs = tx_chunks.len();
    let mut success_count = 0u64;
    let mut error_count = 0u64;

    for (i, ix_batch) in tx_chunks.into_iter().enumerate() {
        let recent_blockhash = client.get_latest_blockhash().await?;
        let tx = Transaction::new_signed_with_payer(
            ix_batch,
            Some(&keypair.pubkey()),
            &[&*keypair],
            recent_blockhash,
        );

        match client.send_and_confirm_transaction(&tx).await {
            Ok(sig) => {
                success_count += 1;
                println!("[{}/{}] Sent: {sig}", i + 1, total_txs);
            }
            Err(e) => {
                error_count += 1;
                println!("[{}/{}] Error: {e}", i + 1, total_txs);
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
