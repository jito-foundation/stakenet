use std::sync::Arc;

use anchor_lang::AccountDeserialize;
use anyhow::Result;
use jito_steward::{
    constants::TVC_ACTIVATION_EPOCH, score::validator_score, Config as StewardConfig,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use validator_history::{ClusterHistory, ValidatorHistory};

use crate::commands::command_args::ViewParameters;
use stakenet_sdk::utils::accounts::{
    get_all_validator_history_accounts, get_cluster_history_address, get_steward_config_account,
};

#[derive(clap::Parser)]
#[command(about = "Analyze validator scores off-chain with overridden priority fee params")]
pub struct AnalyzeScores {
    #[command(flatten)]
    pub view_parameters: ViewParameters,
}

pub async fn command_analyze_scores(
    args: AnalyzeScores,
    client: &Arc<RpcClient>,
    _steward_program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.view_parameters.steward_config;

    // Fetch config
    let mut config: Box<StewardConfig> =
        get_steward_config_account(client, &steward_config).await?;

    // Override local config parameters per request (does not persist on-chain)
    config.parameters.priority_fee_scoring_start_epoch = u16::MAX;
    config.parameters.priority_fee_max_commission_bps = 5000;
    config.parameters.priority_fee_lookback_epochs = 10;
    config.parameters.priority_fee_lookback_offset = 2;

    // Fetch ClusterHistory
    let vh_program_id = validator_history::id();
    let cluster_history_address = get_cluster_history_address(&vh_program_id);
    let cluster_account = client.get_account(&cluster_history_address).await?;
    let mut cluster_account_data = cluster_account.data.as_slice();
    let cluster_history: ClusterHistory =
        ClusterHistory::try_deserialize(&mut cluster_account_data)?;

    // Fetch all ValidatorHistory accounts
    let validator_histories: Vec<ValidatorHistory> =
        get_all_validator_history_accounts(client, vh_program_id).await?;

    // Current epoch
    let epoch_info = client.get_epoch_info().await?;
    let current_epoch_u16: u16 = validator_history::utils::cast_epoch(epoch_info.epoch)?;

    // Compute scores
    let mut results: Vec<(Pubkey, f64, f64, f64, f64)> =
        Vec::with_capacity(validator_histories.len());
    let mut skipped: usize = 0;
    for vh in validator_histories.iter() {
        match validator_score(
            vh,
            &cluster_history,
            &config,
            current_epoch_u16,
            TVC_ACTIVATION_EPOCH,
        ) {
            Ok(score) => {
                results.push((
                    vh.vote_account,
                    score.score,
                    score.priority_fee_commission_score,
                    score.priority_fee_merkle_root_upload_authority_score,
                    score.merkle_root_upload_authority_score,
                ));
            }
            Err(_) => skipped += 1,
        }
    }

    // Aggregate stats
    let total = results.len();
    let non_zero_scores = results.iter().filter(|(_, s, _, _, _)| *s > 0.0).count();

    let (mut pf_commission_ones, mut pf_commission_zeros) = (0usize, 0usize);
    let (mut pf_merkle_ones, mut pf_merkle_zeros) = (0usize, 0usize);
    let (mut merkle_ones, mut merkle_zeros) = (0usize, 0usize);

    let mut pf_commission_zero_validators: Vec<Pubkey> = Vec::new();
    let mut pf_merkle_zero_validators: Vec<Pubkey> = Vec::new();
    let mut merkle_zero_validators: Vec<Pubkey> = Vec::new();

    for (vote, _, pf_commission, pf_merkle, merkle) in results.iter() {
        if (*pf_commission - 1.0).abs() < f64::EPSILON {
            pf_commission_ones += 1;
        } else if (*pf_commission - 0.0).abs() < f64::EPSILON {
            pf_commission_zeros += 1;
            pf_commission_zero_validators.push(*vote);
        }

        if (*pf_merkle - 1.0).abs() < f64::EPSILON {
            pf_merkle_ones += 1;
        } else if (*pf_merkle - 0.0).abs() < f64::EPSILON {
            pf_merkle_zeros += 1;
            pf_merkle_zero_validators.push(*vote);
        }

        if (*merkle - 1.0).abs() < f64::EPSILON {
            merkle_ones += 1;
        } else if (*merkle - 0.0).abs() < f64::EPSILON {
            merkle_zeros += 1;
            merkle_zero_validators.push(*vote);
        }
    }

    println!("----- Validator Score Analysis -----");
    println!("Current epoch: {}", current_epoch_u16);
    println!("Total validators processed: {}", total);
    println!("Skipped due to missing data/errors: {}", skipped);
    println!("Non-zero final scores: {}", non_zero_scores);
    println!("\nComponent 1.0 / 0.0 counts:");
    println!(
        "priority_fee_commission_score -> 1.0: {}, 0.0: {}",
        pf_commission_ones, pf_commission_zeros
    );
    if !pf_commission_zero_validators.is_empty() {
        let list = pf_commission_zero_validators
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!("priority_fee_commission_score 0.0 validators: {}", list);
    }
    println!(
        "priority_fee_merkle_root_upload_authority_score -> 1.0: {}, 0.0: {}",
        pf_merkle_ones, pf_merkle_zeros
    );
    if !pf_merkle_zero_validators.is_empty() {
        let list = pf_merkle_zero_validators
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "priority_fee_merkle_root_upload_authority_score 0.0 validators: {}",
            list
        );
    }
    println!(
        "merkle_root_upload_authority_score -> 1.0: {}, 0.0: {}",
        merkle_ones, merkle_zeros
    );
    if !merkle_zero_validators.is_empty() {
        let list = merkle_zero_validators
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "merkle_root_upload_authority_score 0.0 validators: {}",
            list
        );
    }

    Ok(())
}
