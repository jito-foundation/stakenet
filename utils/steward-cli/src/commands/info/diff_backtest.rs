use crate::commands::command_args::DiffBacktest;
use crate::commands::info::view_backtest::ValidatorMetadata;
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

#[derive(Debug, Deserialize, Clone)]
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
    dropped_validators: Vec<ValidatorScoreResultJson>,
    added_validators: Vec<ValidatorScoreResultJson>,
}

#[derive(Debug)]
struct ScoreDistribution {
    file2_deciles: Vec<f64>,
}


/// Compare results from two backtest runs
pub async fn command_diff_backtest(args: DiffBacktest) -> Result<()> {
    println!("🔍 Analyzing differences between backtest strategies...\n");

    // Load both result files
    let file1_data: Vec<BacktestResultJson> = load_backtest_file(&args.file1)?;
    let file2_data: Vec<BacktestResultJson> = load_backtest_file(&args.file2)?;

    // Analyze scoring strategies
    analyze_scoring_strategies(&file1_data, &file2_data)?;

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
        print_epoch_analysis(&comparison, epoch1, epoch2);
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

fn analyze_scoring_strategies(
    file1_data: &[BacktestResultJson],
    file2_data: &[BacktestResultJson],
) -> Result<()> {
    if file1_data.is_empty() || file2_data.is_empty() {
        return Ok(());
    }

    // Sample first epoch to understand scoring strategies
    let epoch1 = &file1_data[0];
    let epoch2 = &file2_data[0];

    if epoch1.validator_scores.is_empty() || epoch2.validator_scores.is_empty() {
        return Ok(());
    }

    // Analyze File 1 scoring characteristics
    let file1_scores: Vec<f64> = epoch1.validator_scores
        .iter()
        .take(100)
        .map(|v| v.score_for_backtest_comparison)
        .collect();

    let file2_scores: Vec<f64> = epoch2.validator_scores
        .iter()
        .take(100)
        .map(|v| v.score_for_backtest_comparison)
        .collect();

    // Determine if scores look like production (continuous) vs MEV (discrete)
    let file1_unique_scores: std::collections::HashSet<_> = file1_scores
        .iter()
        .map(|&f| (f * 10000.0) as i32)  // Round to 4 decimal places
        .collect();

    let file2_unique_scores: std::collections::HashSet<_> = file2_scores
        .iter()
        .map(|&f| (f * 100.0) as i32)  // Round to 2 decimal places for MEV scores
        .collect();

    println!("🎯 SCORING STRATEGY ANALYSIS");
    println!("═══════════════════════════════════════════════════════════");
    
    // Check if file2 has MEV-style discrete scores
    let file2_has_mev_pattern = file2_unique_scores.len() <= 5 && 
        file2_scores.iter().any(|&s| s == 0.9 || s == 0.92 || s == 1.0);

    if file2_has_mev_pattern {
        println!("📊 File 1: Production Scoring (continuous yield-based scores)");
        println!("📊 File 2: MEV Commission Strategy (discrete commission-based scores)");
        println!("   • 1.0 = 0% MEV commission");
        println!("   • 0.92 = 8% MEV commission"); 
        println!("   • 0.9 = 10% MEV commission");
    } else {
        println!("📊 File 1: Strategy A (continuous scores)");
        println!("📊 File 2: Strategy B (continuous scores)");
    }

    println!("📈 Score Characteristics:");
    println!("   File 1: {} unique score values in sample", file1_unique_scores.len());
    println!("   File 2: {} unique score values in sample", file2_unique_scores.len());
    
    let file1_range = file1_scores.iter().fold((f64::INFINITY, f64::NEG_INFINITY), 
        |(min, max), &x| (min.min(x), max.max(x)));
    let file2_range = file2_scores.iter().fold((f64::INFINITY, f64::NEG_INFINITY), 
        |(min, max), &x| (min.min(x), max.max(x)));
    
    println!("   File 1 range: {:.6} - {:.6}", file1_range.0, file1_range.1);
    println!("   File 2 range: {:.6} - {:.6}", file2_range.0, file2_range.1);
    
    if file2_has_mev_pattern {
        println!("\n⚠️  NOTE: High churn is expected when comparing continuous vs discrete scoring!");
        println!("   Production scoring creates fine-grained rankings, while MEV strategy");
        println!("   groups validators into commission tiers, causing major rank changes.");
    }
    
    println!();
    Ok(())
}

fn analyze_epoch(
    epoch1: &BacktestResultJson,
    epoch2: &BacktestResultJson,
) -> Result<EpochComparison> {
    // Create rankings for both strategies
    let file1_ranking: Vec<_> = epoch1.validator_scores.iter().enumerate().collect();
    let file2_ranking: Vec<_> = epoch2.validator_scores.iter().enumerate().collect();

    // Get top 400 from each strategy - store both the pubkey and the validator object
    let file1_top_400_validators: Vec<_> = file1_ranking
        .iter()
        .take(400.min(file1_ranking.len()))
        .map(|(_, v)| (*v).clone())
        .collect();

    let file2_top_400_validators: Vec<_> = file2_ranking
        .iter()
        .take(400.min(file2_ranking.len()))
        .map(|(_, v)| (*v).clone())
        .collect();

    // Create sets of vote account strings for intersection/difference operations
    let file1_top_400_keys: HashSet<_> = file1_top_400_validators
        .iter()
        .map(|v| v.vote_account.to_string())
        .collect();

    let file2_top_400_keys: HashSet<_> = file2_top_400_validators
        .iter()
        .map(|v| v.vote_account.to_string())
        .collect();

    // Calculate churn
    let stayed_in_top_400 = file1_top_400_keys.intersection(&file2_top_400_keys).count();
    let dropped_from_top_400 = file1_top_400_keys.difference(&file2_top_400_keys).count();
    let added_to_top_400 = file2_top_400_keys.difference(&file1_top_400_keys).count();

    // Get the actual validator objects for dropped/added lists
    let dropped_keys: HashSet<_> = file1_top_400_keys.difference(&file2_top_400_keys).cloned().collect();
    let added_keys: HashSet<_> = file2_top_400_keys.difference(&file1_top_400_keys).cloned().collect();

    let dropped_validators: Vec<_> = file1_top_400_validators
        .into_iter()
        .filter(|v| dropped_keys.contains(&v.vote_account.to_string()))
        .collect();

    let added_validators: Vec<_> = file2_top_400_validators
        .into_iter()
        .filter(|v| added_keys.contains(&v.vote_account.to_string()))
        .collect();

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


    let score_distribution = ScoreDistribution {
        file2_deciles: calculate_deciles(&file2_scores),
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


fn print_epoch_analysis(comparison: &EpochComparison, epoch1: &BacktestResultJson, epoch2: &BacktestResultJson) {
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
            let mev_commission_pct = (1.0 - validator.mev_ranking_score) * 100.0;
            println!("  {}. {} [MEV: {:.0}%]", 
                i + 1, 
                format_validator_display(validator),
                mev_commission_pct
            );
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
            let mev_commission_pct = (1.0 - validator.mev_ranking_score) * 100.0;
            println!("  {}. {} [MEV: {:.0}%]", 
                i + 1, 
                format_validator_display(validator),
                mev_commission_pct
            );
        }
        if comparison.top_400_churn.added_validators.len() > 10 {
            println!(
                "  ... and {} more",
                comparison.top_400_churn.added_validators.len() - 10
            );
        }
    }

    // Add vote credit ratio analysis for top 400
    println!("\n📊 VOTE CREDIT RATIO DECILES (Top 400):");
    let file1_top_400_vote_ratios: Vec<f64> = epoch1.validator_scores
        .iter()
        .take(400)
        .map(|v| v.vote_credits_ratio)
        .collect();
    let file2_top_400_vote_ratios: Vec<f64> = epoch2.validator_scores
        .iter()
        .take(400)
        .map(|v| v.vote_credits_ratio)
        .collect();
    
    let file1_vote_deciles = calculate_deciles(&file1_top_400_vote_ratios);
    let file2_vote_deciles = calculate_deciles(&file2_top_400_vote_ratios);
    
    println!("  File 1 vote credit ratios: {:?}", 
             file1_vote_deciles.iter().map(|&x| format!("{:.4}", x)).collect::<Vec<_>>());
    println!("  File 2 vote credit ratios: {:?}", 
             file2_vote_deciles.iter().map(|&x| format!("{:.4}", x)).collect::<Vec<_>>());

    // Add MEV tier analysis if this looks like MEV vs production comparison
    let file2_scores: Vec<f64> = comparison.score_distribution.file2_deciles.clone();
    let has_mev_pattern = file2_scores.iter().any(|&s| s == 0.9 || s == 0.92 || s == 1.0);
    
    if has_mev_pattern {
        println!("\n🎯 MEV COMMISSION TIER ANALYSIS (File 2):");
        
        // Show some examples of high-performing validators that got dropped due to MEV commission
        println!("💡 HIGH-YIELD VALIDATORS DROPPED (MEV commission > 0%):");
        let high_yield_dropped: Vec<_> = comparison.top_400_churn.dropped_validators.iter()
            .filter(|v| v.yield_score > 0.995)  // High yield score
            .take(5)
            .collect();
            
        for (i, validator) in high_yield_dropped.iter().enumerate() {
            println!("  {}. {} (yield: {:.4}, MEV: {:.0}%)", 
                i + 1, 
                format_validator_display(validator),
                validator.yield_score,
                (1.0 - validator.mev_ranking_score) * 100.0
            );
        }
        
        // Add comprehensive bucket analysis for File 2 (MEV strategy)
        println!("\n📊 FULL MEV SCORE DISTRIBUTION (File 2):");
        analyze_mev_score_buckets(epoch2);
    }

    println!();
}

fn analyze_mev_score_buckets(epoch: &BacktestResultJson) {
    use std::collections::BTreeMap;
    
    // Group all validators by exact score_for_backtest_comparison value
    let mut score_buckets: BTreeMap<String, (usize, f64)> = BTreeMap::new();
    
    for validator in &epoch.validator_scores {
        let score = validator.score_for_backtest_comparison;
        let score_key = format!("{:.6}", score); // Exact score as key
        let entry = score_buckets.entry(score_key).or_insert((0, score));
        entry.0 += 1;
    }
    
    // Convert to sorted vector (highest score first)
    let mut sorted_buckets: Vec<_> = score_buckets.into_iter().collect();
    sorted_buckets.sort_by(|a, b| b.1.1.partial_cmp(&a.1.1).unwrap_or(std::cmp::Ordering::Equal));
    
    // Print exact score distribution
    println!("  Exact score buckets (total {} validators, sorted by score):", epoch.validator_scores.len());
    for (score_key, (count, score_val)) in &sorted_buckets {
        let percentage = *count as f64 / epoch.validator_scores.len() as f64 * 100.0;
        let commission_pct = (1.0 - score_val) * 100.0;
        println!("    • {} ({:.0}% MEV commission): {} validators ({:.1}%)", 
                 score_key, commission_pct, count, percentage);
    }
    
    // Show top 400 qualification
    let mut top_400_scores: BTreeMap<String, (usize, f64)> = BTreeMap::new();
    for validator in epoch.validator_scores.iter().take(400) {
        let score_key = format!("{:.6}", validator.score_for_backtest_comparison);
        let entry = top_400_scores.entry(score_key).or_insert((0, validator.score_for_backtest_comparison));
        entry.0 += 1;
    }
    
    // Convert to sorted vector (highest score first)
    let mut sorted_top_400: Vec<_> = top_400_scores.into_iter().collect();
    sorted_top_400.sort_by(|a, b| b.1.1.partial_cmp(&a.1.1).unwrap_or(std::cmp::Ordering::Equal));
    
    println!("  Top 400 breakdown (sorted by score):");
    for (score_key, (count, score_val)) in &sorted_top_400 {
        let commission_pct = (1.0 - score_val) * 100.0;
        println!("    • {} ({:.0}% MEV commission): {} validators in top 400", 
                 score_key, commission_pct, count);
    }
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
