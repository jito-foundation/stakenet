use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use stakenet_sdk::utils::accounts::{
    get_all_steward_accounts, get_directed_stake_meta, get_directed_stake_meta_address,
};

use crate::commands::command_args::ViewDirectedStakeDifferences;

pub async fn command_view_directed_stake_differences(
    args: ViewDirectedStakeDifferences,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.steward_config;
    let stake_meta_address = get_directed_stake_meta_address(&steward_config, &program_id);
    
    // Fetch all steward accounts to get validator list
    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;
    
    // Fetch directed stake meta
    let stake_meta =
        get_directed_stake_meta(client.clone(), &steward_config, &program_id).await?;

    // Build a map from vote_pubkey to validator_list_index for efficient lookup
    let vote_pubkey_to_validator_index: HashMap<Pubkey, usize> = 
        all_steward_accounts
            .validator_list_account
            .validators
            .iter()
            .enumerate()
            .map(|(idx, v)| (v.vote_account_address, idx))
            .collect();

    if args.print_json {
        let mut entries = Vec::new();
        for i in 0..stake_meta.total_stake_targets as usize {
            let target = &stake_meta.targets[i];
            if target.vote_pubkey == Pubkey::default() {
                continue;
            }

            // Find the validator list index for this target
            let validator_list_index = vote_pubkey_to_validator_index.get(&target.vote_pubkey).copied();

            let (directed_stake_lamports, difference) = if let Some(index) = validator_list_index {
                // Verify the mapping is correct using directed_stake_meta_indices
                let expected_target_index = stake_meta.directed_stake_meta_indices[index];
                if expected_target_index != u64::MAX && expected_target_index as usize == i {
                    let directed_lamports = stake_meta.directed_stake_lamports[index];
                    let difference = directed_lamports as i64 - target.total_staked_lamports as i64;
                    (directed_lamports, difference)
                } else {
                    // Mapping mismatch - validator list index doesn't point to this target
                    (0, -(target.total_staked_lamports as i64))
                }
            } else {
                // Validator not found in validator list
                (0, -(target.total_staked_lamports as i64))
            };

            entries.push(serde_json::json!({
                "target_index": i,
                "vote_pubkey": target.vote_pubkey.to_string(),
                "directed_stake_lamports": directed_stake_lamports,
                "target_total_staked_lamports": target.total_staked_lamports,
                "difference": difference,
                "validator_list_index": validator_list_index,
            }));
        }
        let output = serde_json::json!({
            "directed_stake_meta_address": stake_meta_address.to_string(),
            "total_stake_targets": stake_meta.total_stake_targets,
            "entries": entries,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Directed Stake Meta Address: {}", stake_meta_address);
        println!("Total Stake Targets: {}", stake_meta.total_stake_targets);
        println!("\nDirected Stake Differences (directed_stake_lamports - total_staked_lamports):");
        println!("{:<45} {:>25} {:>25} {:>15}", "Vote Pubkey", "Directed Stake", "Target Staked", "Difference");
        println!("{}", "-".repeat(112));

        for i in 0..stake_meta.total_stake_targets as usize {
            let target = &stake_meta.targets[i];
            if target.vote_pubkey == Pubkey::default() {
                continue;
            }

            // Find the validator list index for this target
            let validator_list_index = vote_pubkey_to_validator_index.get(&target.vote_pubkey).copied();

            let (directed_stake_lamports, difference) = if let Some(index) = validator_list_index {
                // Verify the mapping is correct using directed_stake_meta_indices
                let expected_target_index = stake_meta.directed_stake_meta_indices[index];
                if expected_target_index != u64::MAX && expected_target_index as usize == i {
                    let directed_lamports = stake_meta.directed_stake_lamports[index];
                    let difference = directed_lamports as i64 - target.total_staked_lamports as i64;
                    (directed_lamports, difference)
                } else {
                    // Mapping mismatch - validator list index doesn't point to this target
                    (0, -(target.total_staked_lamports as i64))
                }
            } else {
                // Validator not found in validator list
                (0, -(target.total_staked_lamports as i64))
            };

            if args.show_all || difference != 0 {
                println!(
                    "{:<45} {:>25} {:>25} {:>15}",
                    target.vote_pubkey,
                    directed_stake_lamports / 1_000_000_000,
                    target.total_staked_lamports / 1_000_000_000,
                    difference / 1_000_000_000
                );
            }
        }
    }

    Ok(())
}

