use std::{str::FromStr, sync::Arc};

use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use stakenet_sdk::utils::accounts::{get_all_steward_accounts, get_all_validator_history_accounts};

#[derive(Parser)]
#[command(about = "View the current blacklist")]
pub struct ViewBamValidators {
    pub epoch: u64,
}

pub async fn command_view_bam_validators(args: ViewBamValidators, rpc_url: String) -> Result<()> {
    let client = RpcClient::new(rpc_url);
    let validator_histories =
        get_all_validator_history_accounts(&client, validator_history::id()).await?;
    let client = Arc::new(client);
    let all_steward_accounts = get_all_steward_accounts(
        &client,
        &jito_steward::id(),
        &Pubkey::from_str("5pZmpk3ktweGZW9xFknpEHhQoWeAKTzSGwnCUyVdiye")?,
    )
    .await?;

    let mut bam_validators: Vec<Pubkey> = Vec::new();
    for validator_history in validator_histories {
        for entry in validator_history.history.arr {
            if entry.epoch.eq(&(args.epoch as u16)) && entry.client_type.eq(&6) {
                bam_validators.push(validator_history.vote_account);
            }
        }
    }

    for validator in all_steward_accounts
        .validator_list_account
        .validators
        .iter()
        .take(50)
    {
        bam_validators.push(validator.vote_account_address);
    }

    // let mut join = Vec::new();
    // let accounts = client.get_vote_accounts().await?;
    // for account in accounts.current {
    //     for bam_validator in bam_validators.clone().into_iter() {
    //         if bam_validator.to_string().eq(&account.vote_pubkey) {
    //             join.push((account.node_pubkey.clone(), account.vote_pubkey.clone()));
    //         }
    //     }
    // }

    for (i, v) in bam_validators.iter().enumerate() {
        println!("{}", v);
    }

    println!(
        "Stake pool accounts: {}",
        all_steward_accounts.stake_pool_account.total_lamports
    );

    Ok(())
}
