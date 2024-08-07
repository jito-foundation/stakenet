use anchor_lang::AccountDeserialize;
use anyhow::Result;
use jito_steward::{
    constants::LAMPORT_BALANCE_DEFAULT, utils::ValidatorList, Config, StewardStateAccount,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{account::Account, pubkey::Pubkey};
use spl_stake_pool::{
    find_stake_program_address, find_transient_stake_program_address, state::StakeStatus,
};
use std::{collections::HashMap, sync::Arc};
use validator_history::ValidatorHistory;

use crate::commands::command_args::ViewState;

use stakenet_sdk::utils::{
    accounts::{get_all_steward_accounts, get_validator_history_address},
    debug::{format_simple_steward_state_string, format_steward_state_string},
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

    if args.verbose {
        let vote_accounts: Vec<Pubkey> = all_steward_accounts
            .validator_list_account
            .validators
            .iter()
            .map(|validator| validator.vote_account_address)
            .collect();

        let history_accounts_to_fetch: Vec<Pubkey> = vote_accounts
            .iter()
            .map(|vote_account| {
                get_validator_history_address(vote_account, &validator_history::id())
            })
            .collect();

        let raw_history_accounts: Vec<Option<Account>> = {
            let chunk_size = 100;
            let mut all_accounts = Vec::new();

            for chunk in history_accounts_to_fetch.chunks(chunk_size) {
                let accounts_chunk = client.get_multiple_accounts(chunk).await?;
                all_accounts.extend(accounts_chunk);
            }

            all_accounts
        };

        let all_history_map: HashMap<Pubkey, Option<Account>> = vote_accounts
            .into_iter()
            .zip(raw_history_accounts)
            .collect();

        _print_verbose_state(
            &all_steward_accounts.state_account,
            &all_steward_accounts.config_account,
            &all_steward_accounts.validator_list_account,
            &all_history_map,
        );
    } else {
        _print_default_state(
            &steward_config,
            &all_steward_accounts.state_address,
            &all_steward_accounts.state_account,
            &all_steward_accounts.validator_list_account,
            &all_steward_accounts.reserve_stake_account,
        );
    }

    Ok(())
}

fn _print_default_state(
    steward_config: &Pubkey,
    steward_state: &Pubkey,
    state_account: &StewardStateAccount,
    validator_list_account: &ValidatorList,
    reserve_stake_account: &Account,
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

    let mut non_zero_score_count = 0;
    for i in 0..state.num_pool_validators {
        if let Some(score) = state.scores.get(i as usize) {
            if *score != 0 {
                non_zero_score_count += 1;
            }
        }
    }

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
    formatted_string += "\n";
    formatted_string += &format!("num_pool_validators: {}\n", state.num_pool_validators);
    formatted_string += &format!(
        "validator list length: {}\n",
        validator_list_account.validators.len()
    );
    formatted_string += &format!(
        "Validators marked to remove: {}\n",
        state.validators_to_remove.count()
    );
    formatted_string += &format!(
        "Validators marked to remove immediately: {}\n",
        state.validators_for_immediate_removal.count()
    );
    formatted_string += &format!("Validators added: {}\n", state.validators_added);
    formatted_string += "\n";
    formatted_string += &format!(
        "Total Staked Lamports: {} ({:.2} ◎)\n",
        total_staked_lamports,
        total_staked_lamports as f64 / 10f64.powf(9.)
    );
    formatted_string += &format!(
        "Total Transient Lamports: {} ({:.2} ◎)\n",
        total_transient_lamports,
        total_transient_lamports as f64 / 10f64.powf(9.)
    );

    formatted_string += &format!(
        "Reserve Lamports: {} ({:.2} ◎)\n",
        reserve_stake_account.lamports,
        reserve_stake_account.lamports as f64 / 10f64.powf(9.)
    );
    formatted_string += "\n";
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
    formatted_string += &format!("Non Zero Scores: {}\n", non_zero_score_count);
    formatted_string += "\n";
    formatted_string += &format!(
        "State: {}\n",
        format_steward_state_string(&state_account.state)
    );
    formatted_string += &format!(
        "State: {}\n",
        format_simple_steward_state_string(&state_account.state)
    );
    formatted_string += "\n";

    formatted_string += "---------------------";

    println!("{}", formatted_string)
}

fn _print_verbose_state(
    steward_state_account: &StewardStateAccount,
    config_account: &Config,
    validator_list_account: &ValidatorList,
    validator_histories: &HashMap<Pubkey, Option<Account>>,
) {
    let mut formatted_string;

    let mut top_scores: Vec<(Pubkey, u32)> = vec![];

    for (index, validator) in validator_list_account.validators.iter().enumerate() {
        let history_info = validator_histories
            .get(&validator.vote_account_address)
            .and_then(|account| account.as_ref())
            .and_then(|account| {
                ValidatorHistory::try_deserialize(&mut account.data.as_slice()).ok()
            });

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
            "Internal Validator Lamports: {}\n",
            match steward_state_account
                .state
                .validator_lamport_balances
                .get(index)
            {
                Some(&LAMPORT_BALANCE_DEFAULT) | None => "Unset".to_string(),
                Some(&lamports) => lamports.to_string(),
            }
        );
        formatted_string += &format!("Index: {}\n", index);

        formatted_string += &format!(
            "Marked for removal: {}\n",
            steward_state_account
                .state
                .validators_to_remove
                .get(index)
                .unwrap_or_default()
        );
        formatted_string += &format!(
            "Marked for immediate removal: {}\n",
            steward_state_account
                .state
                .validators_for_immediate_removal
                .get(index)
                .unwrap_or_default()
        );
        formatted_string += &format!(
            "Is Instant Unstake: {}\n",
            steward_state_account
                .state
                .instant_unstake
                .get(index)
                .unwrap_or_default()
        );
        formatted_string += &format!(
            "Score: {}\n",
            steward_state_account.state.scores.get(index).unwrap_or(&0)
        );
        formatted_string += &format!(
            "Yield Score: {}\n",
            steward_state_account
                .state
                .yield_scores
                .get(index)
                .unwrap_or(&0)
        );
        formatted_string += &format!("Score Index: {:?}\n", score_index);
        formatted_string += &format!("Yield Score Index: {:?}\n", yield_score_index);

        if let Some(history_info) = history_info {
            formatted_string += &format!(
                "\nValidator History Index: {}\n",
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
        formatted_string += &format!(
            "Active Lamports: {:?} ({:.2} ◎)\n",
            u64::from(validator.active_stake_lamports),
            u64::from(validator.active_stake_lamports) as f64 / 10f64.powf(9.),
        );
        formatted_string += &format!(
            "Transient Lamports: {:?} ({:.2} ◎)\n",
            u64::from(validator.transient_stake_lamports),
            u64::from(validator.transient_stake_lamports) as f64 / 10f64.powf(9.),
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

        if let Some(score) = steward_state_account.state.scores.get(index) {
            if *score != 0 {
                top_scores.push((vote_account, *score));
            }
        }

        println!("{}", formatted_string);
    }

    println!("\nAll Ranked Validators ( {} ): \n", top_scores.len());
    println!("{:<45} : Score\n", "Vote Account");

    top_scores.sort_by(|a, b| b.1.cmp(&a.1));
    top_scores.iter().for_each(|(vote_account, score)| {
        let formatted_score =
            format!("{}", score)
                .chars()
                .rev()
                .enumerate()
                .fold(String::new(), |acc, (i, c)| {
                    if i > 0 && i % 3 == 0 {
                        format!("{}_{}", c, acc)
                    } else {
                        format!("{}{}", c, acc)
                    }
                });
        let vote_account = format!("{:?}", vote_account);
        println!("{:<45} : {}", vote_account, formatted_score);
    });
}
