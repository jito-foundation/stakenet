use std::{collections::HashMap, sync::Arc};

use anchor_lang::AccountDeserialize;
use anyhow::{anyhow, Result};
use jito_steward::{
    constants::TVC_ACTIVATION_EPOCH,
    score::{instant_unstake_validator, validator_score, InstantUnstakeComponentsV3},
    Config,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{account::Account, pubkey::Pubkey};
use validator_history::{ClusterHistory, EpochCreditsRatio, ValidatorHistory};

use crate::commands::command_args::ViewScores;
use stakenet_sdk::utils::accounts::{
    get_all_steward_accounts, get_cluster_history_address, get_validator_history_address,
};

/// One validator's recomputed score and instant unstake decision.
struct Row {
    vote_account: Pubkey,
    score: u64,
    raw_score: u64,
    vote_credits_avg: u32,
    delinquency_score: u8,
    delinquency_ratio: f64,
    delinquency_epoch: u16,
    scorable_epochs: usize,
    window_epochs: usize,
    instant_unstake: InstantUnstakeComponentsV3,
}

/// Recomputes scores off-chain using the same functions the program runs.
///
/// Scoring only depends on account state, so this needs no on-chain execution and no waiting for a
/// scoring cycle. `--alpenglow-migration-epoch` overrides the configured value, which lets you see
/// how the same history scores on either side of the migration boundary.
pub async fn command_view_scores(
    args: ViewScores,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.view_parameters.steward_config;
    let steward_accounts = get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let mut config: Config = *steward_accounts.config_account;
    if let Some(migration_epoch) = args.alpenglow_migration_epoch {
        config.parameters.alpenglow_migration_epoch = migration_epoch;
    }

    let cluster_history: ClusterHistory = {
        let address = get_cluster_history_address(&validator_history::id());
        let account = client.get_account(&address).await?;
        ClusterHistory::try_deserialize(&mut account.data.as_slice())?
    };

    let epoch_schedule = client.get_epoch_schedule().await?;
    let epoch_info = client.get_epoch_info().await?;
    let current_epoch = match args.epoch {
        Some(epoch) => epoch,
        None => u16::try_from(epoch_info.epoch)?,
    };
    let slots_per_epoch = epoch_schedule.get_slots_in_epoch(u64::from(current_epoch));
    let epoch_start_slot = epoch_schedule.get_first_slot_in_epoch(u64::from(current_epoch));

    // Which validators to cover: one if asked for, otherwise the whole pool
    let vote_accounts: Vec<Pubkey> = match args.vote_account {
        Some(vote_account) => vec![vote_account],
        None => steward_accounts
            .validator_list_account
            .validators
            .iter()
            .map(|validator| validator.vote_account_address)
            .collect(),
    };

    let histories = fetch_validator_histories(client, &vote_accounts).await?;

    let epoch_credits_range = config.parameters.epoch_credits_range;
    let start_epoch = current_epoch
        .checked_sub(epoch_credits_range)
        .ok_or_else(|| anyhow!("epoch {current_epoch} is before the scoring window begins"))?;
    let end_epoch = current_epoch
        .checked_sub(1)
        .ok_or_else(|| anyhow!("epoch {current_epoch} has no previous epoch to score"))?;

    let mut rows = Vec::new();
    let mut skipped = Vec::new();

    for vote_account in &vote_accounts {
        let Some(validator_history) = histories.get(vote_account) else {
            skipped.push((*vote_account, "no validator history account".to_string()));
            continue;
        };

        let ratios = validator_history.history.epoch_credits_ratio_range(
            &cluster_history,
            start_epoch,
            end_epoch,
            TVC_ACTIVATION_EPOCH,
            slots_per_epoch,
            config.parameters.alpenglow_migration_epoch(),
        );
        let scorable_epochs = ratios
            .iter()
            .filter(|ratio| matches!(ratio, EpochCreditsRatio::Scored(_)))
            .count();

        let score = match validator_score(
            validator_history,
            &cluster_history,
            &config,
            current_epoch,
            TVC_ACTIVATION_EPOCH,
            slots_per_epoch,
        ) {
            Ok(score) => score,
            Err(e) => {
                skipped.push((*vote_account, format!("score failed: {e}")));
                continue;
            }
        };

        let instant_unstake = match instant_unstake_validator(
            validator_history,
            &cluster_history,
            &config,
            epoch_start_slot,
            current_epoch,
            TVC_ACTIVATION_EPOCH,
            slots_per_epoch,
        ) {
            Ok(components) => components,
            Err(e) => {
                skipped.push((*vote_account, format!("instant unstake failed: {e}")));
                continue;
            }
        };

        // A single validator gets the full per-epoch breakdown
        if args.vote_account.is_some() {
            print_epoch_window(&config, start_epoch, &ratios);
        }

        rows.push(Row {
            vote_account: *vote_account,
            score: score.score,
            raw_score: score.raw_score,
            vote_credits_avg: score.vote_credits_avg,
            delinquency_score: score.delinquency_score,
            delinquency_ratio: score.details.delinquency_ratio,
            delinquency_epoch: score.details.delinquency_epoch,
            scorable_epochs,
            window_epochs: ratios.len(),
            instant_unstake,
        });
    }

    rows.sort_by(|a, b| b.score.cmp(&a.score));

    if args.view_parameters.print_json {
        print_json(&config, current_epoch, start_epoch, end_epoch, &rows)?;
    } else {
        print_header(&config, current_epoch, start_epoch, end_epoch, rows.len());
        print_scores(&rows);
        print_instant_unstake(&rows);
        print_summary(&rows, &skipped);
    }

    Ok(())
}

async fn fetch_validator_histories(
    client: &Arc<RpcClient>,
    vote_accounts: &[Pubkey],
) -> Result<HashMap<Pubkey, ValidatorHistory>> {
    let addresses: Vec<Pubkey> = vote_accounts
        .iter()
        .map(|vote_account| get_validator_history_address(vote_account, &validator_history::id()))
        .collect();

    let mut accounts: Vec<Option<Account>> = Vec::with_capacity(addresses.len());
    for chunk in addresses.chunks(100) {
        accounts.extend(client.get_multiple_accounts(chunk).await?);
    }

    Ok(vote_accounts
        .iter()
        .zip(accounts)
        .filter_map(|(vote_account, account)| {
            let account = account?;
            let history = ValidatorHistory::try_deserialize(&mut account.data.as_slice()).ok()?;
            Some((*vote_account, history))
        })
        .collect())
}

fn era_of(config: &Config, epoch: u16) -> &'static str {
    if config.parameters.is_alpenglow_transition_epoch(epoch) {
        "migration"
    } else if config.parameters.is_alpenglow_epoch(epoch) {
        "alpenglow"
    } else {
        "tower"
    }
}

