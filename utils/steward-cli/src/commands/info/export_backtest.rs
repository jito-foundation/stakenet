use crate::commands::command_args::ExportBacktest;
use crate::commands::info::view_backtest::ValidatorMetadata;
use anyhow::Result;
use serde::{Deserialize, Deserializer};
use solana_sdk::pubkey::Pubkey;
use std::collections::{BTreeMap, HashMap, HashSet};
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
    pub validator_index: usize,
    #[serde(default)]
    pub score: f64,
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
    #[serde(default)]
    pub mev_ranking_score: f64,
    #[serde(default)]
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

#[derive(Debug)]
struct EpochComparison {
    epoch: u64,
    top_400_churn: ChurnAnalysis,
    validator_counts: ValidatorCounts,
    vote_credit_stats: VoteCreditStats,
    mev_distribution: MevDistribution,
}

#[derive(Debug)]
struct ChurnAnalysis {
    stayed_in_top_400: usize,
    dropped_from_top_400: usize,
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
struct ValidatorCounts {
    production_total: usize,
    proposed_total: usize,
    production_meeting_threshold: usize,
    proposed_meeting_threshold: usize,
}

#[derive(Debug)]
struct VoteCreditStats {
    production_deciles: Vec<f64>,
    proposed_deciles: Vec<f64>,
}

#[derive(Debug)]
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
    (1.0 - validator.mev_ranking_score) * 100.0
}

fn get_change_reason(validator: &ValidatorScoreResultJson, is_dropped: bool) -> String {
    let mev_commission = get_mev_commission_percent(validator);
    
    if is_dropped {
        if mev_commission > 0.1 {
            format!("MEV Commission Too High ({:.1}%)", mev_commission)
        } else if validator.vote_credits_ratio < 0.99 {
            "Performance Below Threshold".to_string()
        } else if validator.yield_score < 0.95 {
            "Low Yield Score".to_string()
        } else {
            "Strategy Change".to_string()
        }
    } else {
        if mev_commission < 0.1 {
            "0% MEV Commission".to_string()
        } else if validator.vote_credits_ratio >= 0.99 {
            "High Performance".to_string()
        } else {
            "Strategy Preference".to_string()
        }
    }
}

