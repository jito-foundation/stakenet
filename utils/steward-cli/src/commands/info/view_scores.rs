use std::sync::Arc;

use anchor_lang::AccountDeserialize;
use anyhow::{anyhow, Result};
use jito_steward::{
    constants::TVC_ACTIVATION_EPOCH,
    score::{validator_score, ScoreComponentsV5},
    Config,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use validator_history::{ClusterHistory, EpochCreditsRatio, ValidatorHistory};

use crate::commands::command_args::ViewScores;
use stakenet_sdk::utils::accounts::{
    get_all_steward_accounts, get_cluster_history_address, get_validator_history_address,
};

/// Recomputes a validator's score off-chain using the same functions the program runs.
///
/// The score only depends on account state, so this needs no on-chain execution and no waiting for
/// a scoring cycle. `--alpenglow-migration-epoch` overrides the configured value, which lets you
/// see how the same history scores on either side of the migration boundary.
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

    let cluster_history_address = get_cluster_history_address(&validator_history::id());
    let cluster_history: ClusterHistory = {
        let account = client.get_account(&cluster_history_address).await?;
        ClusterHistory::try_deserialize(&mut account.data.as_slice())?
    };

    let validator_history_address =
        get_validator_history_address(&args.vote_account, &validator_history::id());
    let validator_history: ValidatorHistory = {
        let account = client
            .get_account(&validator_history_address)
            .await
            .map_err(|e| {
                anyhow!(
                    "no validator history for {}: {e}. Has the keeper copied this validator?",
                    args.vote_account
                )
            })?;
        ValidatorHistory::try_deserialize(&mut account.data.as_slice())?
    };

    let epoch_schedule = client.get_epoch_schedule().await?;
    let current_epoch = match args.epoch {
        Some(epoch) => epoch,
        None => u16::try_from(client.get_epoch_info().await?.epoch)?,
    };
    let slots_per_epoch = epoch_schedule.get_slots_in_epoch(u64::from(current_epoch));

    // The window scoring reads: [current - epoch_credits_range, current - 1]
    let epoch_credits_range = config.parameters.epoch_credits_range;
    let start_epoch = current_epoch
        .checked_sub(epoch_credits_range)
        .ok_or_else(|| anyhow!("epoch {current_epoch} is before the scoring window begins"))?;
    let end_epoch = current_epoch
        .checked_sub(1)
        .ok_or_else(|| anyhow!("epoch {current_epoch} has no previous epoch to score"))?;

    let ratios = validator_history.history.epoch_credits_ratio_range(
        &cluster_history,
        start_epoch,
        end_epoch,
        TVC_ACTIVATION_EPOCH,
        slots_per_epoch,
        config.parameters.alpenglow_migration_epoch(),
    );

    let score = validator_score(
        &validator_history,
        &cluster_history,
        &config,
        current_epoch,
        TVC_ACTIVATION_EPOCH,
        slots_per_epoch,
    )?;

    if args.view_parameters.print_json {
        print_json(
            &args.vote_account,
            current_epoch,
            &config,
            start_epoch,
            &ratios,
            &score,
        )?;
    } else {
        print_human(
            &args.vote_account,
            current_epoch,
            &config,
            start_epoch,
            &ratios,
            &score,
        );
    }

    Ok(())
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

fn print_human(
    vote_account: &Pubkey,
    current_epoch: u16,
    config: &Config,
    start_epoch: u16,
    ratios: &[EpochCreditsRatio],
    score: &ScoreComponentsV5,
) {
    println!("------- Score: {vote_account} -------");
    println!("Current epoch:            {current_epoch}");
    println!(
        "Alpenglow migration epoch: {}",
        config.parameters.alpenglow_migration_epoch()
    );
    println!(
        "Epoch credits range:      {}",
        config.parameters.epoch_credits_range
    );
    println!(
        "Delinquency threshold:    {}",
        config.parameters.scoring_delinquency_threshold_ratio
    );
    println!();

    println!("Epoch      Era         Participation");
    let mut scored = 0usize;
    for (i, ratio) in ratios.iter().enumerate() {
        let epoch = start_epoch + i as u16;
        let era = era_of(config, epoch);
        match ratio {
            EpochCreditsRatio::Scored(ratio) => {
                scored += 1;
                let flag = if *ratio < config.parameters.scoring_delinquency_threshold_ratio {
                    "  <- below threshold"
                } else {
                    ""
                };
                println!("{epoch:<10} {era:<11} {ratio:.6}{flag}");
            }
            EpochCreditsRatio::Unscorable => {
                println!("{epoch:<10} {era:<11} unscorable");
            }
        }
    }
    println!();
    println!("Scorable epochs:          {scored} of {}", ratios.len());
    println!();

    println!("------- Components -------");
    println!("score:                    {}", score.score);
    println!("raw_score:                {}", score.raw_score);
    println!("vote_credits_avg:         {}", score.vote_credits_avg);
    println!("commission_max:           {}", score.commission_max);
    println!("mev_commission_avg:       {}", score.mev_commission_avg);
    println!("validator_age:            {}", score.validator_age);
    println!();
    println!("------- Binary filters (0 zeroes the score) -------");
    println!("delinquency_score:        {}", score.delinquency_score);
    println!("mev_commission_score:     {}", score.mev_commission_score);
    println!("commission_score:         {}", score.commission_score);
    println!(
        "historical_commission:    {}",
        score.historical_commission_score
    );
    println!("blacklisted_score:        {}", score.blacklisted_score);
    println!("superminority_score:      {}", score.superminority_score);
    println!("running_bam_score:        {}", score.running_bam_score);
    println!(
        "merkle_root_authority:    {}",
        score.merkle_root_upload_authority_score
    );
    println!(
        "priority_fee_commission:  {}",
        score.priority_fee_commission_score
    );
    println!();
    println!("------- Details -------");
    println!(
        "delinquency_ratio:        {}",
        score.details.delinquency_ratio
    );
    println!(
        "delinquency_epoch:        {}",
        score.details.delinquency_epoch
    );
    println!("max_commission:           {}", score.details.max_commission);
    println!(
        "max_mev_commission:       {}",
        score.details.max_mev_commission
    );
}

fn print_json(
    vote_account: &Pubkey,
    current_epoch: u16,
    config: &Config,
    start_epoch: u16,
    ratios: &[EpochCreditsRatio],
    score: &ScoreComponentsV5,
) -> Result<()> {
    let epochs: Vec<_> = ratios
        .iter()
        .enumerate()
        .map(|(i, ratio)| {
            let epoch = start_epoch + i as u16;
            serde_json::json!({
                "epoch": epoch,
                "era": era_of(config, epoch),
                "participation": match ratio {
                    EpochCreditsRatio::Scored(ratio) => Some(*ratio),
                    EpochCreditsRatio::Unscorable => None,
                },
            })
        })
        .collect();

    // `ScoreComponentsV5` is an on-chain `#[event]` type and doesn't implement `Serialize`, so the
    // fields are listed out rather than adding a serde dependency to the program
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "vote_account": vote_account.to_string(),
            "current_epoch": current_epoch,
            "alpenglow_migration_epoch": config.parameters.alpenglow_migration_epoch(),
            "epoch_credits_range": config.parameters.epoch_credits_range,
            "scoring_delinquency_threshold_ratio":
                config.parameters.scoring_delinquency_threshold_ratio,
            "epochs": epochs,
            "score": {
                "score": score.score,
                "raw_score": score.raw_score,
                "commission_max": score.commission_max,
                "mev_commission_avg": score.mev_commission_avg,
                "validator_age": score.validator_age,
                "vote_credits_avg": score.vote_credits_avg,
                "filters": {
                    "delinquency_score": score.delinquency_score,
                    "mev_commission_score": score.mev_commission_score,
                    "commission_score": score.commission_score,
                    "historical_commission_score": score.historical_commission_score,
                    "blacklisted_score": score.blacklisted_score,
                    "superminority_score": score.superminority_score,
                    "running_bam_score": score.running_bam_score,
                    "merkle_root_upload_authority_score":
                        score.merkle_root_upload_authority_score,
                    "priority_fee_commission_score": score.priority_fee_commission_score,
                    "priority_fee_merkle_root_upload_authority_score":
                        score.priority_fee_merkle_root_upload_authority_score,
                },
                "details": {
                    "delinquency_ratio": score.details.delinquency_ratio,
                    "delinquency_epoch": score.details.delinquency_epoch,
                    "max_commission": score.details.max_commission,
                    "max_commission_epoch": score.details.max_commission_epoch,
                    "max_mev_commission": score.details.max_mev_commission,
                    "max_mev_commission_epoch": score.details.max_mev_commission_epoch,
                    "max_historical_commission": score.details.max_historical_commission,
                    "max_historical_commission_epoch":
                        score.details.max_historical_commission_epoch,
                    "superminority_epoch": score.details.superminority_epoch,
                    "avg_priority_fee_commission": score.details.avg_priority_fee_commission,
                    "max_priority_fee_commission_epoch":
                        score.details.max_priority_fee_commission_epoch,
                },
            },
        }))?
    );
    Ok(())
}