fn print_header(
    config: &Config,
    current_epoch: u16,
    start_epoch: u16,
    end_epoch: u16,
    validators: usize,
) {
    println!("------- Scoring inputs -------");
    println!("Current epoch:             {current_epoch}");
    println!("Scoring window:            {start_epoch}..={end_epoch}");
    println!(
        "Alpenglow migration epoch: {}",
        config.parameters.alpenglow_migration_epoch()
    );
    println!(
        "Scoring delinquency:       {}",
        config.parameters.scoring_delinquency_threshold_ratio
    );
    println!(
        "Instant unstake threshold: {}",
        config
            .parameters
            .instant_unstake_delinquency_threshold_ratio
    );
    println!("Validators:                {validators}");
    println!();
}

fn print_epoch_window(config: &Config, start_epoch: u16, ratios: &[EpochCreditsRatio]) {
    println!("------- Participation by epoch -------");
    println!("Epoch      Era         Participation");
    for (i, ratio) in ratios.iter().enumerate() {
        let epoch = start_epoch + i as u16;
        let era = era_of(config, epoch);
        match ratio {
            EpochCreditsRatio::Scored(ratio) => {
                let flag = if *ratio < config.parameters.scoring_delinquency_threshold_ratio {
                    "  <- below threshold"
                } else {
                    ""
                };
                println!("{epoch:<10} {era:<11} {ratio:.6}{flag}");
            }
            EpochCreditsRatio::Unscorable => println!("{epoch:<10} {era:<11} unscorable"),
        }
    }
    println!();
}

fn print_scores(rows: &[Row]) {
    println!("------- Scores -------");
    println!(
        "{:<45} {:>20} {:>12} {:>7} {:>10} {:>9}",
        "Vote account", "Score", "Credits avg", "Scored", "Delinquent", "Ratio"
    );
    for row in rows {
        let delinquent = if row.delinquency_score == 0 {
            format!("yes @{}", row.delinquency_epoch)
        } else {
            "no".to_string()
        };
        println!(
            "{:<45} {:>20} {:>12} {:>7} {:>10} {:>9.4}",
            row.vote_account.to_string(),
            row.score,
            row.vote_credits_avg,
            format!("{}/{}", row.scorable_epochs, row.window_epochs),
            delinquent,
            row.delinquency_ratio,
        );
    }
    println!();
}

