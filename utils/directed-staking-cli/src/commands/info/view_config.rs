use anchor_lang::AccountDeserialize;
use anyhow::Result;
use jito_steward::Config;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

use crate::commands::command_args::ViewConfig;

pub async fn command_view_config(
    args: ViewConfig,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    // let (directed_stake_meta_pda, _bump) = Pubkey::find_program_address(
    //     &[DirectedStakeMeta::SEED, args.steward_config.as_ref()],
    //     &program_id,
    // );

    // println!("Fetching DirectedStakeMeta account...");
    // println!("  Steward Config: {}", args.steward_config);
    // println!("  DirectedStakeMeta PDA: {}", directed_stake_meta_pda);

    let account = client.get_account(&args.config).await?;
    let config = Config::try_deserialize(&mut account.data.as_slice())?;

    println!("\n📊 DirectedStakeMeta Information:");

    println!(
        "\n🎯 Config stake meta: {}",
        config.directed_stake_meta_upload_authority
    );
    println!(
        "\n🎯 Config whitelist: {}",
        config.directed_stake_whitelist_authority
    );
    // for i in 0..stake_meta.total_stake_targets as usize {
    //     let validator = &stake_meta.targets[i];
    //     if validator.vote_pubkey != Pubkey::default() {
    //         println!("  Target {}:", i + 1);
    //         println!("    Vote Pubkey: {}", validator.vote_pubkey);
    //         println!("    Target Lamports: {}", validator.total_target_lamports);
    //         println!(
    //             "    Target Last Updated Epoch: {}",
    //             validator.target_last_updated_epoch
    //         );
    //         println!("    Staked Lamports: {}", validator.total_staked_lamports);
    //         println!(
    //             "    Staked Last Updated Epoch: {}",
    //             validator.staked_last_updated_epoch
    //         );
    //         println!();
    //     }
    // }

    Ok(())
}
