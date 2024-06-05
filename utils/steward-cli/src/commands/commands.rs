use std::path::PathBuf;

use clap::{arg, command, Parser, Subcommand};
use solana_sdk::pubkey::Pubkey;

#[derive(Parser)]
#[command(about = "CLI for validator history program")]
pub struct Args {
    /// RPC URL for the cluster
    #[arg(
        short,
        long,
        env,
        default_value = "https://api.mainnet-beta.solana.com"
    )]
    pub json_rpc_url: String,

    #[command(subcommand)]
    pub commands: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    InitConfig(InitConfig),
}

#[derive(Parser)]
#[command(about = "Initialize config account")]
pub struct InitConfig {
    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    pub keypair_path: PathBuf,

    /// Tip distribution program ID (Pubkey as base58 string)
    #[arg(short, long, env)]
    pub tip_distribution_program_id: Pubkey,

    /// New tip distribution authority (Pubkey as base58 string)
    ///
    /// If not provided, the initial keypair will be the authority
    #[arg(short, long, env, required(false))]
    pub tip_distribution_authority: Option<Pubkey>,

    // New stake authority (Pubkey as base58 string)
    ///
    /// If not provided, the initial keypair will be the authority
    #[arg(short, long, env, required(false))]
    pub stake_authority: Option<Pubkey>,
}
