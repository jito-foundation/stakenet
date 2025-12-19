use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use stakenet_sdk::utils::accounts::{get_all_steward_accounts, get_all_validator_history_accounts};

use crate::commands::command_args::ViewParameters;

#[derive(Parser)]
#[command(about = "View the current blacklist")]
pub struct ViewBlacklist {
    #[command(flatten)]
    pub view_parameters: ViewParameters,
}

pub async fn command_view_blacklist(
    args: ViewBlacklist,
    client: &Arc<RpcClient>,
    steward_program_id: Pubkey,
    validator_history_program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.view_parameters.steward_config;
    let all_steward_accounts =
        get_all_steward_accounts(client, &steward_program_id, &steward_config).await?;
    let validator_histories =
        get_all_validator_history_accounts(client, validator_history_program_id).await?;

    let mut blacklisted_validators = Vec::new();

    for validator_history in validator_histories {
        if let Ok(true) = all_steward_accounts
            .config_account
            .validator_history_blacklist
            .get(validator_history.index as usize)
        {
            blacklisted_validators.push((validator_history.index, validator_history.vote_account));
        }
    }

    if blacklisted_validators.is_empty() {
        println!("No validators are currently blacklisted.");
    } else {
        println!("Blacklisted Validators: {}", blacklisted_validators.len());
        println!("{:<8} Vote Account", "Index");
        println!("{}", "-".repeat(60));
        for (index, vote_account) in blacklisted_validators {
            println!("{index:<8} {vote_account}");
        }
    }

    Ok(())
}
