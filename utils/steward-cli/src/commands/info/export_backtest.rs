use crate::commands::command_args::ExportBacktest;
use crate::commands::info::view_backtest::ValidatorMetadata;
use anyhow::Result;
use serde::{Deserialize, Deserializer};
use solana_sdk::pubkey::Pubkey;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;

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
    #[serde(default)]
    pub proposed_delinquency_epoch: Option<u16>,
    #[serde(default)]
    pub proposed_delinquency_ratio: Option<f64>,

    // Component scores
    #[serde(default)]
    pub yield_score: f64,
    #[serde(default)]
    pub mev_commission_score: f64,
    #[serde(default)]
    pub blacklisted_score: f64,
    #[serde(default)]
    pub superminority_score: f64,
    #[serde(default)]
    pub delinquency_score: f64,
    #[serde(default)]
    pub running_jito_score: f64,
    #[serde(default)]
    pub commission_score: f64,
    #[serde(default)]
    pub historical_commission_score: f64,
    #[serde(default)]
    pub vote_credits_ratio: f64,

    // Additional metrics
    #[serde(default)]
    pub mev_commission_pct: f64,
    #[serde(default)]
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

#[derive(Debug)]
struct EpochComparison {
    epoch: u64,
    top_400_churn: ChurnAnalysis,
    #[allow(dead_code)]
    validator_counts: ValidatorCounts,
    #[allow(dead_code)]
    vote_credit_stats: VoteCreditStats,
    #[allow(dead_code)]
    mev_distribution: MevDistribution,
}

#[derive(Debug)]
struct ChurnAnalysis {
    #[allow(dead_code)]
    stayed_in_top_400: usize,
    #[allow(dead_code)]
    dropped_from_top_400: usize,
    #[allow(dead_code)]
    added_to_top_400: usize,
    dropped_validators: Vec<ValidatorWithRank>,
    added_validators: Vec<ValidatorWithRank>,
}

#[derive(Debug)]
struct ValidatorWithRank {
    validator: ValidatorScoreResultJson,
    production_rank: Option<usize>,
    proposed_rank: Option<usize>,
}

#[derive(Debug)]
#[allow(dead_code)]
struct ValidatorCounts {
    production_total: usize,
    proposed_total: usize,
    production_meeting_threshold: usize,
    proposed_meeting_threshold: usize,
}

#[derive(Debug)]
#[allow(dead_code)]
struct VoteCreditStats {
    production_deciles: Vec<f64>,
    proposed_deciles: Vec<f64>,
}

#[derive(Debug)]
#[allow(dead_code)]
struct MevDistribution {
    buckets: BTreeMap<String, (usize, usize)>, // (total_count, top_400_count)
}

fn format_validator_display(validator: &ValidatorScoreResultJson) -> String {
    if let Some(name) = &validator.metadata.name {
        name.clone()
    } else {
        format!("{}...", &validator.vote_account.to_string()[..12])
    }
}

fn get_mev_commission_percent(validator: &ValidatorScoreResultJson) -> f64 {
    validator.mev_commission_pct
}

pub async fn command_export_backtest(args: ExportBacktest) -> Result<()> {
    println!("📦 Exporting validator added/dropped lists for each epoch...\n");

    // Create output directory
    fs::create_dir_all(&args.output_dir)?;

    // Load the combined results file
    let data: Vec<BacktestResultJson> = load_backtest_file(&args.file)?;

    println!("📁 Loaded {} epochs from: {:?}", data.len(), args.file);

    // Analyze and export each epoch separately
    for epoch_data in &data {
        let comparison = analyze_epoch_single_file(epoch_data)?;
        export_epoch_validator_csvs(&comparison, &args.output_dir)?;
    }

    println!("\n✅ Export completed successfully!");
    println!("📂 Output files saved to: {}", args.output_dir.display());
    println!("📋 Files created:");

    for epoch_data in &data {
        println!("   • epoch_{}_validators_dropped.csv", epoch_data.epoch);
        println!("   • epoch_{}_validators_added.csv", epoch_data.epoch);
    }

    Ok(())
}

fn load_backtest_file(path: &std::path::PathBuf) -> Result<Vec<BacktestResultJson>> {
    let contents = fs::read_to_string(path)?;
    let data: Vec<BacktestResultJson> = serde_json::from_str(&contents)?;
    Ok(data)
}

