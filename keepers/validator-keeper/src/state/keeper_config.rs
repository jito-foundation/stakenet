use clap::{arg, command, Parser};
use keeper_core::Cluster;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{pubkey::Pubkey, signature::Keypair};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};

pub struct KeeperConfig {
    pub client: Arc<RpcClient>,
    pub keypair: Arc<Keypair>,
    pub program_id: Pubkey,
    pub tip_distribution_program_id: Pubkey,
    pub priority_fee_in_microlamports: u64,
    pub oracle_authority_keypair: Option<Arc<Keypair>>,
    pub gossip_entrypoint: Option<SocketAddr>,
    pub validator_history_interval: u64,
    pub metrics_interval: u64,
}

#[derive(Parser, Debug)]
#[command(about = "Keeps commission history accounts up to date")]
pub struct Args {
    /// RPC URL for the cluster
    #[arg(
        short,
        long,
        env,
        default_value = "https://api.mainnet-beta.solana.com"
    )]
    pub json_rpc_url: String,

    /// Gossip entrypoint in the form of URL:PORT
    #[arg(short, long, env)]
    pub gossip_entrypoint: Option<String>,

    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(short, long, env, default_value = "./credentials/keypair.json")]
    pub keypair: PathBuf,

    /// Path to keypair used specifically for submitting permissioned transactions
    #[arg(short, long, env)]
    pub oracle_authority_keypair: Option<PathBuf>,

    /// Validator history program ID (Pubkey as base58 string)
    #[arg(
        short,
        long,
        env,
        default_value = "HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa"
    )]
    pub program_id: Pubkey,

    /// Tip distribution program ID (Pubkey as base58 string)
    #[arg(
        short,
        long,
        env,
        default_value = "4R3gSG8BpU4t19KYj8CfnbtRpnT8gtk4dvTHxVRwc2r7"
    )]
    pub tip_distribution_program_id: Pubkey,

    // Interval to update Validator History Accounts (default 300 sec)
    #[arg(short, long, env, default_value = "300")]
    pub validator_history_interval: u64,

    // Interval to emit metrics (default 60 sec)
    #[arg(short, long, env, default_value = "60")]
    pub metrics_interval: u64,

    // Priority Fees in microlamports
    #[arg(long, env, default_value = "200000")]
    pub priority_fees: u64,

    #[arg(short, long, env, default_value_t = Cluster::Mainnet)]
    pub cluster: Cluster,
}
