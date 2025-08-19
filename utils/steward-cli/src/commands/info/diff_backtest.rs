use crate::commands::command_args::DiffBacktest;
use anyhow::Result;
use serde::{Deserialize, Deserializer};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
use std::fs;

// Custom deserializer-compatible structures for loading JSON
#[derive(Debug, Deserialize)]
struct BacktestResultJson {
    pub epoch: u64,
    pub validator_scores: Vec<ValidatorScoreResultJson>,
}

#[derive(Debug, Deserialize)]
struct ValidatorScoreResultJson {
    #[serde(deserialize_with = "deserialize_pubkey_from_base58")]
    pub vote_account: Pubkey,
    #[serde(default)]
    #[allow(dead_code)]
    pub validator_index: usize,
    #[serde(default)]
    #[allow(dead_code)]
    pub score: f64,
    #[serde(default)]
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub vote_credits_ratio: f64,
    #[serde(default)]
    #[allow(dead_code)]
    pub mev_ranking_score: f64,
    #[serde(default)]
    #[allow(dead_code)]
    pub validator_age: f64,
    pub score_for_backtest_comparison: f64,
}

fn deserialize_pubkey_from_base58<'de, D>(deserializer: D) -> Result<Pubkey, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = String::deserialize(deserializer)?;
    s.parse::<Pubkey>().map_err(serde::de::Error::custom)
}

#[derive(Debug)]
struct EpochComparison {
    epoch: u64,
    top_400_churn: ChurnAnalysis,
    score_distribution: ScoreDistribution,
}

#[derive(Debug)]
struct ChurnAnalysis {
    stayed_in_top_400: usize,
    dropped_from_top_400: usize,
    added_to_top_400: usize,
    dropped_validators: Vec<String>,
    added_validators: Vec<String>,
}

#[derive(Debug)]
struct ScoreDistribution {
    file1_deciles: Vec<f64>,
    file2_deciles: Vec<f64>,
    file1_top_400_stats: DistributionStats,
    file2_top_400_stats: DistributionStats,
}

#[derive(Debug)]
struct DistributionStats {
    mean: f64,
    median: f64,
    std_dev: f64,
    min: f64,
    max: f64,
}

/// Compare results from two backtest runs
pub async fn command_diff_backtest(args: DiffBacktest) -> Result<()> {
    println!("🔍 Analyzing differences between backtest strategies...\n");

    // Load both result files
    let file1_data: Vec<BacktestResultJson> = load_backtest_file(&args.file1)?;
    let file2_data: Vec<BacktestResultJson> = load_backtest_file(&args.file2)?;

    println!(
        "📁 Loaded {} epochs from file1: {:?}",
        file1_data.len(),
        args.file1
    );
    println!(
        "📁 Loaded {} epochs from file2: {:?}",
        file2_data.len(),
        args.file2
    );

    if file1_data.len() != file2_data.len() {
        return Err(anyhow::anyhow!("Files have different number of epochs"));
    }

    // Compare each epoch
    for (epoch1, epoch2) in file1_data.iter().zip(file2_data.iter()) {
        if epoch1.epoch != epoch2.epoch {
            return Err(anyhow::anyhow!(
                "Epoch mismatch: {} vs {}",
                epoch1.epoch,
                epoch2.epoch
            ));
        }

        let comparison = analyze_epoch(epoch1, epoch2)?;
        print_epoch_analysis(&comparison);
    }

    // Generate overall summary if multiple epochs
    if file1_data.len() > 1 {
        print_overall_summary(&file1_data, &file2_data)?;
    }

    Ok(())
}

fn load_backtest_file(path: &std::path::PathBuf) -> Result<Vec<BacktestResultJson>> {
    let contents = fs::read_to_string(path)?;
    let data: Vec<BacktestResultJson> = serde_json::from_str(&contents)?;
    Ok(data)
}

fn analyze_epoch(
    epoch1: &BacktestResultJson,
    epoch2: &BacktestResultJson,
) -> Result<EpochComparison> {
    // Create rankings for both strategies
    let file1_ranking: Vec<_> = epoch1.validator_scores.iter().enumerate().collect();
    let file2_ranking: Vec<_> = epoch2.validator_scores.iter().enumerate().collect();

    // Get top 400 from each strategy
    let file1_top_400: HashSet<_> = file1_ranking
        .iter()
        .take(400.min(file1_ranking.len()))
        .map(|(_, v)| v.vote_account.to_string())
        .collect();

    let file2_top_400: HashSet<_> = file2_ranking
        .iter()
        .take(400.min(file2_ranking.len()))
        .map(|(_, v)| v.vote_account.to_string())
        .collect();

    // Calculate churn
    let stayed_in_top_400 = file1_top_400.intersection(&file2_top_400).count();
    let dropped_from_top_400 = file1_top_400.difference(&file2_top_400).count();
    let added_to_top_400 = file2_top_400.difference(&file1_top_400).count();

    let dropped_validators: Vec<_> = file1_top_400.difference(&file2_top_400).cloned().collect();
    let added_validators: Vec<_> = file2_top_400.difference(&file1_top_400).cloned().collect();

    let churn = ChurnAnalysis {
        stayed_in_top_400,
        dropped_from_top_400,
        added_to_top_400,
        dropped_validators,
        added_validators,
    };

    // Calculate score distributions
    let file1_scores: Vec<_> = epoch1
        .validator_scores
        .iter()
        .map(|v| v.score_for_backtest_comparison)
        .collect();
    let file2_scores: Vec<_> = epoch2
        .validator_scores
        .iter()
        .map(|v| v.score_for_backtest_comparison)
        .collect();

    let file1_top_400_scores: Vec<_> = file1_scores.iter().take(400).copied().collect();
    let file2_top_400_scores: Vec<_> = file2_scores.iter().take(400).copied().collect();

    let score_distribution = ScoreDistribution {
        file1_deciles: calculate_deciles(&file1_scores),
        file2_deciles: calculate_deciles(&file2_scores),
        file1_top_400_stats: calculate_stats(&file1_top_400_scores),
        file2_top_400_stats: calculate_stats(&file2_top_400_scores),
    };

    Ok(EpochComparison {
        epoch: epoch1.epoch,
        top_400_churn: churn,
        score_distribution,
    })
}

