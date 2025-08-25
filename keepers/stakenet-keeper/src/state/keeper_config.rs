use std::fmt;

use clap::{arg, command, Parser};
use rusqlite::Connection;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{pubkey::Pubkey, signature::Keypair};
use stakenet_sdk::models::cluster::Cluster;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

pub struct KeeperConfig {
    pub client: Arc<RpcClient>,
    pub keypair: Arc<Keypair>,
    pub validator_history_program_id: Pubkey,
    pub tip_distribution_program_id: Pubkey,
    pub priority_fee_distribution_program_id: Pubkey,
    pub steward_program_id: Pubkey,
    pub steward_config: Pubkey,
    pub priority_fee_in_microlamports: u64,
    pub tx_retry_count: u16,
    pub tx_confirmation_seconds: u64,
    pub oracle_authority_keypair: Option<Arc<Keypair>>,
    /// Gossip entrypoints
    ///
    /// Allow multiple gossip entries to re-run the discovery process with next URL if gossip
    /// entries are not found
    pub gossip_entrypoints: Option<Vec<SocketAddr>>,
    pub validator_history_interval: u64,
    pub steward_interval: u64,
    pub metrics_interval: u64,
    pub block_metadata_interval: u64,
    pub run_flags: u32,
    pub cool_down_range: u8,
    pub full_startup: bool,
    pub no_pack: bool,
    pub pay_for_new_accounts: bool,
    pub sqlite_connection: Arc<Mutex<Connection>>,
    pub priority_fee_oracle_authority_keypair: Option<Arc<Keypair>>,
    pub redundant_rpc_urls: Option<Arc<Vec<RpcClient>>>,
    pub cluster: Cluster,
    pub cluster_name: String,
    pub lookback_epochs: u64,

    /// Minimum activated stake threshold for creating validator history accounts (in lamports)
    pub validator_history_min_stake: u64,
}

impl KeeperConfig {
    pub async fn mut_connection(&self) -> tokio::sync::MutexGuard<'_, Connection> {
        self.sqlite_connection.lock().await
    }
}

#[derive(Parser, Debug)]
#[command(about = "Keeps commission history accounts up to date")]
pub struct Args {
    /// RPC URL for the cluster
    #[arg(long, env, default_value = "https://api.mainnet-beta.solana.com")]
    pub json_rpc_url: String,

    /// Gossip entrypoints in the form of URL:PORT
    ///
    /// - Accept multiple URLs
    #[arg(long, env, value_delimiter = ',')]
    pub gossip_entrypoints: Option<Vec<String>>,

    /// Path to keypair used to pay for account creation and execute transactions
    #[arg(long, env, default_value = "./credentials/keypair.json")]
    pub keypair: PathBuf,

    /// Path to keypair used specifically for submitting permissioned transactions
    #[arg(long, env)]
    pub oracle_authority_keypair: Option<PathBuf>,

    /// Path to keypair used specifically for submitting permissioned transactions related to
    /// priority fees and block metadata
    #[arg(long, env)]
    pub priority_fee_oracle_authority_keypair: Option<PathBuf>,

