use std::sync::Arc;

use anchor_lang::AccountDeserialize;
use anyhow::Result;
use jito_steward::{
    constants::TVC_ACTIVATION_EPOCH, score::instant_unstake_validator, Config as StewardConfig,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use validator_history::{ClusterHistory, ValidatorHistory};

use crate::commands::command_args::ViewParameters;
use stakenet_sdk::utils::accounts::{
    get_all_validator_history_accounts, get_cluster_history_address, get_steward_config_account,
};

#[derive(clap::Parser)]
#[command(about = "Analyze instant-unstake checks off-chain for all validators")]
pub struct AnalyzeInstantUnstake {
    #[command(flatten)]
    pub view_parameters: ViewParameters,
}

pub async fn command_analyze_instant_unstake(
    args: AnalyzeInstantUnstake,
    client: &Arc<RpcClient>,
    _steward_program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.view_parameters.steward_config;

    // Fetch config
    let config: Box<StewardConfig> = get_steward_config_account(client, &steward_config).await?;

    // No parameter overrides required for instant-unstake analysis

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

    // Current epoch and epoch start slot
    let epoch_info = client.get_epoch_info().await?;
    let epoch_schedule = client.get_epoch_schedule().await?;
    let current_epoch_u16: u16 = validator_history::utils::cast_epoch(epoch_info.epoch)?;
    let epoch_start_slot: u64 = epoch_schedule.get_first_slot_in_epoch(epoch_info.epoch);

    // Compute instant-unstake checks
    let mut results: Vec<(
        Pubkey,
        bool, // instant_unstake
        bool, // delinquency_check
        bool, // commission_check
        bool, // mev_commission_check
        bool, // is_blacklisted
        bool, // is_bad_merkle_root_upload_authority
        bool, // is_bad_priority_fee_merkle_root_upload_authority
    )> = Vec::with_capacity(validator_histories.len());
    let mut skipped: usize = 0;
    for vh in validator_histories.iter() {
        match instant_unstake_validator(
            vh,
            &cluster_history,
            &config,
            epoch_start_slot,
            current_epoch_u16,
            TVC_ACTIVATION_EPOCH,
        ) {
            Ok(unstake) => results.push((
                vh.vote_account,
                unstake.instant_unstake,
                unstake.delinquency_check,
                unstake.commission_check,
                unstake.mev_commission_check,
                unstake.is_blacklisted,
                unstake.is_bad_merkle_root_upload_authority,
                unstake.is_bad_priority_fee_merkle_root_upload_authority,
            )),
            Err(_) => skipped += 1,
        }
    }

    // Aggregate stats
    let total = results.len();
    let mut total_instant_unstake = 0usize;
    let mut delinquency_true = 0usize;
    let mut commission_true = 0usize;
    let mut mev_commission_true = 0usize;
    let mut blacklisted_true = 0usize;
    let mut bad_merkle_true = 0usize;
    let mut bad_pf_merkle_true = 0usize;

    let mut instant_unstake_validators: Vec<Pubkey> = Vec::new();
    let mut delinquency_validators: Vec<Pubkey> = Vec::new();
    let mut commission_validators: Vec<Pubkey> = Vec::new();
    let mut mev_commission_validators: Vec<Pubkey> = Vec::new();
    let mut blacklisted_validators: Vec<Pubkey> = Vec::new();
    let mut bad_merkle_validators: Vec<Pubkey> = Vec::new();
    let mut bad_pf_merkle_validators: Vec<Pubkey> = Vec::new();

    for (vote, iu, d, c, m, b, bad_m, bad_pm) in results.iter() {
        if *iu {
            total_instant_unstake += 1;
            instant_unstake_validators.push(*vote);
        }
        if *d {
            delinquency_true += 1;
            delinquency_validators.push(*vote);
        }
        if *c {
            commission_true += 1;
            commission_validators.push(*vote);
        }
        if *m {
            mev_commission_true += 1;
            mev_commission_validators.push(*vote);
        }
        if *b {
            blacklisted_true += 1;
            blacklisted_validators.push(*vote);
        }
        if *bad_m {
            bad_merkle_true += 1;
            bad_merkle_validators.push(*vote);
        }
        if *bad_pm {
            bad_pf_merkle_true += 1;
            bad_pf_merkle_validators.push(*vote);
        }
    }

    println!("----- Instant Unstake Analysis -----");
    println!("Current epoch: {}", current_epoch_u16);
    println!("Total validators processed: {}", total);
    println!("Skipped due to missing data/errors: {}", skipped);
    println!("Instant-unstake flagged: {}", total_instant_unstake);
    println!("\nChecks (true counts):");
    println!("delinquency_check: {}", delinquency_true);
    println!("commission_check: {}", commission_true);
    println!("mev_commission_check: {}", mev_commission_true);
    println!("is_blacklisted: {}", blacklisted_true);
    println!("is_bad_merkle_root_upload_authority: {}", bad_merkle_true);
    println!(
        "is_bad_priority_fee_merkle_root_upload_authority: {}",
        bad_pf_merkle_true
    );

    if !instant_unstake_validators.is_empty() {
        let list = instant_unstake_validators
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!("instant_unstake validators: {}", list);
    }
    if !delinquency_validators.is_empty() {
        let list = delinquency_validators
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!("delinquency_check validators: {}", list);
    }
    if !commission_validators.is_empty() {
        let list = commission_validators
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!("commission_check validators: {}", list);
    }
    if !mev_commission_validators.is_empty() {
        let list = mev_commission_validators
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!("mev_commission_check validators: {}", list);
    }
    if !blacklisted_validators.is_empty() {
        let list = blacklisted_validators
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!("is_blacklisted validators: {}", list);
    }
    if !bad_merkle_validators.is_empty() {
        let list = bad_merkle_validators
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!("is_bad_merkle_root_upload_authority validators: {}", list);
    }
    if !bad_pf_merkle_validators.is_empty() {
        let list = bad_pf_merkle_validators
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "is_bad_priority_fee_merkle_root_upload_authority validators: {}",
            list
        );
    }

    Ok(())
}
