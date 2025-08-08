use std::sync::Arc;

use anyhow::Result;
use jito_steward::Config;
use solana_client::nonblocking::rpc_client::RpcClient;

use solana_sdk::pubkey::Pubkey;

use crate::commands::command_args::ViewPriorityFeeConfig;
use stakenet_sdk::utils::accounts::get_all_steward_accounts;

pub async fn command_view_priority_fee_config(
    args: ViewPriorityFeeConfig,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.view_parameters.steward_config;

    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;

    print_priority_fee_config(
        &all_steward_accounts.config_address,
        &all_steward_accounts.config_account,
    );

    Ok(())
}

fn print_priority_fee_config(steward_config: &Pubkey, config_account: &Config) {
    let mut formatted_string = String::new();

    formatted_string += "------- Priority Fee Config -------\n";
    formatted_string += "📚 Accounts 📚\n";
    formatted_string += &format!("Config:      {}\n", steward_config);
    formatted_string += &format!(
        "Priority Fee Parameters Authority:   {}\n",
        config_account.priority_fee_parameters_authority
    );
    formatted_string += "\n⚙️ Priority Fee Parameters ⚙️\n";
    formatted_string += &format!(
        "Priority Fee Lookback Epochs:  {:?}\n",
        config_account.parameters.priority_fee_lookback_epochs
    );
    formatted_string += &format!(
        "Priority Fee Lookback Offset:  {:?}\n",
        config_account.parameters.priority_fee_lookback_offset
    );
    formatted_string += &format!(
        "Priority Fee Max Commission BPS:  {:?}\n",
        config_account.parameters.priority_fee_max_commission_bps
    );
    formatted_string += &format!(
        "Priority Fee Error Margin BPS:  {:?}\n",
        config_account.parameters.priority_fee_error_margin_bps
    );
    formatted_string += &format!(
        "Priority Fee Scoring Start Epoch:  {:?}\n",
        config_account.parameters.priority_fee_scoring_start_epoch
    );
    formatted_string += "--------------------------------\n";

    println!("{}", formatted_string);
}
