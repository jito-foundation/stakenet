use anyhow::Result;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use stakenet_sdk::utils::accounts::get_all_validator_history_accounts;

#[derive(Parser)]
#[command(about = "View the current blacklist")]
pub struct ViewBamValidators {
    pub epoch: u64,
}

pub async fn command_view_bam_validators(args: ViewBamValidators, rpc_url: String) -> Result<()> {
    let client = RpcClient::new(rpc_url);
    let validator_histories =
        get_all_validator_history_accounts(&client, validator_history::id()).await?;

    let mut bam_validators: Vec<Pubkey> = Vec::new();
    for validator_history in validator_histories {
        for entry in validator_history.history.arr {
            if entry.epoch.eq(&(args.epoch as u16)) && entry.client_type.eq(&6) {
                bam_validators.push(validator_history.vote_account);
            }
        }
        // if let Ok(true) = all_steward_accounts
        //     .config_account
        //     .validator_history_blacklist
        //     .get(validator_history.index as usize)
        // {
        //     blacklisted_validators.push((validator_history.index, validator_history.vote_account));
        // }
    }

    for bam_validator in bam_validators {
        println!("Bam validators: {bam_validator}");
    }

    // if blacklisted_validators.is_empty() {
    //     println!("No validators are currently blacklisted.");
    // } else {
    //     println!("Blacklisted Validators: {}", blacklisted_validators.len());
    //     println!("{:<8} Vote Account", "Index");
    //     println!("{}", "-".repeat(60));
    //     for (index, vote_account) in blacklisted_validators {
    //         println!("{:<8} {}", index, vote_account);
    //     }
    // }

    Ok(())
}
