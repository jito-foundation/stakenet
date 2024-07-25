use anyhow::Result;
use jito_steward::{utils::ValidatorList, Config, StewardStateAccount};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use spl_stake_pool::{
    find_stake_program_address, find_transient_stake_program_address, state::StakeStatus,
};
use std::sync::Arc;
use validator_history::ValidatorHistory;

use crate::{
    commands::command_args::ViewState,
    utils::accounts::{
        format_simple_state_string, format_state_string, get_all_steward_accounts,
        get_validator_history_accounts_with_retry,
    },
};

pub async fn command_view_state(
    args: ViewState,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    println!("Fetching a lot of accounts, please use a custom RPC for better performance");

    let steward_config = args.view_parameters.steward_config;

    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;

    let all_history_accounts =
        get_validator_history_accounts_with_retry(client, validator_history::id()).await?;

    if args.verbose {
        _print_verbose_state(
            &all_steward_accounts.state_account,
            &all_steward_accounts.config_account,
            &all_steward_accounts.validator_list_account,
            &all_history_accounts,
        );
    } else {
        _print_default_state(
            &steward_config,
            &all_steward_accounts.state_address,
            &all_steward_accounts.state_account,
            &all_steward_accounts.validator_list_account,
        );
    }

    Ok(())
}

fn _print_verbose_state(
    steward_state_account: &StewardStateAccount,
    config_account: &Config,
    validator_list_account: &ValidatorList,
    validator_histories: &Vec<ValidatorHistory>,
) {
    let mut formatted_string;

    for (index, validator) in validator_list_account.validators.iter().enumerate() {
        let mut history_info: Option<ValidatorHistory> = None;
        for validator_history in validator_histories.iter() {
            if validator_history.vote_account == validator.vote_account_address {
                history_info = Some(*validator_history);
                break;
            }
        }

        let vote_account = validator.vote_account_address;
        let (stake_address, _) = find_stake_program_address(
            &spl_stake_pool::id(),
            &vote_account,
            &config_account.stake_pool,
            None,
        );

        let (transient_stake_address, _) = find_transient_stake_program_address(
            &spl_stake_pool::id(),
            &vote_account,
            &config_account.stake_pool,
            validator.transient_seed_suffix.into(),
        );

        let score_index = steward_state_account
            .state
            .sorted_score_indices
            .iter()
            .position(|&i| i == index as u16);
        let yield_score_index = steward_state_account
            .state
            .sorted_yield_score_indices
            .iter()
            .position(|&i| i == index as u16);

        formatted_string = String::new();

        formatted_string += &format!("Vote Account: {:?}\n", vote_account);
        formatted_string += &format!("Stake Account: {:?}\n", stake_address);
        formatted_string += &format!("Transient Stake Account: {:?}\n", transient_stake_address);
        formatted_string += &format!(
            "Validator Lamports: {:?}\n",
            u64::from(validator.active_stake_lamports)
        );
        formatted_string += &format!("Index: {:?}\n", index);
        // formatted_string += &format!(
        //     "Is Blacklisted: {:?}\n",
        //     config_account.validator_history_blacklist.get(index)
        // );
        formatted_string += &format!(
            "Marked for removal: {:?}\n",
            steward_state_account.state.validators_to_remove.get(index)
        );
        formatted_string += &format!(
            "Is Instant Unstake: {:?}\n",
            steward_state_account.state.instant_unstake.get(index)
        );
        formatted_string += &format!(
            "Score: {:?}\n",
            steward_state_account.state.scores.get(index)
        );
        formatted_string += &format!(
            "Yield Score: {:?}\n",
            steward_state_account.state.yield_scores.get(index)
        );
        formatted_string += &format!("Score Index: {:?}\n", score_index);
        formatted_string += &format!("Yield Score Index: {:?}\n", yield_score_index);

        if let Some(history_info) = history_info {
            formatted_string += &format!(
                "\nValidator History Index: {:?}\n",
                format!("{:?}", history_info.index)
            );

            formatted_string += &format!(
                "Is blacklisted: {:?}\n",
                format!(
                    "{:?}",
                    config_account
                        .validator_history_blacklist
                        .get_unsafe(history_info.index as usize)
                )
            );
        }

        formatted_string += "\n";
        formatted_string += &format!("Active Lamports: {:?}\n", validator.active_stake_lamports);
        formatted_string += &format!(
            "Transient Lamports: {:?}\n",
            validator.transient_stake_lamports
        );

        let status = match StakeStatus::try_from(validator.status).unwrap() {
            StakeStatus::Active => "🟩 Active",
            StakeStatus::DeactivatingAll => "🟨 Deactivating All",
            StakeStatus::DeactivatingTransient => "🟨 Deactivating Transient",
            StakeStatus::DeactivatingValidator => "🟥 Deactivating Validator",
            StakeStatus::ReadyForRemoval => "🟥 Ready for Removal",
        };
        formatted_string += &format!("Status: {}\n", status);

        formatted_string += "\n";

        println!("{}", formatted_string);
    }
}

