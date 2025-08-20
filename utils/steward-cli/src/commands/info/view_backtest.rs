use std::collections::HashMap;
use std::fs;

use anyhow::Result;
use jito_steward::constants::TVC_ACTIVATION_EPOCH;
use jito_steward::score::{validator_score, ScoreComponentsV3};
use log::info;
use reqwest::Client;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::pubkey::Pubkey;
use solana_sdk::account::Account;

use crate::commands::command_args::BacktestParameters;

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
    pub vote_account: String, // vote account pubkey
    pub name: Option<String>,
    pub www_url: Option<String>,
    pub keybase_id: Option<String>,
}

// Cached data structure that stores raw account data
#[derive(Debug, Serialize, Deserialize)]
pub struct CachedBacktestData {
    pub steward_config: Pubkey,
    pub fetched_epoch: u64,
    pub fetched_slot: u64,
    pub config_account: Account,
    pub cluster_history_account: Account,
    pub validator_histories: Vec<(Pubkey, Account)>,
    pub validator_metadata: HashMap<String, ValidatorMetadata>, // vote_account -> metadata
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BacktestResult {
    pub epoch: u64,
    pub validator_scores: Vec<ValidatorScoreResult>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ValidatorScoreResult {
    #[serde(serialize_with = "serialize_pubkey_as_base58")]
    pub vote_account: Pubkey,
    pub validator_index: usize,

    // Production scoring (unmodified config)
    pub production_score: f64,
    pub production_rank: Option<usize>, // Rank in production strategy (1-based)

    // Proposed scoring (97% delinquency + MEV ranking)
    pub proposed_score: f64,
    pub proposed_delinquency_score: f64, // With 97% threshold
    pub proposed_rank: Option<usize>,    // Rank in proposed strategy (1-based)

    // Delinquency failure tracking
    pub proposed_delinquency_epoch: Option<u16>, // Epoch where failed 97% threshold
    pub proposed_delinquency_ratio: Option<f64>, // Ratio at failure epoch

    // Shared component scores (from production scoring)
    pub yield_score: f64,
    pub mev_commission_score: f64,
    pub blacklisted_score: f64,
    pub superminority_score: f64,
    pub delinquency_score: f64, // Production delinquency score
    pub running_jito_score: f64,
    pub commission_score: f64,
    pub historical_commission_score: f64,
    pub vote_credits_ratio: f64,

    // Additional metrics
    pub inflation_commission_pct: f64, // Inflation commission percentage (0-100)
    pub mev_commission_pct: f64,       // MEV commission percentage (0-100)
    pub validator_age: f64,            // Consecutive voting epochs above threshold
    pub metadata: ValidatorMetadata,   // Validator name, website, etc.
}

fn serialize_pubkey_as_base58<S>(pubkey: &Pubkey, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&pubkey.to_string())
}

/// Calculate delinquency details for a specific threshold
fn calculate_delinquency_details(
    validator_history: &validator_history::ValidatorHistory,
    cluster_history: &validator_history::ClusterHistory,
    current_epoch: u16,
    epoch_credits_range: u16,
    delinquency_threshold: f64,
    tvc_activation_epoch: u64,
) -> (f64, Option<u16>, Option<f64>) {
    use validator_history::constants::TVC_MULTIPLIER;

    let epoch_credits_start = current_epoch.saturating_sub(epoch_credits_range);
    let epoch_credits_end = current_epoch.saturating_sub(1);

    let epoch_credits_window = validator_history.history.epoch_credits_range_normalized(
        epoch_credits_start,
        epoch_credits_end,
        tvc_activation_epoch,
    );

    let total_blocks_window = cluster_history
        .history
        .total_blocks_range(epoch_credits_start, epoch_credits_end);

    let mut delinquency_score = 1.0;
    let mut delinquency_epoch = None;
    let mut delinquency_ratio = None;

    for (i, (maybe_credits, maybe_blocks)) in epoch_credits_window
        .iter()
        .zip(total_blocks_window.iter())
        .enumerate()
    {
        if let Some(blocks) = maybe_blocks {
            let credits = maybe_credits.unwrap_or(0);
            let ratio = credits as f64 / (blocks * TVC_MULTIPLIER) as f64;
            if ratio < delinquency_threshold {
                delinquency_score = 0.0;
                delinquency_epoch = Some(epoch_credits_start.saturating_add(i as u16));
                delinquency_ratio = Some(ratio);
                break;
            }
        }
    }

    (delinquency_score, delinquency_epoch, delinquency_ratio)
}

/// Calculate validator age as number of epochs with non-null vote credits
fn calculate_validator_age(
    validator_history: &validator_history::ValidatorHistory,
    _cluster_history: &validator_history::ClusterHistory,
    current_epoch: u16,
    _voting_threshold: f64,
    tvc_activation_epoch: u64,
) -> f64 {
    let mut epochs_with_votes = 0.0;

    // Go backwards from current epoch - 1 (exclude current epoch like epoch credits window)
    for i in 1..=current_epoch.saturating_sub(1) {
        let epoch = current_epoch.saturating_sub(i);

        // Get vote credits for this epoch
        let epoch_credits_window = validator_history.history.epoch_credits_range_normalized(
            epoch,
            epoch,
            tvc_activation_epoch,
        );

        // If we have any vote credits data for this epoch, count it
        if let Some(Some(credits)) = epoch_credits_window.first() {
            if *credits > 0 {
                epochs_with_votes += 1.0;
            }
        }
        // Continue counting even if there are gaps (no break)
    }

    epochs_with_votes
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

    // Create lookup map for vote accounts we care about
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
                icon_url: None, // validators.app doesn't provide icon_url in this format
                description: None, // could be derived from other fields if needed
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

impl ValidatorScoreResult {
    fn new(
        vote_account: Pubkey,
        index: usize,
        production_components: ScoreComponentsV3,
        proposed_delinquency_score: f64,
        proposed_delinquency_epoch: Option<u16>,
        proposed_delinquency_ratio: Option<f64>,
        inflation_commission_pct: f64,
        mev_commission_pct: f64,
        validator_age: f64,
        metadata: ValidatorMetadata,
    ) -> Self {
        // Calculate proposed score
        // If any binary filter in production is 0, proposed must also be 0
        // Otherwise, use MEV ranking score (1.0 - mev_commission_pct/100)
        let proposed_score =
            if production_components.score == 0.0 || proposed_delinquency_score == 0.0 {
                0.0
            } else {
                1.0 - (mev_commission_pct / 100.0)
            };

        ValidatorScoreResult {
            vote_account,
            validator_index: index,
            production_score: production_components.score,
            production_rank: None, // Will be set after sorting
            proposed_score,
            proposed_delinquency_score,
            proposed_rank: None, // Will be set after sorting
            proposed_delinquency_epoch,
            proposed_delinquency_ratio,
            yield_score: production_components.yield_score,
            mev_commission_score: production_components.mev_commission_score,
            blacklisted_score: production_components.blacklisted_score,
            superminority_score: production_components.superminority_score,
            delinquency_score: production_components.delinquency_score,
            running_jito_score: production_components.running_jito_score,
            commission_score: production_components.commission_score,
            historical_commission_score: production_components.historical_commission_score,
            vote_credits_ratio: production_components.vote_credits_ratio,
            inflation_commission_pct,
            mev_commission_pct,
            validator_age,
            metadata,
        }
    }
}

async fn fetch_and_cache_data(
    client: &RpcClient,
    _program_id: &Pubkey,
    steward_config: &Pubkey,
    cache_file: Option<&std::path::Path>,
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

    // Use getProgramAccounts to find all validator history accounts
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
                // Skip accounts that aren't ValidatorHistory accounts (e.g., Config, ClusterHistory)
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

    // Save to cache file if specified
    if let Some(cache_path) = cache_file {
        info!(
            "Serializing and saving data to cache file: {:?}",
            cache_path
        );
        let json = serde_json::to_string_pretty(&cached_data)?;
        let json_len = json.len();
        fs::write(cache_path, json)?;
        info!("Cache saved successfully ({} bytes)", json_len);
    }

    Ok(cached_data)
}

fn load_cached_data(cache_file: &std::path::Path) -> Result<CachedBacktestData> {
    info!("Loading data from cache file: {:?}", cache_file);
    let json = fs::read_to_string(cache_file)?;
    info!("Deserializing cached data ({} bytes)...", json.len());
    let data = serde_json::from_str(&json)?;
    info!("Cache loaded successfully");
    Ok(data)
}

pub async fn run_backtest_with_cached_data(
    cached_data: &CachedBacktestData,
    target_epochs: Vec<u64>,
) -> Result<Vec<BacktestResult>> {
    use anchor_lang::AccountDeserialize;
    use jito_steward::Config;
    use validator_history::{ClusterHistory, ValidatorHistory};

    // Deserialize accounts from cached data
    let config: Config =
        Config::try_deserialize(&mut cached_data.config_account.data.as_slice())
            .map_err(|e| anyhow::anyhow!("Failed to deserialize steward config: {}", e))?;

    let cluster_history: ClusterHistory =
        ClusterHistory::try_deserialize(&mut cached_data.cluster_history_account.data.as_slice())
            .map_err(|e| anyhow::anyhow!("Failed to deserialize cluster history: {}", e))?;

    let mut results = Vec::new();

    let total_epochs = target_epochs.len();
    for (epoch_idx, epoch) in target_epochs.iter().enumerate() {
        info!(
            "Running backtest for epoch {} ({}/{})...",
            epoch,
            epoch_idx + 1,
            total_epochs
        );
        let mut validator_scores = Vec::new();
        let mut scored_count = 0;
        let mut skipped_count = 0;
        let total_validators = cached_data.validator_histories.len();

        for (i, (vote_account, account)) in cached_data.validator_histories.iter().enumerate() {
            let validator_history: ValidatorHistory =
                match ValidatorHistory::try_deserialize(&mut account.data.as_slice()) {
                    Ok(h) => h,
                    Err(e) => {
                        log::debug!(
                            "Could not deserialize validator history for {}: {:?}",
                            vote_account,
                            e
                        );
                        continue;
                    }
                };

            // Skip if validator doesn't have sufficient history (check if it has any entries)
            if validator_history.history.idx == 0 {
                skipped_count += 1;
                continue;
            }

            // First compute production score with unmodified config
            match validator_score(
                &validator_history,
                &cluster_history,
                &config,
                *epoch as u16,
                TVC_ACTIVATION_EPOCH,
            ) {
                Ok(production_score) => {
                    // Calculate delinquency details with 97% threshold
                    let (
                        proposed_delinquency_score,
                        proposed_delinquency_epoch,
                        proposed_delinquency_ratio,
                    ) = calculate_delinquency_details(
                        &validator_history,
                        &cluster_history,
                        *epoch as u16,
                        config.parameters.epoch_credits_range,
                        0.97,
                        TVC_ACTIVATION_EPOCH,
                    );

                    // Calculate commission percentages
                    let inflation_commission_pct = production_score.details.max_commission as f64;
                    let mev_commission_pct =
                        production_score.details.max_mev_commission as f64 / 100.0;

                    // Calculate validator age (consecutive voting epochs above threshold)
                    let validator_age = calculate_validator_age(
                        &validator_history,
                        &cluster_history,
                        *epoch as u16,
                        0.99,
                        TVC_ACTIVATION_EPOCH,
                    );

                    // Get metadata for this validator
                    let metadata = cached_data
                        .validator_metadata
                        .get(&vote_account.to_string())
                        .cloned()
                        .unwrap_or_default();

                    let result = ValidatorScoreResult::new(
                        *vote_account,
                        i,
                        production_score,
                        proposed_delinquency_score,
                        proposed_delinquency_epoch,
                        proposed_delinquency_ratio,
                        inflation_commission_pct,
                        mev_commission_pct,
                        validator_age,
                        metadata,
                    );
                    validator_scores.push(result);
                    scored_count += 1;
                }
                Err(e) => {
                    // Log but continue - some validators may not have sufficient history
                    log::debug!(
                        "Could not score validator {} for epoch {}: {:?}",
                        vote_account,
                        epoch,
                        e
                    );
                    skipped_count += 1;
                }
            }

            // Progress update every 50 validators or at the end
            if (i + 1) % 50 == 0 || (i + 1) == total_validators {
                info!(
                    "  Scoring progress: {}/{} validators processed, {} scored, {} skipped",
                    i + 1,
                    total_validators,
                    scored_count,
                    skipped_count
                );
            }
        }

        // Sort by production score and assign production ranks
        validator_scores.sort_by(|a, b| {
            b.production_score
                .partial_cmp(&a.production_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Assign production ranks (1-based)
        for (rank, validator) in validator_scores.iter_mut().enumerate() {
            validator.production_rank = Some(rank + 1);
        }

        // Create a copy for proposed sorting
        let mut proposed_validators = validator_scores.clone();

        // Sort by proposed score with validator age as tiebreaker
        proposed_validators.sort_by(|a, b| {
            match b.proposed_score.partial_cmp(&a.proposed_score) {
                Some(std::cmp::Ordering::Equal) => {
                    // Tiebreaker: validator age
                    b.validator_age
                        .partial_cmp(&a.validator_age)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
                other => other.unwrap_or(std::cmp::Ordering::Equal),
            }
        });

        // Create a map of vote_account to proposed rank
        let mut proposed_ranks = std::collections::HashMap::new();
        for (rank, validator) in proposed_validators.iter().enumerate() {
            proposed_ranks.insert(validator.vote_account, rank + 1);
        }

        // Update proposed ranks in the original vector
        for validator in &mut validator_scores {
            validator.proposed_rank = proposed_ranks.get(&validator.vote_account).copied();
        }

        info!(
            "Epoch {} complete: {} validators scored, {} skipped",
            epoch, scored_count, skipped_count
        );

        results.push(BacktestResult {
            epoch: *epoch,
            validator_scores,
        });
    }

    Ok(results)
}

pub async fn command_view_backtest(
    client: &RpcClient,
    program_id: Pubkey,
    args: BacktestParameters,
) -> Result<()> {
    // Check if output file already exists
    if args.output_file.exists() {
        return Err(anyhow::anyhow!(
            "Output file {:?} already exists. Please choose a different filename or delete the existing file.",
            args.output_file
        ));
    }
    // Determine cache file path
    let cache_file = args.cache_file.as_ref().map(|p| p.as_path());

    // Fetch or load cached data
    let cached_data = if let Some(cache_path) = cache_file {
        if cache_path.exists() && !args.force_fetch {
            // Load from cache if it exists and force_fetch is not set
            match load_cached_data(cache_path) {
                Ok(data) => {
                    // Verify it's for the same steward config
                    if data.steward_config != args.steward_config {
                        info!("Cache is for different steward config, fetching fresh data...");
                        fetch_and_cache_data(
                            client,
                            &program_id,
                            &args.steward_config,
                            Some(cache_path),
                        )
                        .await?
                    } else {
                        info!(
                            "Using cached data from epoch {} (slot {})",
                            data.fetched_epoch, data.fetched_slot
                        );
                        data
                    }
                }
                Err(e) => {
                    info!("Failed to load cache: {:?}, fetching fresh data...", e);
                    fetch_and_cache_data(
                        client,
                        &program_id,
                        &args.steward_config,
                        Some(cache_path),
                    )
                    .await?
                }
            }
        } else {
            // Fetch fresh data if cache doesn't exist or force_fetch is set
            if args.force_fetch {
                info!("Force fetch enabled, fetching fresh data...");
            } else {
                info!("Cache file not found, fetching fresh data...");
            }
            fetch_and_cache_data(client, &program_id, &args.steward_config, Some(cache_path))
                .await?
        }
    } else {
        // No cache file specified, always fetch fresh
        info!("No cache file specified, fetching fresh data...");
        fetch_and_cache_data(client, &program_id, &args.steward_config, None).await?
    };

    // Determine start epoch and calculate target epochs
    let start_epoch = if let Some(epoch) = args.start_epoch {
        epoch
    } else {
        // Default to current epoch - 1
        cached_data.fetched_epoch.saturating_sub(1)
    };

    // Calculate target epochs aligned with rebalancing schedule (every 10 epochs)
    // Find the most recent rebalancing epoch at or before start_epoch
    let rebalancing_interval = 10u64;
    let latest_rebalancing_epoch = (start_epoch / rebalancing_interval) * rebalancing_interval;

    let mut target_epochs = Vec::new();
    for i in 0..args.lookback_epochs {
        if let Some(epoch) = latest_rebalancing_epoch.checked_sub(i * rebalancing_interval) {
            target_epochs.push(epoch);
        }
    }
    target_epochs.reverse(); // Order from oldest to newest

    info!("Running backtest for epochs: {:?}", target_epochs);

    info!("Starting backtest analysis for epochs: {:?}", target_epochs);

    // Run backtest with cached data (computes both production and proposed scores)
    let results = run_backtest_with_cached_data(&cached_data, target_epochs).await?;

    info!("Backtest analysis complete for {} epochs", results.len());

    // Save results to file
    info!("Saving detailed results to file...");
    let json = serde_json::to_string_pretty(&results)?;
    let json_len = json.len();
    fs::write(&args.output_file, json)?;
    info!(
        "Results saved to {:?} ({} bytes)",
        args.output_file, json_len
    );

    info!("Backtest complete!");

    Ok(())
}
