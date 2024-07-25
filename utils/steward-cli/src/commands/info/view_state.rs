use anyhow::Result;
use jito_steward::{utils::ValidatorList, Config, StewardStateAccount};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use spl_stake_pool::{find_stake_program_address, find_transient_stake_program_address};
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

        println!("{}", formatted_string);
    }
}

pub fn format_state(
    steward_config: &Pubkey,
    steward_state: &Pubkey,
    state_account: &StewardStateAccount,
) -> String {
    let state = &state_account.state;

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
    formatted_string += "---------------------";

    formatted_string
}

fn _print_default_state(
    steward_config: &Pubkey,
    steward_state: &Pubkey,
    state_account: &StewardStateAccount,
    validator_list_account: &ValidatorList,
) {
    let state = &state_account.state;

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
    formatted_string += &format!("State: {}\n", format_state_string(&state_account.state));
    formatted_string += &format!(
        "State: {}\n",
        format_simple_state_string(&state_account.state)
    );
    formatted_string += "\n";
    formatted_string += "---------------------";

    println!("{}", formatted_string)
}
