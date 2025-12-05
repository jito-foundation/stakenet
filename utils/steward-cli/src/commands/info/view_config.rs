use std::sync::Arc;

use anyhow::Result;
use jito_steward::Config;
use solana_client::nonblocking::rpc_client::RpcClient;

use solana_sdk::pubkey::Pubkey;

use crate::commands::command_args::ViewConfig;
use stakenet_sdk::utils::accounts::get_all_steward_accounts;

pub async fn command_view_config(
    args: ViewConfig,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.view_parameters.steward_config;

    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;

    _print_default_config(
        &all_steward_accounts.config_address,
        &all_steward_accounts.state_address,
        &all_steward_accounts.config_account,
        &all_steward_accounts.stake_pool_account.staker,
    );

    Ok(())
}

fn _print_default_config(
    steward_config: &Pubkey,
    steward_state: &Pubkey,
    config_account: &Config,
    staker: &Pubkey,
) {
    let mut formatted_string = String::new();

    formatted_string += "------- Config -------\n";
    formatted_string += "📚 Accounts 📚\n";
    formatted_string += &format!("Config:      {}\n", steward_config);
    formatted_string += &format!("Admin:   {}\n", config_account.admin);
    formatted_string += &format!("Blacklist Auth:   {}\n", config_account.blacklist_authority);
    formatted_string += &format!(
        "Parameter Auth:   {}\n",
        config_account.parameters_authority
    );
    formatted_string += &format!(
        "Directed Stake Whitelist Auth:   {}\n",
        config_account.directed_stake_whitelist_authority
    );
    formatted_string += &format!(
        "Directed Stake Meta Upload Auth:   {}\n",
        config_account.directed_stake_meta_upload_authority
    );
    formatted_string += &format!(
        "Directed Stake Ticket Override Auth:   {}\n",
        config_account.directed_stake_ticket_override_authority
    );
    formatted_string += &format!("Staker:      {}\n", staker);
    formatted_string += &format!("State:       {}\n", steward_state);
    formatted_string += &format!("Stake Pool:  {}\n", config_account.stake_pool);
    formatted_string += &format!("Validator List:  {}\n", config_account.validator_list);
    formatted_string += &format!(
        "Priority Fee Parameters Authority:   {}\n",
        config_account.priority_fee_parameters_authority
    );
    formatted_string += "\n↺ State ↺\n";
    formatted_string += &format!("Is Paused:   {:?}\n", config_account.paused);
    formatted_string += &format!(
        "Blacklisted: {:?}\n",
        config_account.validator_history_blacklist.count()
    );
    formatted_string += "\n⚙️ Parameters ⚙️\n";
    formatted_string += &format!(
        "Commission Range:  {:?}\n",
        config_account.parameters.commission_range
    );
    formatted_string += &format!(
        "MEV Commission Range:  {:?}\n",
        config_account.parameters.mev_commission_range
    );
    formatted_string += &format!(
        "Epoch Credits Range:  {:?}\n",
        config_account.parameters.epoch_credits_range
    );
    formatted_string += &format!(
        "MEV Commission BPS Threshold:  {:?}\n",
        config_account.parameters.mev_commission_bps_threshold
    );
    formatted_string += &format!(
        "Scoring Delinquency Threshold Ratio:  {:?}\n",
        config_account
            .parameters
            .scoring_delinquency_threshold_ratio
    );
    formatted_string += &format!(
        "Instant Unstake Delinquency Threshold Ratio:  {:?}\n",
        config_account
            .parameters
            .instant_unstake_delinquency_threshold_ratio
    );
    formatted_string += &format!(
        "Commission Threshold:  {:?}\n",
        config_account.parameters.commission_threshold
    );
    formatted_string += &format!(
        "Historical Commission Threshold:  {:?}\n",
        config_account.parameters.historical_commission_threshold
    );
    formatted_string += &format!(
        "Number of Delegation Validators:  {:?}\n",
        config_account.parameters.num_delegation_validators
    );
    formatted_string += &format!(
        "Scoring Unstake Cap BPS:  {:?}\n",
        config_account.parameters.scoring_unstake_cap_bps
    );
    formatted_string += &format!(
        "Instant Unstake Cap BPS:  {:?}\n",
        config_account.parameters.instant_unstake_cap_bps
    );
    formatted_string += &format!(
        "Stake Deposit Unstake Cap BPS:  {:?}\n",
        config_account.parameters.stake_deposit_unstake_cap_bps
    );
    formatted_string += &format!(
        "Compute Score Slot Range:  {:?}\n",
        config_account.parameters.compute_score_slot_range
    );
    formatted_string += &format!(
        "Instant Unstake Epoch Progress:  {:?}\n",
        config_account.parameters.instant_unstake_epoch_progress
    );
    formatted_string += &format!(
        "Instant Unstake Inputs Epoch Progress:  {:?}\n",
        config_account
            .parameters
            .instant_unstake_inputs_epoch_progress
    );
    formatted_string += &format!(
        "Number of Epochs Between Scoring:  {:?}\n",
        config_account.parameters.num_epochs_between_scoring
    );
    formatted_string += &format!(
        "Minimum Stake Lamports:  {:?}\n",
        config_account.parameters.minimum_stake_lamports
    );
    formatted_string += &format!(
        "Minimum Voting Epochs:  {:?}\n",
        config_account.parameters.minimum_voting_epochs
    );
    formatted_string += &format!(
        "Compute Score Epoch Progress:  {:?}\n",
        config_account.parameters.compute_score_epoch_progress
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
    formatted_string += "\n⚙️ Directed Stake Parameters ⚙️\n";
    formatted_string += &format!(
        "Directed Stake Unstake Cap BPS:  {:?}\n",
        config_account.parameters.directed_stake_unstake_cap_bps
    );
    formatted_string += &format!(
        "Undirected Stake Ceiling Lamports:  {:?}\n",
        config_account
            .parameters
            .undirected_stake_ceiling_lamports()
    );
    formatted_string += "---------------------";

    println!("{}", formatted_string)
}
