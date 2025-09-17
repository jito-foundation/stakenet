use std::fs;
use std::path::Path;

use anyhow::Result;
use jito_steward::constants::TVC_ACTIVATION_EPOCH;
use jito_steward::score::{validator_score, ScoreComponentsV4};
use log::info;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::pubkey::Pubkey;

use crate::commands::command_args::BacktestParameters;
use crate::commands::info::create_backtest_cache::{CachedBacktestData, ValidatorMetadata};

use serde::{Deserialize, Serialize};

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

    // Score from the new scoring system
    pub score: u64,          // Final score with filters applied
    pub raw_score: u64,      // Raw 4-tier encoded score
    pub rank: Option<usize>, // Rank based on score (1-based)

    // Binary filter scores (0 or 1)
    pub mev_commission_score: u8,
    pub blacklisted_score: u8,
    pub superminority_score: u8,
    pub delinquency_score: u8,
    pub running_jito_score: u8,
    pub commission_score: u8,
    pub historical_commission_score: u8,
    pub merkle_root_upload_authority_score: u8,
    pub priority_fee_commission_score: u8,
    pub priority_fee_merkle_root_upload_authority_score: u8,

    // 4-tier score components
    pub commission_max: u8,        // Max inflation commission (0-100)
    pub mev_commission_avg: u16,   // Average MEV commission (basis points)
    pub validator_age: u32,        // Epochs with non-zero vote credits
    pub vote_credits_avg: u32,     // Normalized vote credits ratio scaled by 10M

    // Metadata
    pub metadata: ValidatorMetadata,
}

fn serialize_pubkey_as_base58<S>(pubkey: &Pubkey, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&pubkey.to_string())
}

impl ValidatorScoreResult {
    fn from_score_components(
        vote_account: Pubkey,
        index: usize,
        components: ScoreComponentsV4,
        metadata: ValidatorMetadata,
    ) -> Self {
        ValidatorScoreResult {
            vote_account,
            validator_index: index,
            score: components.score,
            raw_score: components.raw_score,
            rank: None, // Will be set after sorting
            mev_commission_score: components.mev_commission_score,
            blacklisted_score: components.blacklisted_score,
            superminority_score: components.superminority_score,
            delinquency_score: components.delinquency_score,
            running_jito_score: components.running_jito_score,
            commission_score: components.commission_score,
            historical_commission_score: components.historical_commission_score,
            merkle_root_upload_authority_score: components.merkle_root_upload_authority_score,
            priority_fee_commission_score: components.priority_fee_commission_score,
            priority_fee_merkle_root_upload_authority_score: components.priority_fee_merkle_root_upload_authority_score,
            commission_max: components.commission_max,
            mev_commission_avg: components.mev_commission_avg,
            validator_age: components.validator_age,
            vote_credits_avg: components.vote_credits_avg,
            metadata,
        }
    }
}

fn load_cached_data(cache_file: &std::path::Path) -> Result<CachedBacktestData> {
    info!("Loading data from cache file: {:?}", cache_file);
    let json = fs::read_to_string(cache_file)?;
    info!("Deserializing cached data ({} bytes)...", json.len());
    let data = serde_json::from_str(&json)?;
    info!("Cache loaded successfully");
    Ok(data)
}