pub async fn command_export_backtest(args: ExportBacktest) -> Result<()> {
    println!("📦 Exporting validator added/dropped lists for each epoch...\n");

    // Create output directory
    fs::create_dir_all(&args.output_dir)?;

    // Load both result files
    let file1_data: Vec<BacktestResultJson> = load_backtest_file(&args.file1)?;
    let file2_data: Vec<BacktestResultJson> = load_backtest_file(&args.file2)?;

    println!("📁 Loaded {} epochs from Production strategy: {:?}", file1_data.len(), args.file1);
    println!("📁 Loaded {} epochs from Proposed strategy: {:?}", file2_data.len(), args.file2);

    if file1_data.len() != file2_data.len() {
        return Err(anyhow::anyhow!("Files have different number of epochs"));
    }

    // Analyze and export each epoch separately
    for (epoch1, epoch2) in file1_data.iter().zip(file2_data.iter()) {
        if epoch1.epoch != epoch2.epoch {
            return Err(anyhow::anyhow!(
                "Epoch mismatch: {} vs {}",
                epoch1.epoch,
                epoch2.epoch
            ));
        }
        let comparison = analyze_epoch(epoch1, epoch2)?;
        export_epoch_validator_csvs(&comparison, &args.output_dir)?;
    }

    println!("\n✅ Export completed successfully!");
    println!("📂 Output files saved to: {}", args.output_dir.display());
    println!("📋 Files created:");
    
    for (epoch1, _) in file1_data.iter().zip(file2_data.iter()) {
        println!("   • epoch_{}_validators_dropped.csv", epoch1.epoch);
        println!("   • epoch_{}_validators_added.csv", epoch1.epoch);
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
    // Create validator maps for quick lookups
    let file1_validators: HashMap<String, (usize, &ValidatorScoreResultJson)> = epoch1
        .validator_scores
        .iter()
        .enumerate()
        .map(|(i, v)| (v.vote_account.to_string(), (i, v)))
        .collect();

    let file2_validators: HashMap<String, (usize, &ValidatorScoreResultJson)> = epoch2
        .validator_scores
        .iter()
        .enumerate()
        .map(|(i, v)| (v.vote_account.to_string(), (i, v)))
        .collect();

    // Get top 400 from each strategy
    let file1_top_400: Vec<_> = epoch1.validator_scores.iter().take(400).collect();
    let file2_top_400: Vec<_> = epoch2.validator_scores.iter().take(400).collect();

    let file1_top_400_keys: HashSet<_> = file1_top_400
        .iter()
        .map(|v| v.vote_account.to_string())
        .collect();

    let file2_top_400_keys: HashSet<_> = file2_top_400
        .iter()
        .map(|v| v.vote_account.to_string())
        .collect();

    // Calculate churn
    let stayed_in_top_400 = file1_top_400_keys.intersection(&file2_top_400_keys).count();
    let dropped_from_top_400 = file1_top_400_keys.difference(&file2_top_400_keys).count();
    let added_to_top_400 = file2_top_400_keys.difference(&file1_top_400_keys).count();

    // Get dropped validators with ranks
    let dropped_validators: Vec<ValidatorWithRank> = file1_top_400_keys
        .difference(&file2_top_400_keys)
        .filter_map(|key| {
            let (prod_rank, validator) = file1_validators.get(key)?;
            let proposed_rank = file2_validators.get(key).map(|(rank, _)| *rank);
            Some(ValidatorWithRank {
                validator: (*validator).clone(),
                production_rank: Some(*prod_rank),
                proposed_rank,
            })
        })
        .collect();

    // Get added validators with ranks
    let added_validators: Vec<ValidatorWithRank> = file2_top_400_keys
        .difference(&file1_top_400_keys)
        .filter_map(|key| {
            let (prop_rank, validator) = file2_validators.get(key)?;
            let production_rank = file1_validators.get(key).map(|(rank, _)| *rank);
            Some(ValidatorWithRank {
                validator: (*validator).clone(),
                production_rank,
                proposed_rank: Some(*prop_rank),
            })
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
    let production_meeting_threshold = epoch1
        .validator_scores
        .iter()
        .filter(|v| v.vote_credits_ratio >= 0.99)
        .count();

    let proposed_meeting_threshold = epoch2
        .validator_scores
        .iter()
        .filter(|v| v.vote_credits_ratio >= 0.99)
        .count();

    let validator_counts = ValidatorCounts {
        production_total: epoch1.validator_scores.len(),
        proposed_total: epoch2.validator_scores.len(),
        production_meeting_threshold,
        proposed_meeting_threshold,
    };

    // Calculate vote credit statistics
    let file1_vote_ratios: Vec<f64> = file1_top_400.iter().map(|v| v.vote_credits_ratio).collect();
    let file2_vote_ratios: Vec<f64> = file2_top_400.iter().map(|v| v.vote_credits_ratio).collect();

    let vote_credit_stats = VoteCreditStats {
        production_deciles: calculate_deciles(&file1_vote_ratios),
        proposed_deciles: calculate_deciles(&file2_vote_ratios),
    };

    // Calculate MEV distribution
    let mev_distribution = calculate_mev_distribution(epoch2, &file2_top_400_keys);

    Ok(EpochComparison {
        epoch: epoch1.epoch,
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

fn calculate_mev_distribution(epoch: &BacktestResultJson, top_400_keys: &HashSet<String>) -> MevDistribution {
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

fn export_epoch_dropped_validators_csv(comparison: &EpochComparison, output_dir: &Path) -> Result<()> {
    let path = output_dir.join(format!("epoch_{}_validators_dropped.csv", comparison.epoch));
    let mut csv = String::new();
    
    csv.push_str("Validator Name,Vote Account,Production Rank,Proposed Rank,Score,Yield Score,MEV Commission Score,Blacklisted Score,Superminority Score,Delinquency Score,Running Jito Score,Commission Score,Historical Commission Score,Vote Credits Ratio,MEV Ranking Score,Validator Age,Score For Backtest Comparison,MEV Commission %,Change Reason\n");
    
    for validator_with_rank in &comparison.top_400_churn.dropped_validators {
        let validator = &validator_with_rank.validator;
        csv.push_str(&format!(
            "\"{}\",{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.1},\"{}\"\n",
            format_validator_display(validator).replace("\"", "\"\""),
            validator.vote_account,
            validator_with_rank.production_rank.map_or("".to_string(), |r| (r + 1).to_string()),
            validator_with_rank.proposed_rank.map_or("".to_string(), |r| (r + 1).to_string()),
            validator.score,
            validator.yield_score,
            validator.mev_commission_score,
            validator.blacklisted_score,
            validator.superminority_score,
            validator.delinquency_score,
            validator.running_jito_score,
            validator.commission_score,
            validator.historical_commission_score,
            validator.vote_credits_ratio,
            validator.mev_ranking_score,
            validator.validator_age,
            validator.score_for_backtest_comparison,
            get_mev_commission_percent(validator),
            get_change_reason(validator, true),
        ));
    }
    
    fs::write(path, csv)?;
    Ok(())
}

fn export_epoch_added_validators_csv(comparison: &EpochComparison, output_dir: &Path) -> Result<()> {
    let path = output_dir.join(format!("epoch_{}_validators_added.csv", comparison.epoch));
    let mut csv = String::new();
    
    csv.push_str("Validator Name,Vote Account,Production Rank,Proposed Rank,Score,Yield Score,MEV Commission Score,Blacklisted Score,Superminority Score,Delinquency Score,Running Jito Score,Commission Score,Historical Commission Score,Vote Credits Ratio,MEV Ranking Score,Validator Age,Score For Backtest Comparison,MEV Commission %,Change Reason\n");
    
    for validator_with_rank in &comparison.top_400_churn.added_validators {
        let validator = &validator_with_rank.validator;
        csv.push_str(&format!(
            "\"{}\",{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:.1},\"{}\"\n",
            format_validator_display(validator).replace("\"", "\"\""),
            validator.vote_account,
            validator_with_rank.production_rank.map_or("".to_string(), |r| (r + 1).to_string()),
            validator_with_rank.proposed_rank.map_or("".to_string(), |r| (r + 1).to_string()),
            validator.score,
            validator.yield_score,
            validator.mev_commission_score,
            validator.blacklisted_score,
            validator.superminority_score,
            validator.delinquency_score,
            validator.running_jito_score,
            validator.commission_score,
            validator.historical_commission_score,
            validator.vote_credits_ratio,
            validator.mev_ranking_score,
            validator.validator_age,
            validator.score_for_backtest_comparison,
            get_mev_commission_percent(validator),
            get_change_reason(validator, false),
        ));
    }
    
    fs::write(path, csv)?;
    Ok(())
}

