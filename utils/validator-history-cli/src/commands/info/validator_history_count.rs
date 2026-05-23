use anchor_lang::AccountDeserialize;
use anyhow::anyhow;
use clap::Parser;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use stakenet_sdk::utils::accounts::get_all_validator_history_accounts;
use validator_history::Config;

#[derive(Parser)]
#[command(about = "Show the number of ValidatorHistory accounts on chain")]
pub struct InfoValidatorHistoryCount {
    /// Print output in JSON format
    #[arg(long, default_value = "false")]
    pub print_json: bool,
}

pub async fn run(args: InfoValidatorHistoryCount, rpc_url: String) -> anyhow::Result<()> {
    let client = RpcClient::new(rpc_url);

    let validator_histories =
        get_all_validator_history_accounts(&client, validator_history::id())
            .await
            .map_err(|e| anyhow!("Failed to fetch validator history accounts: {e}"))?;

    let count = validator_histories.len();

    let (config_pda, _) =
        Pubkey::find_program_address(&[Config::SEED], &validator_history::id());
    let config_counter = client
        .get_account(&config_pda)
        .await
        .ok()
        .and_then(|account| Config::try_deserialize(&mut account.data.as_slice()).ok())
        .map(|config| config.counter);

    if args.print_json {
        let output = serde_json::json!({
            "validator_history_account_count": count,
            "config_counter": config_counter,
        });
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        println!("ValidatorHistory accounts on chain: {count}");
        if let Some(counter) = config_counter {
            println!("Config counter:                    {counter}");
        }
    }

    Ok(())
}