    /// Validator history program ID (Pubkey as base58 string)
    #[arg(
        long,
        env,
        default_value = "HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa"
    )]
    pub validator_history_program_id: Pubkey,

    /// Tip distribution program ID (Pubkey as base58 string)
    #[arg(
        short,
        long,
        env,
        default_value = "4R3gSG8BpU4t19KYj8CfnbtRpnT8gtk4dvTHxVRwc2r7"
    )]
    pub tip_distribution_program_id: Pubkey,

    /// Priority Fee distribution program ID (Pubkey as base58 string)
    #[arg(
        short,
        long,
        env,
        // TODO: Change this for actual deployment
        default_value = "5DdB5ZuSR97rqgVHtjb4t1uz1auFEa2xQ32aAxjsJLEC"
    )]
    pub priority_fee_distribution_program_id: Pubkey,

    /// Steward program ID
    #[arg(
        long,
        env,
        default_value = "Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8"
    )]
    pub steward_program_id: Pubkey,

    /// Steward config account
    #[arg(
        long,
        env,
        default_value = "jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv"
    )]
    pub steward_config: Pubkey,

    /// Interval to update Validator History Accounts (default 300 sec)
    #[arg(long, env, default_value = "300")]
    pub validator_history_interval: u64,

    /// Interval to run steward (default 301 sec)
    #[arg(long, env, default_value = "301")]
    pub steward_interval: u64,

    /// Interval to emit metrics (default 60 sec)
    #[arg(long, env, default_value = "60")]
    pub metrics_interval: u64,

    /// Priority Fees in microlamports
    #[arg(long, env, default_value = "20000")]
    pub priority_fees: u64,

    #[arg(long, env, default_value = "50")]
    pub tx_retry_count: u16,

    #[arg(long, env, default_value = "30")]
    pub tx_confirmation_seconds: u64,

    /// Cluster to specify
    #[arg(long, env, default_value_t = Cluster::Mainnet)]
    pub cluster: Cluster,

    /// Run running the cluster history
    #[arg(long, env, default_value = "false")]
    pub run_cluster_history: bool,

    /// Run MEV commission
    #[arg(long, env, default_value = "false")]
    pub run_copy_vote_accounts: bool,

    /// Run MEV commission
    #[arg(long, env, default_value = "false")]
    pub run_mev_commission: bool,

    /// Run MEV earned
    #[arg(long, env, default_value = "false")]
    pub run_mev_earned: bool,

    /// Run stake upload
    /// NOTE: This is a permissioned operation and requires the oracle_authority_keypair
    #[arg(long, env, default_value = "false")]
    pub run_stake_upload: bool,

    /// Run gossip upload
    /// NOTE: This is a permissioned operation and requires the oracle_authority_keypair
    #[arg(long, env, default_value = "false")]
    pub run_gossip_upload: bool,

    /// Run Dune Backfill
    /// Requires Dune API key
    #[arg(long, env, default_value = "false")]
    pub run_dune_backfill: bool,

    /// Run stake upload
    #[arg(long, env, default_value = "false")]
    pub run_steward: bool,

    /// Run emit metrics
    #[arg(long, env, default_value = "false")]
    pub run_emit_metrics: bool,

    /// Run with the startup flag set to true
    #[arg(long, env, default_value = "false")]
    pub full_startup: bool,

    /// DEBUGGING Don't smart pack instructions - it will be faster, but more expensive
    #[arg(long, env, default_value = "false")]
    pub no_pack: bool,

    /// Pay for the creation of new accounts when needed
    #[arg(long, env, default_value = "false")]
    pub pay_for_new_accounts: bool,

    /// Pay for the creation of new accounts when needed
    #[arg(long, env, default_value = "local")]
    pub region: String,

    /// DEBUGGING Changes the random cool down range ( minutes )
    #[arg(long, env, default_value = "20")]
    pub cool_down_range: u8,

    /// Run block metadata keeper
    #[arg(long, env, default_value = "true")]
    pub run_block_metadata: bool,

    /// Interval to update block metadata in local SQLite file (default 17280 sec, which is ~1/10 of an epoch)
    #[arg(long, env, default_value = "17280")]
    pub block_metadata_interval: u64,

    /// Path to the local SQLite file
    #[arg(long, env, default_value = "./block_keeper.db3")]
    pub sqlite_path: PathBuf,

    /// Solana JSON RPC URLs that can be used for fall back checks when information is not
    /// available from the primary RPC
    #[arg(long, env)]
    pub redundant_rpc_urls: Option<Vec<String>>,

    /// Run Priority Fee Commission
    #[arg(long, env, default_value = "false")]
    pub run_priority_fee_commission: bool,

    /// Number of epochs to look back for block metadata
    #[arg(long, env, default_value = "3")]
    pub lookback_epochs: u64,

    /// Minimum activated stake threshold for creating validator history accounts (in lamports)
    #[arg(long, env, default_value = "500000000000")]
    pub validator_history_min_stake: u64,
}

impl fmt::Display for Args {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Stakenet Keeper Configuration:\n\
            -------------------------------\n\
            JSON RPC URL: {}\n\
            Gossip Entrypoints: {:?}\n\
            Keypair Path: {:?}\n\
            Oracle Authority Keypair Path: {:?}\n\
            Priority Fee Oracle Authority Keypair Path: {:?}\n\
            Validator History Program ID: {}\n\
            Tip Distribution Program ID: {}\n\
            Priority Fee Distribution Program ID: {}\n\
            Steward Program ID: {}\n\
            Steward Config: {}\n\
            Validator History Interval: {} seconds\n\
            Steward Interval: {} seconds\n\
            Metrics Interval: {} seconds\n\
            Priority Fees: {} microlamports\n\
            Retry Count: {}\n\
            Confirmation Seconds: {}\n\
            Cluster: {:?}\n\
            Run Cluster History: {}\n\
            Run Copy Vote Accounts: {}\n\
            Run MEV Commission: {}\n\
            Run MEV Earned: {}\n\
            Run Stake Upload: {}\n\
            Run Gossip Upload: {}\n\
            Run Steward: {}\n\
            Run Emit Metrics: {}\n\
            Full Startup: {}\n\
            No Pack: {}\n\
            Pay for New Accounts: {}\n\
            Cool Down Range: {} minutes\n\
            Run Block Metadata {}\n\
            Block Metadata Interval: {} seconds\n\
            Lookback Epochs: {}\n\
            SQLite path: {:?}\n\
            Region: {}\n\
            Redundant RPC URLs: {:?}\n\
            Run Priority Fee Commission: {:?}\n\
            Validator History Min Stake: {:?} lamports\n\
            -------------------------------",
            self.json_rpc_url,
            self.gossip_entrypoints,
            self.keypair,
            self.oracle_authority_keypair,
            self.priority_fee_oracle_authority_keypair,
            self.validator_history_program_id,
            self.tip_distribution_program_id,
            self.priority_fee_distribution_program_id,
            self.steward_program_id,
            self.steward_config,
            self.validator_history_interval,
            self.steward_interval,
            self.metrics_interval,
            self.priority_fees,
            self.tx_retry_count,
            self.tx_confirmation_seconds,
            self.cluster,
            self.run_cluster_history,
            self.run_copy_vote_accounts,
            self.run_mev_commission,
            self.run_mev_earned,
            self.run_stake_upload,
            self.run_gossip_upload,
            self.run_steward,
            self.run_emit_metrics,
            self.full_startup,
            self.no_pack,
            self.pay_for_new_accounts,
            self.cool_down_range,
            self.run_block_metadata,
            self.block_metadata_interval,
            self.lookback_epochs,
            self.sqlite_path,
            self.region,
            self.redundant_rpc_urls,
            self.run_priority_fee_commission,
            self.validator_history_min_stake
        )
    }
}
