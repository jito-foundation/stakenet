use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::str::FromStr;
use std::{path::PathBuf, time::Duration};

use anchor_lang::{InstructionData, ToAccountMetas};
use clap::{arg, command, Parser};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::instruction::Instruction;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use solana_sdk::transaction::Transaction;
use solana_sdk::{pubkey::Pubkey, signature::read_keypair_file};
use stakenet_sdk::utils::accounts::get_all_validator_history_accounts;
use tokio::time;
use validator_history::{ValidatorHistory, ValidatorHistoryEntry};

#[derive(Parser)]
#[command(about = "Copy vote account")]
pub struct CopyVoteAccount {
    /// Path to oracle authority keypair
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
struct SourceData {
    vote_account: Pubkey,
    epoch: u16,
    credits: u32,
}

pub async fn command_crank_copy_vote_account(args: CopyVoteAccount, rpc_url: String) {
    let client = RpcClient::new(rpc_url);
    let vote_accounts = get_all_validator_history_accounts(client, validator_history::id())
        .await
        .expect("Failed to get all validator history accounts");
}
