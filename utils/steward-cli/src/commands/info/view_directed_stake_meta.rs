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

    println!("\n🎯 Stake Targets:");
    for i in 0..stake_meta.total_stake_targets as usize {
        let validator = &stake_meta.targets[i];
        if validator.vote_pubkey != Pubkey::default() {
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
        }
    }

    Ok(())
}
