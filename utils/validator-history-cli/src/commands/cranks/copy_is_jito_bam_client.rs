use std::{path::PathBuf, str::FromStr, sync::Arc};

use anyhow::anyhow;
use clap::Parser;
use kobe_client::client_builder::KobeApiClientBuilder;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file, signer::Signer};
use stakenet_keeper::entries::is_jito_bam_client_entry::IsJitoBamClientEntry;
use stakenet_sdk::{
    models::entries::UpdateInstruction,
    utils::{accounts::get_all_validator_history_accounts, transactions::submit_instructions},
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

pub async fn run(args: CrankCopyIsJitoBamClient, rpc_url: String) -> anyhow::Result<()> {
    let keypair = read_keypair_file(args.keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file: {e}"))?;
    let keypair = Arc::new(keypair);
    let client = RpcClient::new(rpc_url.clone());
    let client = Arc::new(client);

    let epoch_info = client.get_epoch_info().await?;
    let program_id = validator_history::id();

    // Fetch validator history accounts
    let validator_histories = get_all_validator_history_accounts(&client, program_id).await?;

    // Filter to accounts that haven't had is_jito_bam_client set this epoch
    let vote_accounts_to_update: Vec<Pubkey> = validator_histories
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

    println!(
        "Found {} vote accounts to update",
        vote_accounts_to_update.len()
    );

    if vote_accounts_to_update.is_empty() {
        println!("All accounts already up to date");
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

    // Build instructions with BAM status for each validator
    let update_instructions: Vec<_> = vote_accounts_to_update
        .iter()
        .map(|vote_account| {
            let is_jito_bam_client = bam_validators.iter().any(|bam_v| {
                Pubkey::from_str(&bam_v.vote_account)
                    .map(|pubkey| pubkey == *vote_account)
                    .unwrap_or(false)
            });

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
