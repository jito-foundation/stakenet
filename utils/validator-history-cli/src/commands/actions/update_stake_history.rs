use std::{path::PathBuf, sync::Arc};

use anyhow::anyhow;
use clap::Parser;
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_response::RpcVoteAccountInfo};
use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};
use stakenet_keeper::{
    entries::stake_history_entry::StakeHistoryEntry,
    operations::stake_upload::get_stake_rank_map_and_superminority_count,
};
use stakenet_sdk::models::entries::UpdateInstruction;

#[derive(Parser)]
#[command(about = "Crank to copy vote account data to validator history accounts")]
pub struct UpdateStakeHistory {
    /// Path to keypair for transaction signing
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Only process validators returned by get_vote_accounts RPC (active validators)
    #[arg(long, env)]
    vote_account: Pubkey,
}

pub async fn run(args: UpdateStakeHistory, rpc_url: String) -> anyhow::Result<()> {
    let keypair = read_keypair_file(args.keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file: {e}"))?;
    let keypair = Arc::new(keypair);
    let client = RpcClient::new(rpc_url);
    let client = Arc::new(client);

    let vote_accounts = client.get_vote_accounts().await?;
    let vote_accounts: Vec<&RpcVoteAccountInfo> = vote_accounts.current.iter().collect();
    let epoch_info = client.get_epoch_info().await?;

    let (stake_rank_map, superminority_threshold) =
        get_stake_rank_map_and_superminority_count(&vote_accounts);

    let rank = stake_rank_map[&args.vote_account.to_string()];
    let is_superminority = rank <= superminority_threshold;

    for vote_account_info in vote_accounts {
        if vote_account_info.vote_pubkey == args.vote_account.to_string() {
            let copy_vote_account_entry = StakeHistoryEntry::new(
                vote_account_info,
                &validator_history::id(),
                &keypair.pubkey(),
                epoch_info.epoch,
                rank,
                is_superminority,
            );

            let update_instruction = copy_vote_account_entry.update_instruction();

            let hash = client
                .get_latest_blockhash()
                .await
                .map_err(|e| anyhow!("Failed to fetch latest blockhash: {e}"))?;
            let transaction = Transaction::new_signed_with_payer(
                &[update_instruction],
                Some(&keypair.pubkey()),
                &[keypair.clone()],
                hash,
            );
            let signature = client.send_transaction(&transaction).await?;

            println!("Submit Result: {signature:?}");
            break;
        }
    }

    Ok(())
}