fn calculate_deciles(scores: &[f64]) -> Vec<f64> {
    let mut sorted_scores = scores.to_vec();
    sorted_scores.sort_by(|a, b| a.partial_cmp(b).unwrap());

    (0..=10)
        .map(|i| {
            let index = (i * sorted_scores.len()) / 10;
            let clamped_index = index.min(sorted_scores.len().saturating_sub(1));
            sorted_scores[clamped_index]
        })
        .collect()
}

fn calculate_stats(scores: &[f64]) -> DistributionStats {
    if scores.is_empty() {
        return DistributionStats {
            mean: 0.0,
            median: 0.0,
            std_dev: 0.0,
            min: 0.0,
            max: 0.0,
        };
    }

    let mut sorted_scores = scores.to_vec();
    sorted_scores.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let mean = scores.iter().sum::<f64>() / scores.len() as f64;
    let median = sorted_scores[sorted_scores.len() / 2];
    let variance = scores.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / scores.len() as f64;
    let std_dev = variance.sqrt();

    DistributionStats {
        mean,
        median,
        std_dev,
        min: sorted_scores[0],
        max: sorted_scores[sorted_scores.len() - 1],
    }
}

fn print_epoch_analysis(comparison: &EpochComparison) {
    println!("═══════════════════════════════════════════════════════════");
    println!("📊 EPOCH {} ANALYSIS", comparison.epoch);
    println!("═══════════════════════════════════════════════════════════");

    println!("\n🔄 TOP 400 CHURN ANALYSIS:");
    println!(
        "  • Stayed in top 400: {}",
        comparison.top_400_churn.stayed_in_top_400
    );
    println!(
        "  • Dropped from top 400: {}",
        comparison.top_400_churn.dropped_from_top_400
    );
    println!(
        "  • Added to top 400: {}",
        comparison.top_400_churn.added_to_top_400
    );
    println!(
        "  • Churn rate: {:.1}%",
        (comparison.top_400_churn.dropped_from_top_400 + comparison.top_400_churn.added_to_top_400)
            as f64
            / 400.0
            * 100.0
    );

    if !comparison.top_400_churn.dropped_validators.is_empty() {
        println!("\n❌ DROPPED FROM TOP 400 (first 10):");
        for (i, validator) in comparison
            .top_400_churn
            .dropped_validators
            .iter()
            .take(10)
            .enumerate()
        {
            println!("  {}. {}", i + 1, validator);
        }
        if comparison.top_400_churn.dropped_validators.len() > 10 {
            println!(
                "  ... and {} more",
                comparison.top_400_churn.dropped_validators.len() - 10
            );
        }
    }

    if !comparison.top_400_churn.added_validators.is_empty() {
        println!("\n✅ ADDED TO TOP 400 (first 10):");
        for (i, validator) in comparison
            .top_400_churn
            .added_validators
            .iter()
            .take(10)
            .enumerate()
        {
            println!("  {}. {}", i + 1, validator);
        }
        if comparison.top_400_churn.added_validators.len() > 10 {
            println!(
                "  ... and {} more",
                comparison.top_400_churn.added_validators.len() - 10
            );
        }
    }

    println!("\n📈 SCORE DISTRIBUTION (Top 400):");
    println!("  File 1 (Strategy A):");
    print_stats("    ", &comparison.score_distribution.file1_top_400_stats);
    println!("  File 2 (Strategy B):");
    print_stats("    ", &comparison.score_distribution.file2_top_400_stats);

    println!("\n📊 GLOBAL SCORE DECILES:");
    println!(
        "  File 1: {:?}",
        comparison.score_distribution.file1_deciles
    );
    println!(
        "  File 2: {:?}",
        comparison.score_distribution.file2_deciles
    );

    println!();
}

fn print_stats(prefix: &str, stats: &DistributionStats) {
    println!(
        "{}Mean: {:.6}, Median: {:.6}",
        prefix, stats.mean, stats.median
    );
    println!(
        "{}Std Dev: {:.6}, Min: {:.6}, Max: {:.6}",
        prefix, stats.std_dev, stats.min, stats.max
    );
}

fn print_overall_summary(
    file1_data: &[BacktestResultJson],
    file2_data: &[BacktestResultJson],
) -> Result<()> {
    println!("\n🎯 OVERALL SUMMARY ACROSS ALL EPOCHS");
    println!("═══════════════════════════════════════════════════════════");

    let mut total_churn = 0;
    let mut total_opportunities = 0;

    for (epoch1, epoch2) in file1_data.iter().zip(file2_data.iter()) {
        let comparison = analyze_epoch(epoch1, epoch2)?;
        total_churn += comparison.top_400_churn.dropped_from_top_400
            + comparison.top_400_churn.added_to_top_400;
        total_opportunities += 800; // 400 slots × 2 (drop + add)
    }

    let average_churn_rate = total_churn as f64 / total_opportunities as f64 * 100.0;

    println!(
        "📊 Average churn rate across all epochs: {:.1}%",
        average_churn_rate
    );
    println!("📊 Total epochs analyzed: {}", file1_data.len());

    Ok(())
}
