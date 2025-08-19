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
    pub validator_list_account: Account,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BacktestResult {
    pub epoch: u64,
    pub validator_scores: Vec<ValidatorScoreResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ValidatorScoreResult {
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
}

impl ValidatorScoreResult {
    fn from_components(components: ScoreComponentsV3, vote_account: Pubkey, index: usize) -> Self {
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
        }
    }
}

async fn fetch_and_cache_data(
    client: &RpcClient,
    program_id: &Pubkey,
    steward_config: &Pubkey,
    cache_file: Option<&std::path::Path>,
) -> Result<CachedBacktestData> {
    use crate::utils::accounts::{
        get_cluster_history_address, get_stake_pool_account, get_steward_config_account,
        get_steward_state_account, get_validator_history_address,
    };
    use crate::utils::get_validator_list_account;

    info!("Fetching steward config...");
    let config = get_steward_config_account(client, steward_config).await?;
    let config_account = client.get_account(steward_config).await?;

    info!("Fetching cluster history...");
    let cluster_history_address = get_cluster_history_address(program_id);
    let cluster_history_account = client.get_account(&cluster_history_address).await?;

    info!("Fetching steward state...");
    let (_steward_state, _) = get_steward_state_account(client, program_id, steward_config).await?;

    info!("Fetching stake pool...");
    let stake_pool_account_data = get_stake_pool_account(client, &config.stake_pool).await?;

    info!("Fetching validator list...");
    let validator_list_address = &stake_pool_account_data.validator_list;
    let validator_list_account = client.get_account(validator_list_address).await?;
    let validator_list = get_validator_list_account(client, validator_list_address).await?;

    // Extract vote accounts from validator list
    let vote_accounts: Vec<Pubkey> = validator_list
        .validators
        .iter()
        .map(|v| v.vote_account_address)
        .collect();

    info!(
        "Fetching validator histories for {} validators...",
        vote_accounts.len()
    );

    // Fetch validator history accounts
    let mut validator_histories = Vec::new();
    for vote_account in vote_accounts {
        let validator_history_address = get_validator_history_address(&vote_account, program_id);
        match client.get_account(&validator_history_address).await {
            Ok(account) => {
                validator_histories.push((vote_account, account));
            }
            Err(e) => {
                log::debug!(
                    "Could not fetch validator history for {}: {:?}",
                    vote_account,
                    e
                );
            }
        }
    }

    let current_slot = client.get_slot().await?;
    let current_epoch = client.get_epoch_info().await?.epoch;

    let cached_data = CachedBacktestData {
        steward_config: *steward_config,
        fetched_epoch: current_epoch,
        fetched_slot: current_slot,
        config_account,
        cluster_history_account,
        validator_histories,
        validator_list_account,
    };

    // Save to cache file if specified
    if let Some(cache_path) = cache_file {
        info!("Saving data to cache file: {:?}", cache_path);
        let json = serde_json::to_string_pretty(&cached_data)?;
        fs::write(cache_path, json)?;
        info!("Cache saved successfully");
    }

    Ok(cached_data)
}

fn load_cached_data(cache_file: &std::path::Path) -> Result<CachedBacktestData> {
    info!("Loading data from cache file: {:?}", cache_file);
    let json = fs::read_to_string(cache_file)?;
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
    use spl_stake_pool::state::ValidatorList;
    use validator_history::{ClusterHistory, ValidatorHistory};

    // Deserialize accounts from cached data
    let config: Config = Config::try_deserialize(&mut cached_data.config_account.data.as_slice())?;

    let cluster_history: ClusterHistory =
        ClusterHistory::try_deserialize(&mut cached_data.cluster_history_account.data.as_slice())?;

    let validator_list: ValidatorList =
        borsh::from_slice(&cached_data.validator_list_account.data)?;

    // Extract vote accounts from validator list
    let vote_accounts: Vec<Pubkey> = validator_list
        .validators
        .iter()
        .map(|v| v.vote_account_address)
        .collect();

    let mut results = Vec::new();

    for epoch in target_epochs {
        info!("Running backtest for epoch {}...", epoch);
        let mut validator_scores = Vec::new();

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
                continue;
            }

            // Only score validators that are in the validator list
            if !vote_accounts.contains(vote_account) {
                continue;
            }

            match validator_score(
                &validator_history,
                &cluster_history,
                &config,
                epoch as u16,
                TVC_ACTIVATION_EPOCH,
            ) {
                Ok(score) => {
                    let result = ValidatorScoreResult::from_components(score, *vote_account, i);
                    validator_scores.push(result);
                }
                Err(e) => {
                    // Log but continue - some validators may not have sufficient history
                    log::debug!(
                        "Could not score validator {} for epoch {}: {:?}",
                        vote_account,
                        epoch,
                        e
                    );
                }
            }
        }

        // Sort by score descending
        validator_scores.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        results.push(BacktestResult {
            epoch,
            validator_scores,
        });
    }

    Ok(results)
}

pub fn generate_comparison_report(results: &[BacktestResult]) -> String {
    let mut report = String::new();

    report.push_str("=== Backtest Results ===\n\n");

    for result in results {
        report.push_str(&format!("Epoch {}\n", result.epoch));
        report.push_str(&format!(
            "Total validators scored: {}\n",
            result.validator_scores.len()
        ));

        // Top 10 validators
        report.push_str("\nTop 10 Validators:\n");
        for (i, validator) in result.validator_scores.iter().take(10).enumerate() {
            report.push_str(&format!(
                "  {}. {} - Score: {:.6}, Yield: {:.6}\n",
                i + 1,
                validator.vote_account,
                validator.score,
                validator.yield_score
            ));
        }

        // Score distribution
        if !result.validator_scores.is_empty() {
            let avg_score: f64 = result.validator_scores.iter().map(|v| v.score).sum::<f64>()
                / result.validator_scores.len() as f64;
            let max_score = result
                .validator_scores
                .iter()
                .map(|v| v.score)
                .fold(f64::MIN, f64::max);
            let min_score = result
                .validator_scores
                .iter()
                .map(|v| v.score)
                .fold(f64::MAX, f64::min);

            report.push_str("\nScore Statistics:\n");
            report.push_str(&format!("  Average: {:.6}\n", avg_score));
            report.push_str(&format!("  Max: {:.6}\n", max_score));
            report.push_str(&format!("  Min: {:.6}\n", min_score));
        }

        report.push_str(&format!("\n{}\n\n", "=".repeat(50)));
    }

    report
}

pub async fn command_view_backtest(
    client: &RpcClient,
    program_id: Pubkey,
    args: BacktestParameters,
) -> Result<()> {
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

    // Parse target epochs
    let target_epochs = if let Some(epochs) = args.target_epochs {
        epochs
    } else {
        // Default to last 3 epochs based on cached data
        let current_epoch = cached_data.fetched_epoch;
        vec![current_epoch - 2, current_epoch - 1, current_epoch]
    };

    info!("Running backtest for epochs: {:?}", target_epochs);

    // Run backtest with cached data
    let results = run_backtest_with_cached_data(&cached_data, target_epochs).await?;

    // Generate and print report
    let report = generate_comparison_report(&results);
    println!("{}", report);

    // Optionally save results to file
    if let Some(output_file) = args.output_file {
        let json = serde_json::to_string_pretty(&results)?;
        fs::write(&output_file, json)?;
        info!("Results saved to {:?}", output_file);
    }

    Ok(())
}

