use std::fs;

use anyhow::Result;
use jito_steward::constants::TVC_ACTIVATION_EPOCH;
use jito_steward::score::{validator_score, ScoreComponentsV3};
use log::info;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::pubkey::Pubkey;
use solana_sdk::account::Account;

use crate::commands::command_args::BacktestParameters;

use serde::{Deserialize, Serialize};

// Cached data structure that stores raw account data
#[derive(Debug, Serialize, Deserialize)]
pub struct CachedBacktestData {
    pub steward_config: Pubkey,
    pub fetched_epoch: u64,
    pub fetched_slot: u64,
    pub config_account: Account,
    pub cluster_history_account: Account,
    pub validator_histories: Vec<(Pubkey, Account)>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BacktestResult {
    pub epoch: u64,
    pub validator_scores: Vec<ValidatorScoreResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ValidatorScoreResult {
    #[serde(serialize_with = "serialize_pubkey_as_base58")]
    pub vote_account: Pubkey,
    pub validator_index: usize,
    pub score: f64,
    pub yield_score: f64,
    pub mev_commission_score: f64,
    pub blacklisted_score: f64,
    pub superminority_score: f64,
    pub delinquency_score: f64,
    pub running_jito_score: f64,
    pub commission_score: f64,
    pub historical_commission_score: f64,
    pub vote_credits_ratio: f64,
    pub mev_ranking_score: f64, // New: 1.0 - (max_mev_commission / 10000.0)
    pub validator_age: f64,     // New: consecutive voting epochs above threshold
    pub score_for_backtest_comparison: f64, // Consistent comparison metric across strategies
}

fn serialize_pubkey_as_base58<S>(pubkey: &Pubkey, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&pubkey.to_string())
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

impl ValidatorScoreResult {
    fn from_components(
        components: ScoreComponentsV3,
        vote_account: Pubkey,
        index: usize,
        mev_ranking_score: f64,
        validator_age: f64,
        score_for_backtest_comparison: f64,
    ) -> Self {
        ValidatorScoreResult {
            vote_account,
            validator_index: index,
            score: components.score,
            yield_score: components.yield_score,
            mev_commission_score: components.mev_commission_score,
            blacklisted_score: components.blacklisted_score,
            superminority_score: components.superminority_score,
            delinquency_score: components.delinquency_score,
            running_jito_score: components.running_jito_score,
            commission_score: components.commission_score,
            historical_commission_score: components.historical_commission_score,
            vote_credits_ratio: components.vote_credits_ratio,
            mev_ranking_score,
            validator_age,
            score_for_backtest_comparison,
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

    let cached_data = CachedBacktestData {
        steward_config: *steward_config,
        fetched_epoch: current_epoch,
        fetched_slot: current_slot,
        config_account,
        cluster_history_account,
        validator_histories,
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

            match validator_score(
                &validator_history,
                &cluster_history,
                &config,
                *epoch as u16,
                TVC_ACTIVATION_EPOCH,
            ) {
                Ok(score) => {
                    // Calculate MEV ranking score (1.0 - max_mev_commission / 10000.0)
                    let mev_ranking_score =
                        1.0 - (score.details.max_mev_commission as f64 / 10000.0);

                    // Calculate validator age (consecutive voting epochs above threshold)
                    // Hardcoded to 0.99 for backtesting experiments
                    let validator_age = calculate_validator_age(
                        &validator_history,
                        &cluster_history,
                        *epoch as u16,
                        0.99,
                        TVC_ACTIVATION_EPOCH,
                    );

                    // For MEV strategy, use mev_ranking_score as the comparison score
                    let score_for_backtest_comparison = mev_ranking_score;

                    let result = ValidatorScoreResult::from_components(
                        score,
                        *vote_account,
                        i,
                        mev_ranking_score,
                        validator_age,
                        score_for_backtest_comparison,
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

        // Sort by MEV commission first, then by validator age as tiebreaker
        validator_scores.sort_by(|a, b| {
            // Primary: Compare MEV ranking scores (higher = better, meaning lower MEV commission)
            match b.mev_ranking_score.partial_cmp(&a.mev_ranking_score) {
                Some(std::cmp::Ordering::Equal) => {
                    // Tiebreaker: Compare validator age (higher = better, meaning more consecutive epochs)
                    b.validator_age
                        .partial_cmp(&a.validator_age)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
                other => other.unwrap_or(std::cmp::Ordering::Equal),
            }
        });

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

    // Run backtest with cached data
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
