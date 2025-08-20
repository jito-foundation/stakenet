use crate::commands::command_args::DiffBacktest;
use crate::commands::info::view_backtest::ValidatorMetadata;
use anyhow::Result;
use serde::{Deserialize, Deserializer};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
use std::fs;

// Custom deserializer-compatible structures for loading JSON
#[derive(Debug, Deserialize, Clone)]
struct BacktestResultJson {
    pub epoch: u64,
    pub validator_scores: Vec<ValidatorScoreResultJson>,
}

#[derive(Debug, Deserialize, Clone)]
struct ValidatorScoreResultJson {
    #[serde(deserialize_with = "deserialize_pubkey_from_base58")]
    pub vote_account: Pubkey,
    #[serde(default)]
    #[allow(dead_code)]
    pub validator_index: usize,

    // Production scoring
    #[serde(default)]
    pub production_score: f64,
    #[serde(default)]
    pub production_rank: Option<usize>,

    // Proposed scoring
    #[serde(default)]
    pub proposed_score: f64,
    #[serde(default)]
    pub proposed_delinquency_score: f64,
    #[serde(default)]
    pub proposed_rank: Option<usize>,

    // Component scores
    #[serde(default)]
    pub yield_score: f64,
    #[serde(default)]
    #[allow(dead_code)]
    pub mev_commission_score: f64,
    #[serde(default)]
    #[allow(dead_code)]
    pub blacklisted_score: f64,
    #[serde(default)]
    #[allow(dead_code)]
    pub superminority_score: f64,
    #[serde(default)]
    pub delinquency_score: f64,
    #[serde(default)]
    #[allow(dead_code)]
    pub running_jito_score: f64,
    #[serde(default)]
    #[allow(dead_code)]
    pub commission_score: f64,
    #[serde(default)]
    #[allow(dead_code)]
    pub historical_commission_score: f64,
    #[serde(default)]
    pub vote_credits_ratio: f64,

    // Additional metrics
    #[serde(default)]
    pub mev_commission_pct: f64,
    #[serde(default)]
    #[allow(dead_code)]
    pub validator_age: f64,
    #[serde(default)]
    pub metadata: ValidatorMetadata,
}

fn deserialize_pubkey_from_base58<'de, D>(deserializer: D) -> Result<Pubkey, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = String::deserialize(deserializer)?;
    s.parse::<Pubkey>().map_err(serde::de::Error::custom)
}

/// Format validator display with name if available, fallback to truncated pubkey
fn format_validator_display(validator: &ValidatorScoreResultJson) -> String {
    if let Some(name) = &validator.metadata.name {
        name.clone()
    } else {
        format!("{}...", &validator.vote_account.to_string()[..12])
    }
}

