use anchor_lang::AccountDeserialize;
use jito_steward::{Config, StewardStateAccount};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;

use super::commands::ViewConfig;

pub fn command_view_config(args: ViewConfig, client: RpcClient) {
    let steward_config = args.steward_config;

    let config_raw_account = client
        .get_account(&steward_config)
        .expect("Cannot find config account");

    let config_account: Config = Config::try_deserialize(&mut config_raw_account.data.as_slice())
        .expect("Cannot deserialize config account");

    let (steward_state, _) = Pubkey::find_program_address(
        &[StewardStateAccount::SEED, steward_config.as_ref()],
        &jito_steward::id(),
    );

    let mut output = String::new(); // Initialize the string directly
    output = _print_default_config(&steward_config, &steward_state, &config_account).to_string();

    println!("{}", output);
}

fn _print_default_config(
    steward_config: &Pubkey,
    steward_state: &Pubkey,
    config_account: &Config,
) -> String {
    let mut formatted_string = String::new();

    formatted_string += "------- Config -------\n";
    formatted_string += "📚 Accounts 📚\n";
    formatted_string += &format!("Config:      {}\n", steward_config);
    formatted_string += &format!("Authority:   {}\n", config_account.authority);
    formatted_string += &format!("State:       {}\n", steward_state);
    formatted_string += &format!("Stake Pool:  {}\n", config_account.stake_pool);
    formatted_string += "\n↺ State ↺\n";
    formatted_string += &format!("Is Paused:   {:?}\n", config_account.paused);
    formatted_string += &format!("Blacklisted: {:?}\n", config_account.blacklist.count());
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
    formatted_string += "---------------------";

    formatted_string
}
