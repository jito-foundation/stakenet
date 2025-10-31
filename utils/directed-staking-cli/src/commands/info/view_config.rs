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
    _program_id: Pubkey,
) -> Result<()> {
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

    Ok(())
}
