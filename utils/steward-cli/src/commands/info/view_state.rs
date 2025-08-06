use anchor_lang::AccountDeserialize;
use anyhow::Result;
use jito_steward::{
    constants::LAMPORT_BALANCE_DEFAULT, stake_pool_utils::ValidatorList, Config, Delegation,
    StewardStateAccount,
};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{account::Account, native_token::lamports_to_sol, pubkey::Pubkey};
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

#[derive(Serialize, Deserialize, Debug)]
struct LamportBalance {
    lamports: u64,
    sol: f64,
}

impl LamportBalance {
    fn new(lamports: u64) -> Self {
        Self {
            lamports,
            sol: lamports_to_sol(lamports),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct AccountAddresses {
    config: String,
    state: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct StateProgress {
    completed: usize,
    total: u64,
    remaining: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StateInfo {
    state_tag: String,
    progress: StateProgress,
    validator_lamport_balances_count: usize,
    scores_count: usize,
    sorted_score_indices_count: usize,
    yield_scores_count: usize,
    sorted_yield_score_indices_count: usize,
    delegations_count: usize,
    instant_unstake_count: usize,
    start_computing_scores_slot: u64,
    current_epoch: u64,
    next_cycle_epoch: u64,
    num_pool_validators: u64,
    scoring_unstake_total: u64,
    instant_unstake_total: u64,
    stake_deposit_unstake_total: u64,
    validators_to_remove_count: usize,
    validators_for_immediate_removal_count: usize,
    validators_added: u16,
    non_zero_scores: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LamportSummary {
    total_staked: LamportBalance,
    total_transient: LamportBalance,
    reserve: LamportBalance,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ValidatorCounts {
    active: u32,
    deactivating_transient: u32,
    deactivating_all: u32,
    deactivating: u32,
    ready_for_removal: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StateStrings {
    detailed: String,
    simple: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DefaultStateOutput {
    accounts: AccountAddresses,
    state: StateInfo,
    lamports: LamportSummary,
    validator_counts: ValidatorCounts,
    state_strings: StateStrings,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ValidatorAddresses {
    vote_account: String,
    stake_account: String,
    transient_stake_account: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ValidatorHistoryOutput {
    index: u32,
    is_blacklisted: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ValidatorLamports {
    active: LamportBalance,
    transient: LamportBalance,
    steward_internal: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorStatus {
    Active,
    DeactivatingAll,
    DeactivatingTransient,
    DeactivatingValidator,
    ReadyForRemoval,
}

impl From<StakeStatus> for ValidatorStatus {
    fn from(status: StakeStatus) -> Self {
        match status {
            StakeStatus::Active => ValidatorStatus::Active,
            StakeStatus::DeactivatingAll => ValidatorStatus::DeactivatingAll,
            StakeStatus::DeactivatingTransient => ValidatorStatus::DeactivatingTransient,
            StakeStatus::DeactivatingValidator => ValidatorStatus::DeactivatingValidator,
            StakeStatus::ReadyForRemoval => ValidatorStatus::ReadyForRemoval,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ValidatorDetails {
    addresses: ValidatorAddresses,
    steward_list_index: usize,
    overall_rank: Option<usize>,
    score: u32,
    yield_score: u32,
    passing_eligibility_criteria: String,
    target_delegation_percent: f64,
    is_instant_unstake: bool,
    validator_history_output: Option<ValidatorHistoryOutput>,
    lamports: ValidatorLamports,
    status: ValidatorStatus,
    marked_for_removal: bool,
    marked_for_immediate_removal: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RankedValidator {
    vote_account: String,
    score: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RankedValidatorsSummary {
    count: usize,
    validators: Vec<RankedValidator>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct VerboseStateOutput {
    validators: Vec<ValidatorDetails>,
    ranked_validators: Option<RankedValidatorsSummary>,
}

pub async fn command_view_state(
    args: ViewState,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    if !args.view_parameters.print_json {
        println!("Fetching a lot of accounts, please use a custom RPC for better performance");
    }

    let steward_config = args.view_parameters.steward_config;
    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;
    if args.verbose || args.vote_account.is_some() {
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
            args.vote_account,
            args.view_parameters.print_json,
        );
    } else {
        _print_default_state(
            &steward_config,
            &all_steward_accounts.state_address,
            &all_steward_accounts.state_account,
            &all_steward_accounts.validator_list_account,
            &all_steward_accounts.reserve_stake_account,
            args.view_parameters.print_json,
        );
    }

    Ok(())
}

fn build_default_state_output(
    steward_config: &Pubkey,
    steward_state: &Pubkey,
    state_account: &StewardStateAccount,
    validator_list_account: &ValidatorList,
    reserve_stake_account: &Account,
) -> DefaultStateOutput {
    let state = &state_account.state;

    let mut total_staked_lamports = 0;
    let mut total_transient_lamports = 0;
    let mut validator_counts = ValidatorCounts {
        active: 0,
        deactivating_transient: 0,
        deactivating_all: 0,
        deactivating: 0,
        ready_for_removal: 0,
    };

    validator_list_account
        .validators
        .iter()
        .for_each(|validator| {
            total_staked_lamports += u64::from(validator.active_stake_lamports);
            total_transient_lamports += u64::from(validator.transient_stake_lamports);

            match StakeStatus::try_from(validator.status).unwrap() {
                StakeStatus::Active => validator_counts.active += 1,
                StakeStatus::DeactivatingTransient => validator_counts.deactivating_transient += 1,
                StakeStatus::ReadyForRemoval => validator_counts.ready_for_removal += 1,
                StakeStatus::DeactivatingValidator => validator_counts.deactivating += 1,
                StakeStatus::DeactivatingAll => validator_counts.deactivating_all += 1,
            }
        });

    let non_zero_score_count = (0..state.num_pool_validators)
        .filter_map(|i| state.scores.get(i as usize))
        .filter(|&&score| score != 0)
        .count() as u32;

    DefaultStateOutput {
        accounts: AccountAddresses {
            config: steward_config.to_string(),
            state: steward_state.to_string(),
        },
        state: StateInfo {
            state_tag: format!("{}", state.state_tag),
            progress: StateProgress {
                completed: state.progress.count(),
                total: state.num_pool_validators,
                remaining: state.num_pool_validators - state.progress.count() as u64,
            },
            validator_lamport_balances_count: state.validator_lamport_balances.len(),
            scores_count: state.scores.len(),
            sorted_score_indices_count: state.sorted_score_indices.len(),
            yield_scores_count: state.yield_scores.len(),
            sorted_yield_score_indices_count: state.sorted_yield_score_indices.len(),
            delegations_count: state.delegations.len(),
            instant_unstake_count: state.instant_unstake.count(),
            start_computing_scores_slot: state.start_computing_scores_slot,
            current_epoch: state.current_epoch,
            next_cycle_epoch: state.next_cycle_epoch,
            num_pool_validators: state.num_pool_validators,
            scoring_unstake_total: state.scoring_unstake_total,
            instant_unstake_total: state.instant_unstake_total,
            stake_deposit_unstake_total: state.stake_deposit_unstake_total,
            validators_to_remove_count: state.validators_to_remove.count(),
            validators_for_immediate_removal_count: state.validators_for_immediate_removal.count(),
            validators_added: state.validators_added,
            non_zero_scores: non_zero_score_count,
        },
        lamports: LamportSummary {
            total_staked: LamportBalance::new(total_staked_lamports),
            total_transient: LamportBalance::new(total_transient_lamports),
            reserve: LamportBalance::new(reserve_stake_account.lamports),
        },
        validator_counts,
        state_strings: StateStrings {
            detailed: format_steward_state_string(&state_account.state),
            simple: format_simple_steward_state_string(&state_account.state),
        },
    }
}

fn _print_default_state(
    steward_config: &Pubkey,
    steward_state: &Pubkey,
    state_account: &StewardStateAccount,
    validator_list_account: &ValidatorList,
    reserve_stake_account: &Account,
    print_json: bool,
) {
    if print_json {
        let output = build_default_state_output(
            steward_config,
            steward_state,
            state_account,
            validator_list_account,
            reserve_stake_account,
        );
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        // Original formatted string output
        let state = &state_account.state;
        let output = build_default_state_output(
            steward_config,
            steward_state,
            state_account,
            validator_list_account,
            reserve_stake_account,
        );

        let mut formatted_string = String::new();

        formatted_string += "------- State -------\n";
        formatted_string += "📚 Accounts 📚\n";
        formatted_string += &format!("Config:      {}\n", output.accounts.config);
        formatted_string += &format!("State:       {}\n", output.accounts.state);
        formatted_string += "\n";
        formatted_string += "↺ State ↺\n";
        formatted_string += &format!("State Tag: {}\n", output.state.state_tag);
        formatted_string += &format!(
            "Progress: {} / {} ({} remaining)\n",
            output.state.progress.completed,
            output.state.progress.total,
            output.state.progress.remaining
        );
        formatted_string += &format!(
            "Validator Lamport Balances Count: {}\n",
            output.state.validator_lamport_balances_count
        );
        formatted_string += &format!("Scores Count: {}\n", output.state.scores_count);
        formatted_string += &format!(
            "Sorted Score Indices Count: {}\n",
            output.state.sorted_score_indices_count
        );
        formatted_string += &format!("Yield Scores Count: {}\n", output.state.yield_scores_count);
        formatted_string += &format!(
            "Sorted Yield Score Indices Count: {}\n",
            output.state.sorted_yield_score_indices_count
        );
        formatted_string += &format!("Delegations Count: {}\n", output.state.delegations_count);
        formatted_string += &format!("Instant Unstake: {}\n", output.state.instant_unstake_count);
        formatted_string += &format!(
            "Start Computing Scores Slot: {}\n",
            output.state.start_computing_scores_slot
        );
        formatted_string += &format!("Current Epoch: {}\n", output.state.current_epoch);
        formatted_string += &format!("Next Cycle Epoch: {}\n", output.state.next_cycle_epoch);
        formatted_string += &format!(
            "Number of Pool Validators: {}\n",
            output.state.num_pool_validators
        );
        formatted_string += &format!(
            "Scoring Unstake Total: {}\n",
            output.state.scoring_unstake_total
        );
        formatted_string += &format!(
            "Instant Unstake Total: {}\n",
            output.state.instant_unstake_total
        );
        formatted_string += &format!(
            "Stake Deposit Unstake Total: {}\n",
            output.state.stake_deposit_unstake_total
        );
        formatted_string += &format!("Padding0 Length: {}\n", state._padding0.len());
        formatted_string += "\n";
        formatted_string += &format!(
            "num_pool_validators: {}\n",
            output.state.num_pool_validators
        );
        formatted_string += &format!(
            "validator list length: {}\n",
            validator_list_account.validators.len()
        );
        formatted_string += &format!(
            "Validators marked to remove: {}\n",
            output.state.validators_to_remove_count
        );
        formatted_string += &format!(
            "Validators marked to remove immediately: {}\n",
            output.state.validators_for_immediate_removal_count
        );
        formatted_string += &format!("Validators added: {}\n", output.state.validators_added);
        formatted_string += "\n";
        formatted_string += &format!(
            "Total Staked Lamports: {} ({:.2} ◎)\n",
            output.lamports.total_staked.lamports, output.lamports.total_staked.sol
        );
        formatted_string += &format!(
            "Total Transient Lamports: {} ({:.2} ◎)\n",
            output.lamports.total_transient.lamports, output.lamports.total_transient.sol
        );
        formatted_string += &format!(
            "Reserve Lamports: {} ({:.2} ◎)\n",
            output.lamports.reserve.lamports, output.lamports.reserve.sol
        );
        formatted_string += "\n";
        formatted_string += &format!("🟩 Active Validators: {}\n", output.validator_counts.active);
        formatted_string += &format!(
            "🟨 Deactivating Transient Validators : {}\n",
            output.validator_counts.deactivating_transient
        );
        formatted_string += &format!(
            "🟨 Deactivating All Validators: {}\n",
            output.validator_counts.deactivating_all
        );
        formatted_string += &format!(
            "🟥 Deactivating Validators: {}\n",
            output.validator_counts.deactivating
        );
        formatted_string += &format!(
            "🟥 Ready for Removal Validators: {}\n",
            output.validator_counts.ready_for_removal
        );
        formatted_string += "\n";
        formatted_string += &format!("Non Zero Scores: {}\n", output.state.non_zero_scores);
        formatted_string += "\n";
        formatted_string += &format!("State: {}\n", output.state_strings.detailed);
        formatted_string += &format!("State: {}\n", output.state_strings.simple);
        formatted_string += "\n";
        formatted_string += "---------------------";

        println!("{}", formatted_string)
    }
}

fn compute_overall_ranks(steward_state_account: &StewardStateAccount) -> Vec<usize> {
    // For all validators from index 0 to num_pool_validators, we want to determine an overall rank with primary key of score, and secondary key of yield score, both descending.
    // The final vector created will be a vector of length num_pool_validators, with the index being the rank, and the value being the index of the validator in the steward list.

    let state = &steward_state_account.state;
    let num_pool_validators = state.num_pool_validators as usize;

    // (index, score, yield_score)
    let mut sorted_validator_indices: Vec<(usize, u32, u32)> = (0..num_pool_validators)
        .map(|i| (i, state.scores[i], state.yield_scores[i]))
        .collect();

    // Sorts based on score (descending) and yield_score (descending)
    sorted_validator_indices.sort_by(|a, b| {
        b.1.cmp(&a.1) // Compare scores (descending)
            .then_with(|| b.2.cmp(&a.2)) // If scores are equal, compare yield_scores (descending)
    });

    // final ranking vector
    let mut ranks: Vec<usize> = vec![0; num_pool_validators];
    for (rank, (index, _, _)) in sorted_validator_indices.into_iter().enumerate() {
        ranks[index] = rank;
    }

    ranks
}

fn _print_verbose_state(
    steward_state_account: &StewardStateAccount,
    config_account: &Config,
    validator_list_account: &ValidatorList,
    validator_histories: &HashMap<Pubkey, Option<Account>>,
    maybe_vote_account: Option<Pubkey>,
    print_json: bool,
) {
    if print_json {
    } else {
        let mut top_scores: Vec<(Pubkey, u32)> = vec![];

        let overall_ranks = compute_overall_ranks(steward_state_account);

        for (index, validator) in validator_list_account.validators.iter().enumerate() {
            let mut formatted_string = String::new();

            let history_info = validator_histories
                .get(&validator.vote_account_address)
                .and_then(|account| account.as_ref())
                .and_then(|account| {
                    ValidatorHistory::try_deserialize(&mut account.data.as_slice()).ok()
                });

            if let Some(vote_account) = maybe_vote_account {
                if vote_account != validator.vote_account_address {
                    continue;
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

            // let score_index = steward_state_account
            //     .state
            //     .sorted_score_indices
            //     .iter()
            //     .position(|&i| i == index as u16);
            // let yield_score_index = steward_state_account
            //     .state
            //     .sorted_yield_score_indices
            //     .iter()
            //     .position(|&i| i == index as u16);

            let score = steward_state_account.state.scores.get(index);

            let eligibility_criteria = match score {
                Some(0) => "No",
                Some(_) => "Yes",
                None => "N/A",
            };

            formatted_string += &format!("Vote Account: {:?}\n", vote_account);
            formatted_string += &format!("Stake Account: {:?}\n", stake_address);
            formatted_string +=
                &format!("Transient Stake Account: {:?}\n", transient_stake_address);
            formatted_string += &format!("Steward List Index: {}\n", index);

            let overall_rank_str = match overall_ranks.get(index) {
                Some(rank) => (rank + 1).to_string(),
                None => "N/A".into(),
            };

            formatted_string += &format!("Overall Rank: {}\n", overall_rank_str);
            formatted_string += &format!("Score: {}\n", score.unwrap_or(&0));
            formatted_string += &format!(
                "Yield Score: {}\n",
                steward_state_account
                    .state
                    .yield_scores
                    .get(index)
                    .unwrap_or(&0)
            );
            formatted_string +=
                &format!("Passing Eligibility Criteria: {}\n", eligibility_criteria);

            formatted_string += &format!(
                "Target Delegation Percent: {:.1}%\n",
                steward_state_account
                    .state
                    .delegations
                    .get(index)
                    .unwrap_or(&Delegation::default())
                    .numerator as f64
                    / steward_state_account
                        .state
                        .delegations
                        .get(index)
                        .unwrap_or(&Delegation::default())
                        .denominator as f64
                    * 100.0
            );

            formatted_string += "\n";

            formatted_string += &format!(
                "Is Instant Unstake: {}\n",
                steward_state_account
                    .state
                    .instant_unstake
                    .get(index)
                    .unwrap_or_default()
            );

            if let Some(history_info) = history_info {
                formatted_string += &format!(
                    "Is blacklisted: {}\n",
                    config_account
                        .validator_history_blacklist
                        .get_unsafe(history_info.index as usize)
                );
                formatted_string += &format!("\nValidator History Index: {}\n", history_info.index);
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
            formatted_string += &format!(
                "Steward Internal Lamports: {}\n",
                match steward_state_account
                    .state
                    .validator_lamport_balances
                    .get(index)
                {
                    Some(&LAMPORT_BALANCE_DEFAULT) | None => "Unset".to_string(),
                    Some(&lamports) => lamports.to_string(),
                }
            );
            let status = match StakeStatus::try_from(validator.status).unwrap() {
                StakeStatus::Active => "🟩 Active",
                StakeStatus::DeactivatingAll => "🟨 Deactivating All",
                StakeStatus::DeactivatingTransient => "🟨 Deactivating Transient",
                StakeStatus::DeactivatingValidator => "🟥 Deactivating Validator",
                StakeStatus::ReadyForRemoval => "🟥 Ready for Removal",
            };
            formatted_string += &format!("Status: {}\n", status);
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

            formatted_string += "\n";

            if let Some(score) = steward_state_account.state.scores.get(index) {
                if *score != 0 {
                    top_scores.push((vote_account, *score));
                }
            }

            println!("{}", formatted_string);
        }

        if maybe_vote_account.is_none() {
            println!("\nAll Ranked Validators ( {} ): \n", top_scores.len());
            println!("{:<45} : Score\n", "Vote Account");

            top_scores.sort_by(|a, b| b.1.cmp(&a.1));
            top_scores.iter().for_each(|(vote_account, score)| {
                let formatted_score = format!("{}", score).chars().rev().enumerate().fold(
                    String::new(),
                    |acc, (i, c)| {
                        if i > 0 && i % 3 == 0 {
                            format!("{}_{}", c, acc)
                        } else {
                            format!("{}{}", c, acc)
                        }
                    },
                );
                let vote_account = format!("{:?}", vote_account);
                println!("{:<45} : {}", vote_account, formatted_score);
            });
        }
    }
}
