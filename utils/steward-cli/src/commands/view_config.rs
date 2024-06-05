use anchor_lang::AccountDeserialize;
use jito_steward::Config;
use solana_client::rpc_client::RpcClient;

use super::commands::ViewConfig;

pub fn command_view_config(args: ViewConfig, client: RpcClient) {
    let config_raw_account = client
        .get_account(&args.steward_config)
        .expect("Cannot find config account");

    let config_account: Config = Config::try_deserialize(&mut config_raw_account.data.as_slice())
        .expect("Cannot deserialize config account");

    println!("------- Config -------");
    println!("📚 Accounts 📚");
    println!("Config:      {}", args.steward_config);
    println!("Authority:   {}", config_account.authority);
    println!("Stake Pool:  {}", config_account.stake_pool);
    println!("\n↺ State ↺");
    println!("Is Paused:   {:?}", config_account.paused);
    println!("Blacklisted: {:?}", config_account.blacklist.count());
    println!("\n⚙️ Parameters ⚙️");
    println!(
        "Commission Range:  {:?}",
        config_account.parameters.commission_range
    );
    println!(
        "MEV Commission Range:  {:?}",
        config_account.parameters.mev_commission_range
    );
    println!(
        "Epoch Credits Range:  {:?}",
        config_account.parameters.epoch_credits_range
    );
    println!(
        "MEV Commission BPS Threshold:  {:?}",
        config_account.parameters.mev_commission_bps_threshold
    );
    println!(
        "Scoring Delinquency Threshold Ratio:  {:?}",
        config_account
            .parameters
            .scoring_delinquency_threshold_ratio
    );
    println!(
        "Instant Unstake Delinquency Threshold Ratio:  {:?}",
        config_account
            .parameters
            .instant_unstake_delinquency_threshold_ratio
    );
    println!(
        "Commission Threshold:  {:?}",
        config_account.parameters.commission_threshold
    );
    println!(
        "Historical Commission Threshold:  {:?}",
        config_account.parameters.historical_commission_threshold
    );
    println!(
        "Number of Delegation Validators:  {:?}",
        config_account.parameters.num_delegation_validators
    );
    println!(
        "Scoring Unstake Cap BPS:  {:?}",
        config_account.parameters.scoring_unstake_cap_bps
    );
    println!(
        "Instant Unstake Cap BPS:  {:?}",
        config_account.parameters.instant_unstake_cap_bps
    );
    println!(
        "Stake Deposit Unstake Cap BPS:  {:?}",
        config_account.parameters.stake_deposit_unstake_cap_bps
    );
    println!(
        "Compute Score Slot Range:  {:?}",
        config_account.parameters.compute_score_slot_range
    );
    println!(
        "Instant Unstake Epoch Progress:  {:?}",
        config_account.parameters.instant_unstake_epoch_progress
    );
    println!(
        "Instant Unstake Inputs Epoch Progress:  {:?}",
        config_account
            .parameters
            .instant_unstake_inputs_epoch_progress
    );
    println!(
        "Number of Epochs Between Scoring:  {:?}",
        config_account.parameters.num_epochs_between_scoring
    );
    println!(
        "Minimum Stake Lamports:  {:?}",
        config_account.parameters.minimum_stake_lamports
    );
    println!(
        "Minimum Voting Epochs:  {:?}",
        config_account.parameters.minimum_voting_epochs
    );
    print!("---------------------")
}
