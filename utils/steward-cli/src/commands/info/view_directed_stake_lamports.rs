use std::sync::Arc;

use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use stakenet_sdk::utils::accounts::{get_directed_stake_meta, get_directed_stake_meta_address};

use crate::commands::command_args::ViewDirectedStakeLamports;

pub async fn command_view_directed_stake_lamports(
    args: ViewDirectedStakeLamports,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let stake_meta_address = get_directed_stake_meta_address(&args.steward_config, &program_id);
    let stake_meta =
        get_directed_stake_meta(client.clone(), &args.steward_config, &program_id).await?;

    if args.print_json {
        let mut entries = Vec::new();
        for (index, &lamports) in stake_meta.directed_stake_lamports.iter().enumerate() {
            entries.push(serde_json::json!({
                "index": index,
                "lamports": lamports,
            }));
        }
        let output = serde_json::json!({
            "directed_stake_meta_address": stake_meta_address.to_string(),
            "entries": entries,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Directed Stake Meta Address: {}", stake_meta_address);
        println!("\nDirected Stake Lamports Array:");
        println!("{:<10} {:>20}", "Index", "Lamports");
        println!("{}", "-".repeat(32));

        for (index, &lamports) in stake_meta.directed_stake_lamports.iter().enumerate() {
            if args.show_all || lamports > 0 {
                println!("{:<10} {:>20}", index, lamports/1_000_000_000);
            }
        }
    }

    Ok(())
}

