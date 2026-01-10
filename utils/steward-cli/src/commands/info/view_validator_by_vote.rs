use std::sync::Arc;

use anyhow::{anyhow, Result};
use jito_steward::constants::LAMPORT_BALANCE_DEFAULT;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use stakenet_sdk::utils::accounts::{get_all_steward_accounts, get_directed_stake_meta};

use crate::commands::command_args::ViewValidatorByVote;

pub async fn command_view_validator_by_vote(
    args: ViewValidatorByVote,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.steward_config;
    let vote_pubkey = args.vote_pubkey;

    // Fetch all steward accounts
    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;

    // Find validator list index by vote pubkey
    let validator_list_index = all_steward_accounts
        .validator_list_account
        .validators
        .iter()
        .position(|v| v.vote_account_address == vote_pubkey)
        .ok_or_else(|| anyhow!("Validator with vote pubkey {} not found in validator list", vote_pubkey))?;

    // Get validator lamport balance from state
    let validator_lamport_balance = all_steward_accounts
        .state_account
        .state
        .validator_lamport_balances
        .get(validator_list_index)
        .copied()
        .unwrap_or(LAMPORT_BALANCE_DEFAULT);

    // Fetch directed stake meta
    let directed_stake_meta = get_directed_stake_meta(client.clone(), &steward_config, &program_id)
        .await
        .ok();

    if args.print_json {
        let mut output = serde_json::json!({
            "vote_pubkey": vote_pubkey.to_string(),
            "validator_list_index": validator_list_index,
            "validator_lamport_balance": validator_lamport_balance,
            "directed_stake_meta_entry": serde_json::Value::Null,
        });

        if let Some(ref meta) = directed_stake_meta {
            let meta_index = meta.directed_stake_meta_indices[validator_list_index];
            
            if meta_index != u64::MAX {
                let target = &meta.targets[meta_index as usize];
                output["directed_stake_meta_entry"] = serde_json::json!({
                    "meta_index": meta_index,
                    "vote_pubkey": target.vote_pubkey.to_string(),
                    "total_target_lamports": target.total_target_lamports,
                    "total_staked_lamports": target.total_staked_lamports,
                    "target_last_updated_epoch": target.target_last_updated_epoch,
                    "staked_last_updated_epoch": target.staked_last_updated_epoch,
                    "directed_stake_lamports": meta.directed_stake_lamports[validator_list_index],
                });
            }
        }

        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Vote Pubkey: {}", vote_pubkey);
        println!("Validator List Index: {}", validator_list_index);
        println!("Validator Lamport Balance: {}", validator_lamport_balance);

        if let Some(ref meta) = directed_stake_meta {
            let meta_index = meta.directed_stake_meta_indices[validator_list_index];
            
            if meta_index != u64::MAX {
                let target = &meta.targets[meta_index as usize];
                println!("\nDirected Stake Meta Entry:");
                println!("  Meta Index: {}", meta_index);
                println!("  Vote Pubkey: {}", target.vote_pubkey);
                println!("  Total Target Lamports: {}", target.total_target_lamports);
                println!("  Total Staked Lamports: {}", target.total_staked_lamports);
                println!("  Target Last Updated Epoch: {}", target.target_last_updated_epoch);
                println!("  Staked Last Updated Epoch: {}", target.staked_last_updated_epoch);
                println!("  Directed Stake Lamports: {}", meta.directed_stake_lamports[validator_list_index]);
            } else {
                println!("\nDirected Stake Meta Entry: None (validator not in directed stake targets)");
            }
        } else {
            println!("\nDirected Stake Meta Entry: N/A (DirectedStakeMeta account not found)");
        }
    }

    Ok(())
}

