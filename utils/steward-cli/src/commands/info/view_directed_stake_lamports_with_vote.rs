use std::sync::Arc;

use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use stakenet_sdk::utils::accounts::{get_directed_stake_meta, get_directed_stake_meta_address};

use crate::commands::command_args::ViewDirectedStakeLamportsWithVote;

pub async fn command_view_directed_stake_lamports_with_vote(
    args: ViewDirectedStakeLamportsWithVote,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.steward_config;
    let stake_meta_address = get_directed_stake_meta_address(&steward_config, &program_id);
    
    // Fetch directed stake meta
    let stake_meta =
        get_directed_stake_meta(client.clone(), &steward_config, &program_id).await?;

    if args.print_json {
        let mut entries = Vec::new();
        for i in 0..stake_meta.total_stake_targets as usize {
            let target = &stake_meta.targets[i];
            if target.vote_pubkey != Pubkey::default() {
                entries.push(serde_json::json!({
                    "vote_pubkey": target.vote_pubkey.to_string(),
                    "total_staked_lamports": target.total_staked_lamports,
                }));
            }
        }
        let output = serde_json::json!({
            "directed_stake_meta_address": stake_meta_address.to_string(),
            "entries": entries,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Directed Stake Meta Address: {}", stake_meta_address);
        println!("\nDirected Stake Targets (Vote Pubkey and Staked Lamports):");
        println!("{:<45} {:>20}", "Vote Pubkey", "Staked Lamports (SOL)");
        println!("{}", "-".repeat(67));

        for i in 0..stake_meta.total_stake_targets as usize {
            let target = &stake_meta.targets[i];
            if target.vote_pubkey != Pubkey::default() {
                if args.show_all || target.total_staked_lamports > 0 {
                    println!(
                        "{:<45} {:>20}",
                        target.vote_pubkey,
                        target.total_staked_lamports / 1_000_000_000
                    );
                }
            }
        }
    }

    Ok(())
}

