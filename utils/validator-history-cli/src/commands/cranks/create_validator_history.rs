use std::{path::PathBuf, str::FromStr, sync::Arc};

use anyhow::anyhow;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file};
use stakenet_sdk::utils::{
    accounts::get_validator_history_address,
    instructions::get_create_validator_history_instructions,
    transactions::{
        get_multiple_accounts_batched, get_vote_accounts_with_retry, submit_transactions,
    },
};
use validator_history::constants::MIN_VOTE_EPOCHS;

#[derive(Parser)]
#[command(about = "Crank to create missing validator history accounts")]
pub struct CrankCreateValidatorHistory {
    /// Path to keypair for transaction signing
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Minimum activated stake threshold for creating validator history accounts (in lamports)
    #[arg(long, env, default_value = "500000000000")]
    validator_history_min_stake: u64,

    /// Validator History Program ID
    #[arg(
        long,
        alias = "program-id",
        env,
        default_value_t = validator_history::id()
    )]
    validator_history_program_id: Pubkey,
}

pub async fn run(args: CrankCreateValidatorHistory, client: Arc<RpcClient>) -> anyhow::Result<()> {
    let keypair = read_keypair_file(args.keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file: {e}"))?;
    let keypair = Arc::new(keypair);

    // Vote accounts with enough epoch credits to be initialized, filtered by stake like the keeper
    let vote_accounts = get_vote_accounts_with_retry(&client, MIN_VOTE_EPOCHS, None)
        .await?
        .into_iter()
        .filter(|vote_account| vote_account.activated_stake > args.validator_history_min_stake)
        .filter_map(|vote_account| Pubkey::from_str(&vote_account.vote_pubkey).ok())
        .collect::<Vec<_>>();

    let history_addresses = vote_accounts
        .iter()
        .map(|vote_account| {
            get_validator_history_address(vote_account, &args.validator_history_program_id)
        })
        .collect::<Vec<_>>();
    let history_accounts = get_multiple_accounts_batched(&history_addresses, &client).await?;

    // Create accounts that don't exist
    let create_transactions = vote_accounts
        .iter()
        .zip(history_accounts)
        .filter(|(_, history_account)| history_account.is_none())
        .map(|(vote_account, _)| {
            get_create_validator_history_instructions(
                vote_account,
                &args.validator_history_program_id,
                &keypair,
            )
        })
        .collect::<Vec<_>>();

    println!(
        "Found {} validator history accounts to create",
        create_transactions.len()
    );

    let submit_result = submit_transactions(&client, create_transactions, &keypair, 50, 30).await;

    println!("Submit Result: {submit_result:?}");

    Ok(())
}