async fn run_backtest_with_cached_data(
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

            // Skip if validator doesn't have sufficient history
            if validator_history.history.idx == 0 {
                skipped_count += 1;
                continue;
            }

            // Compute score using the new scoring system
            match validator_score(
                &validator_history,
                &cluster_history,
                &config,
                *epoch as u16,
                TVC_ACTIVATION_EPOCH,
            ) {
                Ok(score_components) => {
                    // Get metadata for this validator
                    let metadata = cached_data
                        .validator_metadata
                        .get(&vote_account.to_string())
                        .cloned()
                        .unwrap_or_default();

                    let result = ValidatorScoreResult::from_score_components(
                        *vote_account,
                        i,
                        score_components,
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

        // Sort by score (higher is better)
        validator_scores.sort_by(|a, b| b.score.cmp(&a.score));

        // Assign ranks (1-based)
        for (rank, validator) in validator_scores.iter_mut().enumerate() {
            validator.rank = Some(rank + 1);
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

fn export_results_to_csv(results: &[BacktestResult], export_dir: &Path) -> Result<()> {
    // Create export directory if it doesn't exist
    fs::create_dir_all(export_dir)?;

    for result in results {
        let csv_path = export_dir.join(format!("epoch_{}_validators.csv", result.epoch));
        let mut csv = String::new();

        // Header
        csv.push_str("Rank,Vote Account,Name,Score,Raw Score,Commission Max %,MEV Commission Avg (bps),Validator Age,Vote Credits Avg,");
        csv.push_str("MEV Commission Filter,Blacklisted Filter,Superminority Filter,Delinquency Filter,Running Jito Filter,");
        csv.push_str("Commission Filter,Historical Commission Filter,Merkle Root Authority Filter,");
        csv.push_str("Priority Fee Commission Filter,Priority Fee Merkle Authority Filter\n");

        // Data rows
        for validator in &result.validator_scores {
            csv.push_str(&format!(
                "{},\"{}\",\"{}\",{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                validator.rank.unwrap_or(0),
                validator.vote_account,
                validator.metadata.name.as_ref().unwrap_or(&"Unknown".to_string()).replace("\"", "\"\""),
                validator.score,
                validator.raw_score,
                validator.commission_max,
                validator.mev_commission_avg,
                validator.validator_age,
                validator.vote_credits_avg,
                validator.mev_commission_score,
                validator.blacklisted_score,
                validator.superminority_score,
                validator.delinquency_score,
                validator.running_jito_score,
                validator.commission_score,
                validator.historical_commission_score,
                validator.merkle_root_upload_authority_score,
                validator.priority_fee_commission_score,
                validator.priority_fee_merkle_root_upload_authority_score,
            ));
        }

        fs::write(&csv_path, csv)?;
        info!("   • Exported {}", csv_path.display());
    }

    Ok(())
}

pub async fn command_run_backtest(
    _client: &RpcClient,
    _program_id: Pubkey,
    args: BacktestParameters,
) -> Result<()> {
    // Check if output file already exists
    if args.output_file.exists() {
        return Err(anyhow::anyhow!(
            "Output file {:?} already exists. Please choose a different filename or delete the existing file.",
            args.output_file
        ));
    }

    // Load cached data (no longer create it here)
    if !args.cache_file.exists() {
        return Err(anyhow::anyhow!(
            "Cache file {:?} not found. Please run 'create-backtest-cache' first to create the cache.",
            args.cache_file
        ));
    }

    let cached_data = load_cached_data(&args.cache_file)?;

    // Determine start epoch and calculate target epochs
    let start_epoch = if let Some(epoch) = args.start_epoch {
        epoch
    } else {
        // Default to current epoch - 1
        cached_data.fetched_epoch.saturating_sub(1)
    };

    // Calculate target epochs aligned with rebalancing schedule (every 10 epochs)
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

    // Run backtest with cached data
    let results = run_backtest_with_cached_data(&cached_data, target_epochs).await?;

    info!("Backtest analysis complete for {} epochs", results.len());

    // Save results to JSON file
    info!("Saving detailed results to file...");
    let json = serde_json::to_string_pretty(&results)?;
    let json_len = json.len();
    fs::write(&args.output_file, json)?;
    info!(
        "Results saved to {:?} ({} bytes)",
        args.output_file, json_len
    );

    // Export to CSV if requested
    if args.export_csv {
        info!("Exporting results to CSV format...");
        export_results_to_csv(&results, &args.export_dir)?;
        info!("CSV export complete: {}", args.export_dir.display());
    }

    info!("✅ Backtest complete!");

    Ok(())
}