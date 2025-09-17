use std::collections::HashMap;
use std::fs;

use anyhow::Result;
use log::info;
use reqwest::Client;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::pubkey::Pubkey;
use solana_sdk::account::Account;

use crate::commands::command_args::CreateBacktestCache;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ValidatorMetadata {
    pub name: Option<String>,
    pub website: Option<String>,
    pub keybase_username: Option<String>,
    pub icon_url: Option<String>,
    pub description: Option<String>,
}

impl Default for ValidatorMetadata {
    fn default() -> Self {
        Self {
            name: None,
            website: None,
            keybase_username: None,
            icon_url: None,
            description: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ValidatorsAppResponse {
    pub vote_account: String,
    pub name: Option<String>,
    pub www_url: Option<String>,
    pub keybase_id: Option<String>,
}

// Cached data structure that stores raw account data with updated validator_age
#[derive(Debug, Serialize, Deserialize)]
pub struct CachedBacktestData {
    pub steward_config: Pubkey,
    pub fetched_epoch: u64,
    pub fetched_slot: u64,
    pub config_account: Account,
    pub cluster_history_account: Account,
    pub validator_histories: Vec<(Pubkey, Account)>,
    pub validator_metadata: HashMap<String, ValidatorMetadata>,
}

/// Fetch validator metadata from validators.app API
async fn fetch_validator_metadata_from_api(
    vote_accounts: &[Pubkey],
) -> HashMap<String, ValidatorMetadata> {
    let api_token = match std::env::var("VALIDATORS_APP_TOKEN") {
        Ok(token) => token,
        Err(_) => {
            info!("VALIDATORS_APP_TOKEN not set, skipping validator metadata fetch");
            return HashMap::new();
        }
    };

    info!("Fetching validator metadata from validators.app API...");

    let client = Client::new();
    let url = "https://www.validators.app/api/v1/validators/mainnet.json";

    let response = match client.get(url).header("Token", &api_token).send().await {
        Ok(resp) => resp,
        Err(e) => {
            info!("Failed to fetch from validators.app API: {:?}", e);
            return HashMap::new();
        }
    };

    let validators: Vec<ValidatorsAppResponse> = match response.json().await {
        Ok(data) => data,
        Err(e) => {
            info!("Failed to parse validators.app API response: {:?}", e);
            return HashMap::new();
        }
    };

    info!(
        "Received {} validators from validators.app API",
        validators.len()
    );

    let vote_account_set: std::collections::HashSet<String> =
        vote_accounts.iter().map(|v| v.to_string()).collect();

    let mut metadata_map = HashMap::new();
    let mut found_count = 0;

    for validator in validators {
        if vote_account_set.contains(&validator.vote_account) {
            let metadata = ValidatorMetadata {
                name: validator.name.clone(),
                website: validator.www_url.clone(),
                keybase_username: validator.keybase_id.clone(),
                icon_url: None,
                description: None,
            };

            metadata_map.insert(validator.vote_account.clone(), metadata);
            found_count += 1;

            if found_count <= 10 {
                info!(
                    "✅ Found validator {} -> {:?}",
                    validator.vote_account, validator.name
                );
            }
        }
    }

    info!(
        "Successfully mapped {}/{} validators to metadata",
        found_count,
        vote_accounts.len()
    );

    metadata_map
}

/// Update validator_age for all validators in their history accounts
fn update_validator_ages(
    validator_histories: &mut [(Pubkey, Account)],
    current_epoch: u16,
) -> Result<()> {
    use anchor_lang::{AccountDeserialize, Discriminator};
    use validator_history::ValidatorHistory;

    info!("Updating validator_age for all validators...");

    let mut updated_count = 0;
    let total_validators = validator_histories.len();

    for (i, (_vote_account, account)) in validator_histories.iter_mut().enumerate() {
        // Progress logging
        if (i + 1) % 100 == 0 || (i + 1) == total_validators {
            info!(
                "  Processing validator ages: {}/{} validators",
                i + 1,
                total_validators
            );
        }

        // Deserialize the validator history (this reads past the discriminator)
        let mut validator_history = match ValidatorHistory::try_deserialize(&mut account.data.as_slice()) {
            Ok(h) => h,
            Err(_) => continue, // Skip if we can't deserialize
        };

        // Update validator age
        if let Err(e) = validator_history.update_validator_age(current_epoch) {
            log::debug!("Failed to update validator_age: {:?}", e);
            continue;
        }

        // Serialize back using the same method as tests
        // For zero-copy accounts, use bytemuck and manually prepend discriminator
        let mut data = vec![];
        let validator_history_bytes = bytemuck::bytes_of(&validator_history);
        data.extend_from_slice(validator_history_bytes);

        // Prepend the discriminator (8 bytes at the beginning)
        for byte in ValidatorHistory::DISCRIMINATOR.iter().rev() {
            data.insert(0, *byte);
        }

        account.data = data;
        updated_count += 1;
    }

    info!(
        "Successfully updated validator_age for {}/{} validators",
        updated_count, total_validators
    );

    Ok(())
}

pub async fn fetch_and_cache_data(
    client: &RpcClient,
    _program_id: &Pubkey,
    steward_config: &Pubkey,
    cache_file: &std::path::Path,
) -> Result<CachedBacktestData> {
    use crate::utils::accounts::get_cluster_history_address;

    info!("Fetching steward config from {}...", steward_config);
    let config_account = client
        .get_account(steward_config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get steward config account: {}", e))?;

    info!("Fetching cluster history...");
    let validator_history_program_id = validator_history::id();
    let cluster_history_address = get_cluster_history_address(&validator_history_program_id);
    info!("Cluster history address: {}", cluster_history_address);
    let cluster_history_account =
        client
            .get_account(&cluster_history_address)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to get cluster history account at {}: {}",
                    cluster_history_address,
                    e
                )
            })?;

    info!("Discovering validator history accounts using getProgramAccounts...");

    let validator_history_program_id = validator_history::id();
    let validator_history_accounts = client
        .get_program_accounts(&validator_history_program_id)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch validator history accounts: {}", e))?;

    info!(
        "Found {} validator history accounts",
        validator_history_accounts.len()
    );

    // Filter for actual ValidatorHistory accounts and try to deserialize them
    let mut validator_histories = Vec::new();
    let mut processed_count = 0;
    let total_accounts = validator_history_accounts.len();

    for (address, account) in validator_history_accounts {
        use anchor_lang::AccountDeserialize;
        use validator_history::ValidatorHistory;

        processed_count += 1;
        if processed_count % 100 == 0 || processed_count == total_accounts {
            info!(
                "Processing validator accounts: {}/{}",
                processed_count, total_accounts
            );
        }

        // Try to deserialize to validate it's a ValidatorHistory account
        match ValidatorHistory::try_deserialize(&mut account.data.as_slice()) {
            Ok(validator_history) => {
                // Extract the vote account from the validated history
                let vote_account = validator_history.vote_account;
                validator_histories.push((vote_account, account));
                log::debug!("Added validator history for vote account: {}", vote_account);
            }
            Err(_) => {
                // Skip accounts that aren't ValidatorHistory accounts
                log::debug!("Skipping non-ValidatorHistory account: {}", address);
            }
        }
    }

    info!(
        "Successfully processed {} validator history accounts",
        validator_histories.len()
    );

    let current_slot = client.get_slot().await?;
    let current_epoch = client.get_epoch_info().await?.epoch;

    // Update validator ages before fetching metadata
    update_validator_ages(&mut validator_histories, current_epoch as u16)?;

    // Fetch validator metadata for all vote accounts
    let vote_accounts: Vec<Pubkey> = validator_histories
        .iter()
        .map(|(vote_account, _)| *vote_account)
        .collect();
    let validator_metadata = fetch_validator_metadata_from_api(&vote_accounts).await;

    let cached_data = CachedBacktestData {
        steward_config: *steward_config,
        fetched_epoch: current_epoch,
        fetched_slot: current_slot,
        config_account,
        cluster_history_account,
        validator_histories,
        validator_metadata,
    };

    // Save to cache file
    info!("Serializing and saving data to cache file: {:?}", cache_file);
    let json = serde_json::to_string_pretty(&cached_data)?;
    let json_len = json.len();
    fs::write(cache_file, json)?;
    info!("Cache saved successfully ({} bytes)", json_len);

    Ok(cached_data)
}

pub async fn command_create_backtest_cache(
    client: &RpcClient,
    program_id: Pubkey,
    args: CreateBacktestCache,
) -> Result<()> {
    // Check if cache file already exists
    if args.cache_file.exists() && !args.force_fetch {
        return Err(anyhow::anyhow!(
            "Cache file {:?} already exists. Use --force-fetch to overwrite or choose a different filename.",
            args.cache_file
        ));
    }

    info!("Creating backtest cache with updated validator ages...");

    let _cached_data = fetch_and_cache_data(
        client,
        &program_id,
        &args.steward_config,
        &args.cache_file,
    )
    .await?;

    info!("✅ Cache created successfully at {:?}", args.cache_file);
    info!("📊 Cache includes:");
    info!("   • Steward config account");
    info!("   • Cluster history account");
    info!("   • Validator history accounts with updated validator_age fields");
    info!("   • Validator metadata (if VALIDATORS_APP_TOKEN is set)");

    Ok(())
}