fn _print_default_state(
    steward_config: &Pubkey,
    steward_state: &Pubkey,
    state_account: &StewardStateAccount,
    validator_list_account: &ValidatorList,
) {
    let state = &state_account.state;

    let mut total_staked_lamports = 0;
    let mut total_transient_lamports = 0;
    let mut active_validators = 0;
    let mut deactivating_validators = 0;
    let mut ready_for_removal_validators = 0;
    let mut deactivating_all_validators = 0;
    let mut deactivating_transient_validators = 0;
    validator_list_account
        .clone()
        .validators
        .iter()
        .for_each(|validator| {
            total_staked_lamports += u64::from(validator.active_stake_lamports);
            total_transient_lamports += u64::from(validator.transient_stake_lamports);

            match StakeStatus::try_from(validator.status).unwrap() {
                StakeStatus::Active => {
                    active_validators += 1;
                }
                StakeStatus::DeactivatingTransient => {
                    deactivating_transient_validators += 1;
                }
                StakeStatus::ReadyForRemoval => {
                    ready_for_removal_validators += 1;
                }
                StakeStatus::DeactivatingValidator => {
                    deactivating_validators += 1;
                }
                StakeStatus::DeactivatingAll => {
                    deactivating_all_validators += 1;
                }
            }
        });

    let mut formatted_string = String::new();

    formatted_string += "------- State -------\n";
    formatted_string += "📚 Accounts 📚\n";
    formatted_string += &format!("Config:      {}\n", steward_config);
    formatted_string += &format!("State:       {}\n", steward_state);
    formatted_string += "\n";
    formatted_string += "↺ State ↺\n";
    formatted_string += &format!("State Tag: {}\n", state.state_tag);
    formatted_string += &format!(
        "Progress: {:?} / {} ({} remaining)\n",
        state.progress.count(),
        state.num_pool_validators,
        state.num_pool_validators - state.progress.count() as u64
    );
    formatted_string += &format!(
        "Validator Lamport Balances Count: {}\n",
        state.validator_lamport_balances.len()
    );
    formatted_string += &format!("Scores Count: {}\n", state.scores.len());
    formatted_string += &format!(
        "Sorted Score Indices Count: {}\n",
        state.sorted_score_indices.len()
    );
    formatted_string += &format!("Yield Scores Count: {}\n", state.yield_scores.len());
    formatted_string += &format!(
        "Sorted Yield Score Indices Count: {}\n",
        state.sorted_yield_score_indices.len()
    );
    formatted_string += &format!("Delegations Count: {}\n", state.delegations.len());
    formatted_string += &format!("Instant Unstake: {:?}\n", state.instant_unstake.count());
    formatted_string += &format!(
        "Progress: {:?} / {} ( {} left )\n",
        state.progress.count(),
        state.num_pool_validators,
        state.num_pool_validators - state.progress.count() as u64
    );
    formatted_string += &format!(
        "Start Computing Scores Slot: {}\n",
        state.start_computing_scores_slot
    );
    formatted_string += &format!("Current Epoch: {}\n", state.current_epoch);
    formatted_string += &format!("Next Cycle Epoch: {}\n", state.next_cycle_epoch);
    formatted_string += &format!("Number of Pool Validators: {}\n", state.num_pool_validators);
    formatted_string += &format!("Scoring Unstake Total: {}\n", state.scoring_unstake_total);
    formatted_string += &format!("Instant Unstake Total: {}\n", state.instant_unstake_total);
    formatted_string += &format!(
        "Stake Deposit Unstake Total: {}\n",
        state.stake_deposit_unstake_total
    );

    formatted_string += &format!("Padding0 Length: {}\n", state._padding0.len());
    formatted_string += "\n\n";
    formatted_string += &format!("num_pool_validators: {}\n", state.num_pool_validators);
    formatted_string += &format!(
        "validator list length: {}\n",
        validator_list_account.validators.len()
    );
    formatted_string += &format!(
        "Validators marked to remove: {}\n",
        state.validators_to_remove.count()
    );
    formatted_string += &format!("Validators added: {}\n", state.validators_added);
    formatted_string += "\n";
    formatted_string += &format!("Total Staked Lamports: {}\n", total_staked_lamports);
    formatted_string += &format!("Total Transient Lamports: {}\n", total_transient_lamports);
    formatted_string += &format!("🟩 Active Validators: {}\n", active_validators);
    formatted_string += &format!(
        "🟨 Deactivating Transient Validators : {}\n",
        deactivating_transient_validators
    );
    formatted_string += &format!(
        "🟨 Deactivating All Validators: {}\n",
        deactivating_all_validators
    );
    formatted_string += &format!("🟥 Deactivating Validators: {}\n", deactivating_validators);
    formatted_string += &format!(
        "🟥 Ready for Removal Validators: {}\n",
        ready_for_removal_validators
    );
    formatted_string += "\n";
    formatted_string += &format!("State: {}\n", format_state_string(&state_account.state));
    formatted_string += &format!(
        "State: {}\n",
        format_simple_state_string(&state_account.state)
    );
    formatted_string += "\n";
    formatted_string += "---------------------";

    println!("{}", formatted_string)
}