/// Compare results from two backtest runs
pub async fn command_diff_backtest(args: DiffBacktest) -> Result<()> {
    println!("🔍 Analyzing differences between production and proposed strategies...\n");

    // Load the single result file
    let data: Vec<BacktestResultJson> = load_backtest_file(&args.file)?;

    println!("📁 Loaded {} epochs from: {:?}\n", data.len(), args.file);

    // Analyze each epoch
    for epoch_data in &data {
        println!("═══════════════════════════════════════════════════════════");
        println!("📊 EPOCH {} ANALYSIS", epoch_data.epoch);
        println!("═══════════════════════════════════════════════════════════");

        // Get validators in top 400 for each strategy
        let production_top_400: HashSet<String> = epoch_data
            .validator_scores
            .iter()
            .filter(|v| v.production_rank.map_or(false, |r| r <= 400))
            .map(|v| v.vote_account.to_string())
            .collect();

        let proposed_top_400: HashSet<String> = epoch_data
            .validator_scores
            .iter()
            .filter(|v| v.proposed_rank.map_or(false, |r| r <= 400))
            .map(|v| v.vote_account.to_string())
            .collect();

        // Calculate churn
        let stayed = production_top_400.intersection(&proposed_top_400).count();
        let dropped = production_top_400.difference(&proposed_top_400).count();
        let added = proposed_top_400.difference(&production_top_400).count();

        println!("\n🔄 TOP 400 CHURN ANALYSIS:");
        println!("  • Stayed in top 400: {}", stayed);
        println!("  • Dropped from top 400: {}", dropped);
        println!("  • Added to top 400: {}", added);
        println!("  • Churn rate: {:.1}%", dropped as f64 / 4.0);

        // Get the actual dropped validators
        let dropped_validators: Vec<&ValidatorScoreResultJson> = epoch_data
            .validator_scores
            .iter()
            .filter(|v| {
                v.production_rank.map_or(false, |r| r <= 400)
                    && v.proposed_rank.map_or(true, |r| r > 400)
            })
            .collect();

        if !dropped_validators.is_empty() {
            println!(
                "\n❌ VALIDATORS DROPPED FROM TOP 400 ({} total):",
                dropped_validators.len()
            );
            for (i, validator) in dropped_validators.iter().take(10).enumerate() {
                let reason = if validator.proposed_score == 0.0 {
                    if validator.proposed_delinquency_score == 0.0 {
                        "Failed 99% delinquency threshold"
                    } else if validator.production_score == 0.0 {
                        "Failed binary filter in production"
                    } else {
                        "Failed binary filter"
                    }
                } else {
                    "Ranked below top 400"
                };

                println!(
                    "  {}. {} [Prod rank: {}, Proposed rank: {}, MEV: {:.1}%] - {}",
                    i + 1,
                    format_validator_display(validator),
                    validator.production_rank.unwrap_or(0),
                    validator
                        .proposed_rank
                        .map_or("N/A".to_string(), |r| r.to_string()),
                    validator.mev_commission_pct,
                    reason
                );
            }
            if dropped_validators.len() > 10 {
                println!("  ... and {} more", dropped_validators.len() - 10);
            }
        }

        // Get the actual added validators
        let added_validators: Vec<&ValidatorScoreResultJson> = epoch_data
            .validator_scores
            .iter()
            .filter(|v| {
                v.production_rank.map_or(true, |r| r > 400)
                    && v.proposed_rank.map_or(false, |r| r <= 400)
            })
            .collect();

        if !added_validators.is_empty() {
            println!(
                "\n✅ VALIDATORS ADDED TO TOP 400 ({} total):",
                added_validators.len()
            );
            for (i, validator) in added_validators.iter().take(10).enumerate() {
                let reason = if validator.mev_commission_pct < 0.1 {
                    "0% MEV commission"
                } else if validator.vote_credits_ratio >= 0.99 {
                    "High performance"
                } else {
                    "MEV strategy preference"
                };

                println!(
                    "  {}. {} [Prod rank: {}, Proposed rank: {}, MEV: {:.1}%] - {}",
                    i + 1,
                    format_validator_display(validator),
                    validator
                        .production_rank
                        .map_or("N/A".to_string(), |r| r.to_string()),
                    validator.proposed_rank.unwrap_or(0),
                    validator.mev_commission_pct,
                    reason
                );
            }
            if added_validators.len() > 10 {
                println!("  ... and {} more", added_validators.len() - 10);
            }
        }

        // Show delinquency threshold impact
        let failed_99_threshold = epoch_data
            .validator_scores
            .iter()
            .filter(|v| v.delinquency_score > 0.0 && v.proposed_delinquency_score == 0.0)
            .count();

        if failed_99_threshold > 0 {
            println!("\n⚠️  DELINQUENCY THRESHOLD IMPACT:");
            println!(
                "  {} validators passed production threshold but failed 99% threshold",
                failed_99_threshold
            );
        }

        // Show summary statistics
        println!("\n📊 YIELD SCORE DISTRIBUTION STATISTICS:");
        
        // Top 400 validators - production strategy
        let top_400_production_yields: Vec<f64> = epoch_data
            .validator_scores
            .iter()
            .filter(|v| v.production_rank.map_or(false, |r| r <= 400))
            .map(|v| v.yield_score)
            .collect();
            
        // Top 400 validators - proposed strategy
        let top_400_proposed_yields: Vec<f64> = epoch_data
            .validator_scores
            .iter()
            .filter(|v| v.proposed_rank.map_or(false, |r| r <= 400))
            .map(|v| v.yield_score)
            .collect();
            
        println!("  Top 400 Production Yield Deciles: {:?}", 
            format_deciles(&calculate_deciles(&top_400_production_yields)));
        println!("  Top 400 Proposed Yield Deciles:   {:?}", 
            format_deciles(&calculate_deciles(&top_400_proposed_yields)));
            
        // Dropped validators yield scores
        if !dropped_validators.is_empty() {
            let dropped_yields: Vec<f64> = dropped_validators
                .iter()
                .map(|v| v.yield_score)
                .collect();
                
            println!("  Dropped Validators Yield Deciles:  {:?}", 
                format_deciles(&calculate_deciles(&dropped_yields)));
        }
        
        // Added validators yield scores
        if !added_validators.is_empty() {
            let added_yields: Vec<f64> = added_validators
                .iter()
                .map(|v| v.yield_score)
                .collect();
                
            println!("  Added Validators Yield Deciles:    {:?}", 
                format_deciles(&calculate_deciles(&added_yields)));
        }

        println!();
    }

    // Overall summary
    if data.len() > 1 {
        println!("\n🎯 OVERALL SUMMARY ACROSS ALL EPOCHS");
        println!("═══════════════════════════════════════════════════════════");

        let mut total_dropped = 0;
        let mut total_added = 0;
        let mut total_stayed = 0;

        for epoch_data in &data {
            let production_top_400: HashSet<String> = epoch_data
                .validator_scores
                .iter()
                .filter(|v| v.production_rank.map_or(false, |r| r <= 400))
                .map(|v| v.vote_account.to_string())
                .collect();

            let proposed_top_400: HashSet<String> = epoch_data
                .validator_scores
                .iter()
                .filter(|v| v.proposed_rank.map_or(false, |r| r <= 400))
                .map(|v| v.vote_account.to_string())
                .collect();

            total_stayed += production_top_400.intersection(&proposed_top_400).count();
            total_dropped += production_top_400.difference(&proposed_top_400).count();
            total_added += proposed_top_400.difference(&production_top_400).count();
        }

        let total_slots = data.len() * 400;
        let avg_churn_rate = (total_dropped as f64 / total_slots as f64) * 100.0;

        println!("📊 Total epochs analyzed: {}", data.len());
        println!("📊 Average churn rate: {:.1}%", avg_churn_rate);
        println!("📊 Total validators dropped: {}", total_dropped);
        println!("📊 Total validators added: {}", total_added);
        println!("📊 Total stable positions: {}", total_stayed);
    }

    Ok(())
}

fn load_backtest_file(path: &std::path::PathBuf) -> Result<Vec<BacktestResultJson>> {
    let contents = fs::read_to_string(path)?;
    let data: Vec<BacktestResultJson> = serde_json::from_str(&contents)?;
    Ok(data)
}

fn calculate_deciles(values: &[f64]) -> Vec<f64> {
    if values.is_empty() {
        return vec![0.0; 11];
    }
    
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    (0..=10)
        .map(|i| {
            let index = (i * sorted.len()) / 10;
            let clamped_index = index.min(sorted.len().saturating_sub(1));
            sorted[clamped_index]
        })
        .collect()
}

fn format_deciles(deciles: &[f64]) -> Vec<String> {
    deciles.iter().map(|&x| format!("{:.4}", x)).collect()
}

