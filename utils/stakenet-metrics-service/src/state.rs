use anchor_lang::AccountDeserialize;
use bytemuck::Zeroable;
use clap::{arg, command, Parser};
use jito_tip_distribution::sdk::derive_tip_distribution_account_address;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_response::RpcVoteAccountInfo;
use solana_sdk::account::Account;
use solana_sdk::epoch_info::EpochInfo;
use solana_sdk::pubkey::Pubkey;
use stakenet_sdk::models::aggregate_accounts::{AllStewardAccounts, AllValidatorAccounts};
use stakenet_sdk::models::cluster::Cluster;
use stakenet_sdk::utils::accounts::{
    get_all_steward_accounts, get_all_steward_validator_accounts, get_all_validator_accounts,
    get_all_validator_history_accounts, get_cluster_history_address,
};
use stakenet_sdk::utils::transactions::{
    get_multiple_accounts_batched, get_vote_accounts_with_retry,
};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use validator_history::constants::MIN_VOTE_EPOCHS;
use validator_history::{ClusterHistory, ValidatorHistory};

pub struct MetricsConfig {
    pub client: Arc<RpcClient>,
    pub validator_history_program_id: Pubkey,
    pub tip_distribution_program_id: Pubkey,
    pub steward_program_id: Pubkey,
    pub steward_config: Pubkey,
    pub metrics_interval: Duration,
    pub cluster: Cluster,
}

#[derive(Parser, Debug)]
#[command(about = "Emits metrics for Steward and Validator History")]
pub struct Args {
    /// RPC URL for the cluster
    #[arg(long, env, default_value = "https://api.mainnet-beta.solana.com")]
    pub json_rpc_url: String,

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

    /// Steward program ID
    #[arg(
        long,
        env,
        default_value = "sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP"
    )]
    pub steward_program_id: Pubkey,

    /// Steward config account
    #[arg(
        long,
        env,
        default_value = "jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv"
    )]
    pub steward_config: Pubkey,

    /// Interval to emit metrics (default 60 sec)
    #[arg(long, env, default_value = "60")]
    pub metrics_interval: u64,

    /// Cluster to specify
    #[arg(long, env, default_value_t = Cluster::Mainnet)]
    pub cluster: Cluster,
}

pub struct MetricsState {
    // pub keeper_flags: KeeperFlags,
    pub epoch_info: EpochInfo,

    // All vote account info fetched with get_vote_accounts - key'd by their pubkey
    pub vote_account_map: HashMap<Pubkey, RpcVoteAccountInfo>,
    // All validator history entries fetched by get_validator_history_accounts - key'd by their vote_account pubkey
    pub validator_history_map: HashMap<Pubkey, ValidatorHistory>,

    // All vote accounts mapped and fetched from validator_history_map - key'd by their vote_account pubkey
    pub all_history_vote_account_map: HashMap<Pubkey, Option<Account>>,
    // All vote accounts mapped and fetched from vote_account_map - key'd by their pubkey
    pub all_get_vote_account_map: HashMap<Pubkey, Option<Account>>,

    // All tip distribution accounts fetched from the last epoch ( current_epoch - 1 ) - key'd by their vote_account pubkey
    pub previous_epoch_tip_distribution_map: HashMap<Pubkey, Option<Account>>,
    // All tip distribution accounts fetched from the current epoch - key'd by their vote_account pubkey
    pub current_epoch_tip_distribution_map: HashMap<Pubkey, Option<Account>>,

    pub cluster_history: ClusterHistory,
    pub keeper_balance: u64,

    pub all_steward_accounts: Option<Box<AllStewardAccounts>>,
    pub all_steward_validator_accounts: Option<Box<AllValidatorAccounts>>,
    pub all_active_validator_accounts: Option<Box<AllValidatorAccounts>>,
    // pub steward_progress_flags: StewardProgressFlags,
}

impl MetricsState {
    pub async fn update_state(&mut self, config: &MetricsConfig) -> Result<(), Box<dyn Error>> {
        let client = &config.client;
        let validator_history_program_id = &config.validator_history_program_id;
        let tip_distribution_program_id = &config.tip_distribution_program_id;

        // Update Epoch
        self.epoch_info = client.get_epoch_info().await?;

        // Fetch Vote Accounts
        self.vote_account_map = get_vote_account_map(&client).await?;

        // Get all get vote accounts
        self.all_get_vote_account_map = get_all_get_vote_account_map(&client, self).await?;

        // Update Cluster History
        self.cluster_history = get_cluster_history(&client, validator_history_program_id).await?;

        // Update Validator History Accounts
        self.validator_history_map =
            get_validator_history_map(&client, validator_history_program_id).await?;

        // Get all history vote accounts
        self.all_history_vote_account_map = get_all_history_vote_account_map(&client, self).await?;

        // Update previous tip distribution map
        self.previous_epoch_tip_distribution_map = get_tip_distribution_accounts(
            &client,
            tip_distribution_program_id,
            self,
            self.epoch_info.epoch.saturating_sub(1),
        )
        .await?;

        // Update current tip distribution map
        self.current_epoch_tip_distribution_map = get_tip_distribution_accounts(
            &client,
            tip_distribution_program_id,
            self,
            self.epoch_info.epoch,
        )
        .await?;

        self.all_steward_accounts = Some(
            get_all_steward_accounts(&client, &config.steward_program_id, &config.steward_config)
                .await?,
        );

        self.all_steward_validator_accounts = Some(
            get_all_steward_validator_accounts(
                &client,
                self.all_steward_accounts.as_ref().unwrap(),
                validator_history_program_id,
            )
            .await?,
        );

        let all_get_vote_accounts: Vec<RpcVoteAccountInfo> =
            self.vote_account_map.values().cloned().collect();

        self.all_active_validator_accounts = Some(
            get_all_validator_accounts(
                &client,
                &all_get_vote_accounts,
                validator_history_program_id,
            )
            .await?,
        );

        Ok(())
    }