fn print_instant_unstake(rows: &[Row]) {
    let flagged: Vec<&Row> = rows
        .iter()
        .filter(|row| row.instant_unstake.instant_unstake)
        .collect();

    println!("------- Instant unstake -------");
    if flagged.is_empty() {
        println!("No validators flagged.");
        println!();
        return;
    }

    println!(
        "{:<45} {:>6} {:>6} {:>5} {:>6} {:>8} {:>8}",
        "Vote account", "Delinq", "Commis", "MEV", "Blklst", "Merkle", "PFMerkle"
    );
    for row in flagged {
        let components = &row.instant_unstake;
        println!(
            "{:<45} {:>6} {:>6} {:>5} {:>6} {:>8} {:>8}",
            row.vote_account.to_string(),
            yes_no(components.delinquency_check),
            yes_no(components.commission_check),
            yes_no(components.mev_commission_check),
            yes_no(components.is_blacklisted),
            yes_no(components.is_bad_merkle_root_upload_authority),
            yes_no(components.is_bad_priority_fee_merkle_root_upload_authority),
        );
    }
    println!();
}

fn print_summary(rows: &[Row], skipped: &[(Pubkey, String)]) {
    let zero_scored = rows.iter().filter(|row| row.score == 0).count();
    let delinquent = rows.iter().filter(|row| row.delinquency_score == 0).count();
    let flagged = rows
        .iter()
        .filter(|row| row.instant_unstake.instant_unstake)
        .count();
    let delinquency_unstake = rows
        .iter()
        .filter(|row| row.instant_unstake.delinquency_check)
        .count();

    println!("------- Summary -------");
    println!("Scored:                    {}", rows.len());
    println!("Score 0 (filtered out):    {zero_scored}");
    println!("Delinquent in window:      {delinquent}");
    println!("Instant unstake flagged:   {flagged}");
    println!("  of which delinquency:    {delinquency_unstake}");

    if !skipped.is_empty() {
        println!();
        println!("Skipped {}:", skipped.len());
        for (vote_account, reason) in skipped {
            println!("  {vote_account}  {reason}");
        }
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "-"
    }
}

/// `ScoreComponentsV5` and `InstantUnstakeComponentsV3` are on-chain `#[event]` types and don't
/// implement `Serialize`, so the fields are listed out rather than adding serde to the program.
fn print_json(
    config: &Config,
    current_epoch: u16,
    start_epoch: u16,
    end_epoch: u16,
    rows: &[Row],
) -> Result<()> {
    let validators: Vec<_> = rows
        .iter()
        .map(|row| {
            let components = &row.instant_unstake;
            serde_json::json!({
                "vote_account": row.vote_account.to_string(),
                "score": row.score,
                "raw_score": row.raw_score,
                "vote_credits_avg": row.vote_credits_avg,
                "scorable_epochs": row.scorable_epochs,
                "window_epochs": row.window_epochs,
                "delinquency_score": row.delinquency_score,
                "delinquency_ratio": row.delinquency_ratio,
                "delinquency_epoch": row.delinquency_epoch,
                "instant_unstake": {
                    "instant_unstake": components.instant_unstake,
                    "delinquency_check": components.delinquency_check,
                    "commission_check": components.commission_check,
                    "mev_commission_check": components.mev_commission_check,
                    "is_blacklisted": components.is_blacklisted,
                    "is_bad_merkle_root_upload_authority":
                        components.is_bad_merkle_root_upload_authority,
                    "is_bad_priority_fee_merkle_root_upload_authority":
                        components.is_bad_priority_fee_merkle_root_upload_authority,
                },
            })
        })
        .collect();

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "current_epoch": current_epoch,
            "scoring_window": { "start": start_epoch, "end": end_epoch },
            "alpenglow_migration_epoch": config.parameters.alpenglow_migration_epoch(),
            "scoring_delinquency_threshold_ratio":
                config.parameters.scoring_delinquency_threshold_ratio,
            "instant_unstake_delinquency_threshold_ratio":
                config.parameters.instant_unstake_delinquency_threshold_ratio,
            "validators": validators,
        }))?
    );
    Ok(())
