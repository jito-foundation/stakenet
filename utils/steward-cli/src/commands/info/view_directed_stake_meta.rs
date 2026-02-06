use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use stakenet_sdk::utils::accounts::{get_directed_stake_meta, get_directed_stake_meta_address};

#[derive(Parser)]
#[command(about = "View DirectedStakeMeta account contents")]
pub struct ViewDirectedStakeMeta {
    /// Steward config account
    #[arg(long, env)]
    pub steward_config: Pubkey,

    /// Print account information in JSON format
    #[arg(long)]
    pub vote_pubkey: Option<Pubkey>,
}

pub async fn command_view_directed_stake_meta(
    args: ViewDirectedStakeMeta,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let stake_meta_address = get_directed_stake_meta_address(&args.steward_config, &program_id);
    let stake_meta =
        get_directed_stake_meta(client.clone(), &args.steward_config, &program_id).await?;
    let _stake_meta_address = get_directed_stake_meta_address(&args.steward_config, &program_id);

    println!("Directed stake meta: {}", _stake_meta_address);

    println!("\n📊 DirectedStakeMeta Information:");
    println!("\nDirectedStakeMeta Account: {stake_meta_address}");
    println!(
        "DirectedStakeMeta Total Stake Targets: {}",
        stake_meta.total_stake_targets
    );
    println!(
        "DirectedStakeMeta Directed Unstake Total: {}",
        stake_meta.directed_unstake_total
    );

    println!("\n🎯 Stake Targets:");
    for i in 0..stake_meta.total_stake_targets as usize {
        let validator = &stake_meta.targets[i];
        if validator.vote_pubkey == Pubkey::default() {
            continue;
        }

        if let Some(vote_pubkey) = args.vote_pubkey {
            if validator.vote_pubkey != vote_pubkey {
                continue;
            }
        }

        println!("  Target {}:", i + 1);
        println!("    Vote Pubkey: {}", validator.vote_pubkey);
        println!("    Target Lamports: {}", validator.total_target_lamports);
        println!(
            "    Target Last Updated Epoch: {}",
            validator.target_last_updated_epoch
        );
        println!("    Staked Lamports: {}", validator.total_staked_lamports);
        println!(
            "    Staked Last Updated Epoch: {}",
            validator.staked_last_updated_epoch
        );
        println!();

        if args.vote_pubkey.is_some() {
            break;
        }
    }

    Ok(())
}