    pub fn get_live_vote_accounts(&self) -> HashSet<&Pubkey> {
        self.all_get_vote_account_map
            .iter()
            .filter(|(_, vote_account)| {
                if let Some(account) = vote_account {
                    account.owner == solana_program::vote::program::id()
                } else {
                    false
                }
            })
            .map(|(pubkey, _)| pubkey)
            .collect()
    }
}

impl Default for MetricsState {
    fn default() -> Self {
        Self {
            epoch_info: EpochInfo {
                epoch: 0,
                slot_index: 0,
                slots_in_epoch: 0,
                absolute_slot: 0,
                block_height: 0,
                transaction_count: None,
            },
            vote_account_map: HashMap::new(),
            validator_history_map: HashMap::new(),
            all_history_vote_account_map: HashMap::new(),
            all_get_vote_account_map: HashMap::new(),
            cluster_history: ClusterHistory::zeroed(), // todo bytemuck crate
            all_steward_accounts: None,
            all_steward_validator_accounts: None,
            all_active_validator_accounts: None,
            previous_epoch_tip_distribution_map: HashMap::new(),
            current_epoch_tip_distribution_map: HashMap::new(),
            keeper_balance: 0,
        }
    }
}

async fn get_vote_account_map(
    client: &Arc<RpcClient>,
) -> Result<HashMap<Pubkey, RpcVoteAccountInfo>, Box<dyn Error>> {
    let active_vote_accounts = HashMap::from_iter(
        get_vote_accounts_with_retry(client, MIN_VOTE_EPOCHS, None)
            .await?
            .iter()
            .map(|vote_account_info| {
                (
                    Pubkey::from_str(vote_account_info.vote_pubkey.as_str())
                        .expect("Could not parse vote pubkey"),
                    vote_account_info.clone(),
                )
            }),
    );

    Ok(active_vote_accounts)
}

async fn get_cluster_history(
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
) -> Result<ClusterHistory, Box<dyn Error>> {
    let cluster_history_address = get_cluster_history_address(program_id);
    let cluster_history_account = client.get_account(&cluster_history_address).await?;
    let cluster_history =
        ClusterHistory::try_deserialize(&mut cluster_history_account.data.as_slice())?;

    Ok(cluster_history)
}

async fn get_validator_history_map(
    client: &Arc<RpcClient>,
    program_id: &Pubkey,
) -> Result<HashMap<Pubkey, ValidatorHistory>, Box<dyn Error>> {
    let validator_histories = get_all_validator_history_accounts(client, *program_id).await?;

    let validator_history_map = HashMap::from_iter(
        validator_histories
            .iter()
            .map(|vote_history| (vote_history.vote_account, *vote_history)),
    );

    Ok(validator_history_map)
}

async fn get_all_history_vote_account_map(
    client: &Arc<RpcClient>,
    keeper_state: &MetricsState,
) -> Result<HashMap<Pubkey, Option<Account>>, Box<dyn Error>> {
    let validator_history_map = &keeper_state.validator_history_map;

    let all_history_vote_account_pubkeys: Vec<Pubkey> =
        validator_history_map.keys().cloned().collect();

    let all_history_vote_accounts =
        get_multiple_accounts_batched(all_history_vote_account_pubkeys.as_slice(), client).await?;

    let history_vote_accounts_map = all_history_vote_account_pubkeys
        .into_iter()
        .zip(all_history_vote_accounts)
        .collect::<HashMap<Pubkey, Option<Account>>>();

    Ok(history_vote_accounts_map)
}

async fn get_all_get_vote_account_map(
    client: &Arc<RpcClient>,
    keeper_state: &MetricsState,
) -> Result<HashMap<Pubkey, Option<Account>>, Box<dyn Error>> {
    let vote_account_map = &keeper_state.vote_account_map;

    // Convert the keys to a vector of Pubkey values
    let all_get_vote_account_pubkeys: Vec<Pubkey> = vote_account_map.keys().cloned().collect();

    let all_get_vote_accounts =
        get_multiple_accounts_batched(all_get_vote_account_pubkeys.as_slice(), client).await?;

    let get_vote_accounts_map = all_get_vote_account_pubkeys
        .into_iter()
        .zip(all_get_vote_accounts)
        .collect::<HashMap<Pubkey, Option<Account>>>();

    Ok(get_vote_accounts_map)
}

async fn get_tip_distribution_accounts(
    client: &Arc<RpcClient>,
    tip_distribution_program_id: &Pubkey,
    keeper_state: &MetricsState,
    epoch: u64,
) -> Result<HashMap<Pubkey, Option<Account>>, Box<dyn Error>> {
    let vote_accounts = keeper_state
        .all_history_vote_account_map
        .keys()
        .collect::<Vec<_>>();

    /* Filters tip distribution tuples to the addresses, then fetches accounts to see which ones exist */
    let tip_distribution_addresses = vote_accounts
        .iter()
        .map(|vote_pubkey| {
            let (pubkey, _) = derive_tip_distribution_account_address(
                tip_distribution_program_id,
                vote_pubkey,
                epoch,
            );
            pubkey
        })
        .collect::<Vec<Pubkey>>();

    let tip_distribution_accounts =
        get_multiple_accounts_batched(&tip_distribution_addresses, client).await?;

    let result = vote_accounts
        .into_iter()
        .zip(tip_distribution_accounts)
        .map(|(vote_pubkey, account)| (*vote_pubkey, account)) // Dereference vote_pubkey here
        .collect::<HashMap<Pubkey, Option<Account>>>();

    Ok(result)
}
