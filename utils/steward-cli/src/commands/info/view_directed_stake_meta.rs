use crate::commands::command_args::ViewDirectedStakeMeta;
use anyhow::Result;
use bytemuck::from_bytes;
use jito_steward::state::directed_stake::DirectedStakeMeta;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

pub async fn command_view_directed_stake_meta(
    args: ViewDirectedStakeMeta,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let (directed_stake_meta_pda, _bump) = Pubkey::find_program_address(
        &[DirectedStakeMeta::SEED, args.steward_config.as_ref()],
        &program_id,
    );

    println!("Fetching DirectedStakeMeta account...");
    println!("  Steward Config: {}", args.steward_config);
    println!("  DirectedStakeMeta PDA: {}", directed_stake_meta_pda);

    let account = client.get_account(&directed_stake_meta_pda).await?;
    let stake_meta = from_bytes::<DirectedStakeMeta>(&account.data);

    println!("\n📊 DirectedStakeMeta Information:");
    println!("  Epoch: {}", stake_meta.epoch);
    println!(
        "  Progress: {}/{} targets uploaded",
        stake_meta.uploaded_stake_targets, stake_meta.total_stake_targets
    );

    let progress_percentage = if stake_meta.total_stake_targets > 0 {
        (stake_meta.uploaded_stake_targets as f64 / stake_meta.total_stake_targets as f64) * 100.0
    } else {
        0.0
    };
    println!("  Progress: {:.1}%", progress_percentage);

    if stake_meta.is_copy_complete() {
        println!("  Status: ✅ Copy Complete");
    } else {
        println!("  Status: 🔄 Copy In Progress");
    }

    println!("\n🎯 Stake Targets:");
    if stake_meta.uploaded_stake_targets == 0 {
        println!("  No targets uploaded yet.");
    } else {
        for i in 0..stake_meta.uploaded_stake_targets as usize {
            let target = &stake_meta.targets[i];
            if target.vote_pubkey != Pubkey::default() {
                println!("  Target {}:", i + 1);
                println!("    Vote Pubkey: {}", target.vote_pubkey);
                println!("    Target Lamports: {}", target.total_target_lamports);
                println!("    Staked Lamports: {}", target.total_staked_lamports);
                println!();
            }
        }
    }

    if args.print_json {
        println!("\n📄 JSON Output:");
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "epoch": stake_meta.epoch,
                "total_stake_targets": stake_meta.total_stake_targets,
                "uploaded_stake_targets": stake_meta.uploaded_stake_targets,
                "is_copy_complete": stake_meta.is_copy_complete(),
                "progress_percentage": progress_percentage,
                "targets": stake_meta.targets.iter()
                    .take(stake_meta.uploaded_stake_targets as usize)
                    .filter(|target| target.vote_pubkey != Pubkey::default())
                    .map(|target| {
                        serde_json::json!({
                            "vote_pubkey": target.vote_pubkey.to_string(),
                            "total_target_lamports": target.total_target_lamports,
                            "total_staked_lamports": target.total_staked_lamports
                        })
                    })
                    .collect::<Vec<_>>()
            }))?
        );
    }

    Ok(())
}