fn analyze_epoch_single_file(epoch_data: &BacktestResultJson) -> Result<EpochComparison> {
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
    let stayed_in_top_400 = production_top_400.intersection(&proposed_top_400).count();
    let dropped_from_top_400 = production_top_400.difference(&proposed_top_400).count();
    let added_to_top_400 = proposed_top_400.difference(&production_top_400).count();

    // Get dropped validators
    let dropped_validators: Vec<ValidatorWithRank> = epoch_data
        .validator_scores
        .iter()
        .filter(|v| {
            v.production_rank.map_or(false, |r| r <= 400)
                && v.proposed_rank.map_or(true, |r| r > 400)
        })
        .map(|v| ValidatorWithRank {
            validator: v.clone(),
            production_rank: v.production_rank,
            proposed_rank: v.proposed_rank,
        })
        .collect();

    // Get added validators
    let added_validators: Vec<ValidatorWithRank> = epoch_data
        .validator_scores
        .iter()
        .filter(|v| {
            v.production_rank.map_or(true, |r| r > 400)
                && v.proposed_rank.map_or(false, |r| r <= 400)
        })
        .map(|v| ValidatorWithRank {
            validator: v.clone(),
            production_rank: v.production_rank,
            proposed_rank: v.proposed_rank,
        })
        .collect();

    let churn = ChurnAnalysis {
        stayed_in_top_400,
        dropped_from_top_400,
        added_to_top_400,
        dropped_validators,
        added_validators,
    };

    // Calculate validator counts
    let production_meeting_threshold = epoch_data
        .validator_scores
        .iter()
        .filter(|v| v.vote_credits_ratio >= 0.99)
        .count();

    let proposed_meeting_threshold = epoch_data
        .validator_scores
        .iter()
        .filter(|v| v.proposed_delinquency_score > 0.0)
        .count();

    let validator_counts = ValidatorCounts {
        production_total: epoch_data.validator_scores.len(),
        proposed_total: epoch_data.validator_scores.len(),
        production_meeting_threshold,
        proposed_meeting_threshold,
    };

    // Calculate vote credit statistics for top 400 validators
    let production_vote_ratios: Vec<f64> = epoch_data
        .validator_scores
        .iter()
        .filter(|v| v.production_rank.map_or(false, |r| r <= 400))
        .map(|v| v.vote_credits_ratio)
        .collect();

    let proposed_vote_ratios: Vec<f64> = epoch_data
        .validator_scores
        .iter()
        .filter(|v| v.proposed_rank.map_or(false, |r| r <= 400))
        .map(|v| v.vote_credits_ratio)
        .collect();

    let vote_credit_stats = VoteCreditStats {
        production_deciles: calculate_deciles(&production_vote_ratios),
        proposed_deciles: calculate_deciles(&proposed_vote_ratios),
    };

    // Calculate MEV distribution
    let mev_distribution = calculate_mev_distribution_single(epoch_data, &proposed_top_400);

    Ok(EpochComparison {
        epoch: epoch_data.epoch,
        top_400_churn: churn,
        validator_counts,
        vote_credit_stats,
        mev_distribution,
    })
}

fn calculate_deciles(values: &[f64]) -> Vec<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    (0..=10)
        .map(|i| {
            let index = (i * sorted.len()) / 10;
            let clamped_index = index.min(sorted.len().saturating_sub(1));
            sorted[clamped_index]
        })
        .collect()
}

fn calculate_mev_distribution_single(
    epoch: &BacktestResultJson,
    top_400_keys: &HashSet<String>,
) -> MevDistribution {
    let mut buckets: BTreeMap<String, (usize, usize)> = BTreeMap::new();

    for validator in &epoch.validator_scores {
        let mev_commission = get_mev_commission_percent(validator);
        let bucket = if mev_commission < 0.1 {
            "0%".to_string()
        } else if mev_commission <= 5.0 {
            "1-5%".to_string()
        } else if mev_commission <= 8.0 {
            "6-8%".to_string()
        } else if mev_commission <= 10.0 {
            "9-10%".to_string()
        } else {
            ">10%".to_string()
        };

        let entry = buckets.entry(bucket).or_insert((0, 0));
        entry.0 += 1;

        if top_400_keys.contains(&validator.vote_account.to_string()) {
            entry.1 += 1;
        }
    }

    MevDistribution { buckets }
}

fn export_epoch_validator_csvs(comparison: &EpochComparison, output_dir: &Path) -> Result<()> {
    export_epoch_dropped_validators_csv(comparison, output_dir)?;
    export_epoch_added_validators_csv(comparison, output_dir)?;
    Ok(())
}

fn export_epoch_dropped_validators_csv(
    comparison: &EpochComparison,
    output_dir: &Path,
) -> Result<()> {
    let path = output_dir.join(format!("epoch_{}_validators_dropped.csv", comparison.epoch));
    let mut csv = String::new();

    csv.push_str("Validator Name,Vote Account,Production Rank,Proposed Rank,Production Score,Proposed Score,MEV Commission Score,Blacklisted Score,Superminority Score,Delinquency Score,Proposed Delinquency Score,Running Jito Score,Commission Score,Historical Commission Score,Vote Credits Ratio,Validator Age,MEV Commission %,Delinquent Epoch,Delinquent Epoch Ratio\n");

    for validator_with_rank in &comparison.top_400_churn.dropped_validators {
        let validator = &validator_with_rank.validator;
        csv.push_str(&format!(
            "\"{}\",{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.1},{},{}\n",
            format_validator_display(validator).replace("\"", "\"\""),
            validator.vote_account,
            validator_with_rank.production_rank.map_or("".to_string(), |r| r.to_string()),
            validator_with_rank.proposed_rank.map_or("N/A".to_string(), |r| r.to_string()),
            validator.production_score,
            validator.proposed_score,
            validator.mev_commission_score,
            validator.blacklisted_score,
            validator.superminority_score,
            validator.delinquency_score,
            validator.proposed_delinquency_score,
            validator.running_jito_score,
            validator.commission_score,
            validator.historical_commission_score,
            validator.vote_credits_ratio,
            validator.validator_age,
            get_mev_commission_percent(validator),
            validator.proposed_delinquency_epoch.map_or("N/A".to_string(), |e| e.to_string()),
            validator.proposed_delinquency_ratio.map_or("N/A".to_string(), |r| format!("{:.6}", r)),
        ));
    }

    fs::write(path, csv)?;
    Ok(())
}

fn export_epoch_added_validators_csv(
    comparison: &EpochComparison,
    output_dir: &Path,
) -> Result<()> {
    let path = output_dir.join(format!("epoch_{}_validators_added.csv", comparison.epoch));
    let mut csv = String::new();

    csv.push_str("Validator Name,Vote Account,Production Rank,Proposed Rank,Production Score,Proposed Score,MEV Commission Score,Blacklisted Score,Superminority Score,Delinquency Score,Proposed Delinquency Score,Running Jito Score,Commission Score,Historical Commission Score,Vote Credits Ratio,Validator Age,MEV Commission %,Delinquent Epoch,Delinquent Epoch Ratio\n");

    for validator_with_rank in &comparison.top_400_churn.added_validators {
        let validator = &validator_with_rank.validator;
        csv.push_str(&format!(
            "\"{}\",{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.1},{},{}\n",
            format_validator_display(validator).replace("\"", "\"\""),
            validator.vote_account,
            validator_with_rank.production_rank.map_or("N/A".to_string(), |r| r.to_string()),
            validator_with_rank.proposed_rank.map_or("".to_string(), |r| r.to_string()),
            validator.production_score,
            validator.proposed_score,
            validator.mev_commission_score,
            validator.blacklisted_score,
            validator.superminority_score,
            validator.delinquency_score,
            validator.proposed_delinquency_score,
            validator.running_jito_score,
            validator.commission_score,
            validator.historical_commission_score,
            validator.vote_credits_ratio,
            validator.validator_age,
            get_mev_commission_percent(validator),
            validator.proposed_delinquency_epoch.map_or("N/A".to_string(), |e| e.to_string()),
            validator.proposed_delinquency_ratio.map_or("N/A".to_string(), |r| format!("{:.6}", r)),
        ));
    }

    fs::write(path, csv)?;
    Ok(())
}
