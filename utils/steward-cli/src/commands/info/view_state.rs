use std::{collections::HashMap, sync::Arc};

use anchor_lang::AccountDeserialize;
use anyhow::Result;
use jito_steward::{
    constants::LAMPORT_BALANCE_DEFAULT, stake_pool_utils::ValidatorList, Config, Delegation,
    StewardStateAccountV2,
};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{account::Account, native_token::lamports_to_sol, pubkey::Pubkey};
use spl_stake_pool::{
    find_stake_program_address, find_transient_stake_program_address, state::StakeStatus,
};
use stakenet_sdk::utils::{
    accounts::{get_all_steward_accounts, get_validator_history_address},
    debug::{format_simple_steward_state_string, format_steward_state_string},
};
use validator_history::ValidatorHistory;

use crate::commands::command_args::ViewState;

/// Represents a balance in both lamports and SOL
#[derive(Serialize, Deserialize, Debug)]
struct LamportBalance {
    /// Lamport amount
    lamports: u64,

    /// SOL amount
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

/// Publlic keys for jito-steward program accounts
#[derive(Serialize, Deserialize, Debug)]
struct AccountAddresses {
    /// Steward configuratioin account public key
    config: String,

    /// Steward state account public key
    state: String,
}

/// Tracks progress through validator processing cycles
#[derive(Serialize, Deserialize, Debug)]
struct StateProgress {
    /// Completed
    completed: usize,

    /// Total
    total: u64,

    /// Remaining
    remaining: u64,
}

/// Steward's current state and configuration
#[derive(Serialize, Deserialize, Debug)]
pub struct StateInfo {
    /// Current state of the steward state machine
    ///
    /// Possible values:
    /// - ComputeScores
    /// - ComputeDelegations
    /// - Idle
    /// - ComputeInstantUnstake
    /// - Rebalance
    state_tag: String,

    /// State progress
    progress: StateProgress,

    /// Count of validator lamport balance entries tracked internally
    validator_lamport_balances_count: usize,

    /// Count of computed validator performance scores
    /// Overall scores used to determine delegates and delegation order
    scores_count: usize,

    /// Count of validator indices sorted by score (descending)
    /// Used for efficient ranking and delegation decisions
    sorted_score_indices_count: usize,

    /// Count of computed raw scores
    raw_scores_count: usize,

    /// Count of validator indices sorted by raw score (descending)
    sorted_raw_score_indices_count: usize,

    /// Count of delegation entries (target stake allocations)
    /// Each entry represents target share of pool as a proportion
    delegations_count: usize,

    /// Count of validators marked for instant unstaking
    /// Each bit in the BitMask represents a validator flagged for immediate unstake
    instant_unstake_count: usize,

    /// Slot number when the first ComputeScores instruction was called
    /// Marks the beginning of the current scoring cycle
    start_computing_scores_slot: u64,

    /// Internal current epoch used for tracking epoch changes
    /// Updated when steward detects epoch transitions
    current_epoch: u64,

    /// Epoch when the next steward cycle will begin
    /// Determines when to start new scoring and delegation cycle
    next_cycle_epoch: u64,

    /// Total number of validators in the stake pool
    /// Updated at cycle start and when validators are removed
    /// Used to determine how many validators to score
    num_pool_validators: u64,

    /// Total lamports scheduled for unstaking due to scoring decisions
    /// Accumulated during the current cycle
    scoring_unstake_total: u64,

    /// Total lamports scheduled for instant unstaking
    /// Accumulated during the current cycle
    instant_unstake_total: u64,

    /// Total lamports from stake deposits scheduled for unstaking
    /// Tracks deposits that need to be withdrawn
    stake_deposit_unstake_total: u64,

    /// Count of validators marked for removal from the pool
    /// Cleaned up in the next epoch after removal
    validators_to_remove_count: usize,

    /// Count of validators marked for immediate removal
    /// Applied when validator can be removed within the same epoch
    validators_for_immediate_removal_count: usize,

    /// Number of validators added to the pool in the current cycle
    /// Used to track pool growth
    validators_added: u16,

    /// Count of validators with non-zero performance scores
    /// Indicates how many validators are currently eligible for delegation
    non_zero_scores: u32,
}

/// Summary of all lamport balances in the stake pool
#[derive(Serialize, Deserialize, Debug)]
pub struct LamportSummary {
    /// Total lamports actively staked across all validators
    total_staked: LamportBalance,

    /// Total lamports in transient stake accounts (pending activation/deactivation)
    total_transient: LamportBalance,

    /// Lamports held in the reserve stake account
    reserve: LamportBalance,
}

/// Count of validators by their current stake status
#[derive(Serialize, Deserialize, Debug)]
pub struct ValidatorCounts {
    /// Validators with active stake
    active: u32,

    /// Validators currently deactivating transient stake
    deactivating_transient: u32,

    /// Validators deactivating all stake (both active and transient)
    deactivating_all: u32,

    /// Validators with stake being deactivated
    deactivating: u32,

    /// Validators ready to be removed from the pool
    ready_for_removal: u32,
}

/// Human-readable string representations of the steward state
#[derive(Serialize, Deserialize, Debug)]
pub struct StateStrings {
    /// Detailed state description with additional context
    detailed: String,

    /// Simple, concise state description
    simple: String,
}

/// Complete output for the default (non-verbose) state view
///
/// This provides a high-level overview of the steward's current state,
/// including account information, processing progress, and stake distribution.
#[derive(Serialize, Deserialize, Debug)]
pub struct DefaultStateOutput {
    /// Public keys for steward accounts
    accounts: AccountAddresses,

    /// Detailed state information and progress
    state: StateInfo,

    /// Summary of stake balances
    lamports: LamportSummary,

    /// Count of validators by status
    validator_counts: ValidatorCounts,

    /// Human-readable state descriptions
    state_strings: StateStrings,
}

/// Public keys for validator-related accounts
#[derive(Serialize, Deserialize, Debug)]
pub struct ValidatorAddresses {
    /// Validator's vote account public key
    vote_account: String,

    /// Validator's stake account public key in the pool
    stake_account: String,

    /// Validator's transient stake account public key
    transient_stake_account: String,
}

/// Information from the validator_history program
#[derive(Serialize, Deserialize, Debug)]
pub struct ValidatorHistoryOutput {
    /// Index of this validator in the validator history program
    index: u32,

    /// Whether this validator is blacklisted based on historical performance
    is_blacklisted: bool,
}

/// Lamport balances for a specific validator across different account types
#[derive(Serialize, Deserialize, Debug)]
pub struct ValidatorLamports {
    /// Lamports in the validator's active stake account
    active: LamportBalance,

    /// Lamports in the validator's transient stake account
    transient: LamportBalance,

    /// Internal lamport balance tracked by steward (None if unset)
    steward_internal: Option<u64>,
}

/// Current status of a validator's stake in the pool
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorStatus {
    /// Validator has active stake and is participating normally
    Active,

    /// Validator is deactivating all stake (active + transient)
    DeactivatingAll,

    /// Validator is deactivating only transient stake
    DeactivatingTransient,

    /// Validator's active stake is being deactivated
    DeactivatingValidator,

    /// Validator is ready to be completely removed from the pool
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

/// Comprehensive details about a single validator in the stake pool
///
/// Contains all relevant information about a validator's performance,
/// stake allocation, status, and steward-specific data.
#[derive(Serialize, Deserialize, Debug)]
pub struct ValidatorDetails {
    /// Public keys for validator-related accounts
    pub addresses: ValidatorAddresses,

    /// Position of this validator in the steward's validator list
    pub steward_list_index: usize,

    /// Overall rank among all validators (1-based, None if unranked)
    pub overall_rank: Option<usize>,

    /// Validator's final score (0 if any eligibility criteria failed, otherwise equals raw_score)
    pub score: u64,

    /// Validator's 4-tier hierarchical score before binary filters applied
    pub raw_score: u64,

    /// Whether validator meets eligibility criteria ("Yes", "No", or "N/A")
    pub passing_eligibility_criteria: String,

    /// Target percentage of total pool stake to delegate to this validator
    pub target_delegation_percent: f64,

    /// Whether this validator is marked for instant unstaking
    pub is_instant_unstake: bool,

    /// Historical performance data (None if not available)
    pub validator_history_output: Option<ValidatorHistoryOutput>,

    /// Lamport balances across different account types
    pub lamports: ValidatorLamports,

    /// Current stake status in the pool
    pub status: ValidatorStatus,

    /// Whether validator is marked for removal in next cycle
    pub marked_for_removal: bool,

    /// Whether validator is marked for immediate removal
    pub marked_for_immediate_removal: bool,
}

/// A validator with its performance score for ranking purposes
#[derive(Serialize, Deserialize, Debug)]
pub struct RankedValidator {
    /// Validator's vote account public key
    pub vote_account: String,

    /// Performance score assigned by the steward
    pub score: u64,
}

/// Summary of all validators with non-zero scores, sorted by performance
#[derive(Serialize, Deserialize, Debug)]
pub struct RankedValidatorsSummary {
    /// Total number of validators with non-zero scores
    pub count: usize,

    /// List of validators sorted by score (highest first)
    pub validators: Vec<RankedValidator>,
}

/// Complete output for the verbose state view
///
/// Provides detailed information about individual validators,
/// including their performance metrics, stake allocations, and rankings.
/// Optionally includes a ranked summary when viewing all validators.
#[derive(Serialize, Deserialize, Debug)]
pub struct VerboseStateOutput {
    /// Detailed information for each validator
    pub validators: Vec<ValidatorDetails>,

    /// Ranked summary of all validators (None when viewing a specific validator)
    pub ranked_validators: Option<RankedValidatorsSummary>,
}

/// View steward state information
///
/// Fetches steward accounts and display either a summary view or detailed validator information
/// based on the provided arguments.
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

/// Constructs a structured representation of the steward's default state
///
/// Processes raw steward state data into a well-structured output format
/// that can be easily serialized to JSON or displayed as formatted text.
/// This function aggregates validator information and computes summary statistics.
fn build_default_state_output(
    steward_config: &Pubkey,
    steward_state: &Pubkey,
    state_account: &StewardStateAccountV2,
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
            raw_scores_count: state.raw_scores.len(),
            sorted_raw_score_indices_count: state.sorted_raw_score_indices.len(),
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

/// Display the information of [`DefaultStateOutput`]
fn _print_default_state(
    steward_config: &Pubkey,
    steward_state: &Pubkey,
    state_account: &StewardStateAccountV2,
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
        formatted_string += &format!("Raw Scores Count: {}\n", output.state.raw_scores_count);
        formatted_string += &format!(
            "Sorted Raw Score Indices Count: {}\n",
            output.state.sorted_raw_score_indices_count
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

/// Computes overall ranking of validators based on performance metrics
fn compute_overall_ranks(steward_state_account: &StewardStateAccountV2) -> Vec<usize> {
    let state = &steward_state_account.state;
    let num_pool_validators = state.num_pool_validators as usize;

    // (index, score, raw_score)
    let mut sorted_validator_indices: Vec<(usize, u64, u64)> = (0..num_pool_validators)
        .map(|i| (i, state.scores[i], state.raw_scores[i]))
        .collect();

    // Sorts based on score (descending) and raw_score (descending)
    sorted_validator_indices.sort_by(|a, b| {
        b.1.cmp(&a.1) // Compare scores (descending)
            .then_with(|| b.2.cmp(&a.2)) // If scores are equal, compare raw_scores (descending)
    });

    // final ranking vector
    let mut ranks: Vec<usize> = vec![0; num_pool_validators];
    for (rank, (index, _, _)) in sorted_validator_indices.into_iter().enumerate() {
        ranks[index] = rank;
    }

    ranks
}

/// Constructs detailed validator information for verbose output
///
/// Processes individual validator data, historical performance, and steward-specific
/// metrics to create comprehensive validator details. Handles filtering for specific
/// vote accounts and builds ranked summaries when appropriate.
fn build_verbose_state_output(
    steward_state_account: &StewardStateAccountV2,
    config_account: &Config,
    validator_list_account: &ValidatorList,
    validator_histories: &HashMap<Pubkey, Option<Account>>,
    maybe_vote_account: Option<Pubkey>,
) -> VerboseStateOutput {
    let overall_ranks = compute_overall_ranks(steward_state_account);
    let mut validators = Vec::new();
    let mut top_scores = Vec::new();

    for (index, validator) in validator_list_account.validators.iter().enumerate() {
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

        let score = steward_state_account.state.scores.get(index).unwrap_or(&0);
        let raw_score = steward_state_account
            .state
            .raw_scores
            .get(index)
            .unwrap_or(&0);

        let eligibility_criteria = match score {
            0 => "No".to_string(),
            _ => "Yes".to_string(),
        };

        let overall_rank = overall_ranks.get(index).map(|r| r + 1);

        let delegation_default = Delegation::default();
        let delegation = steward_state_account
            .state
            .delegations
            .get(index)
            .unwrap_or(&delegation_default);

        let target_delegation_percent = if delegation.denominator != 0 {
            delegation.numerator as f64 / delegation.denominator as f64 * 100.0
        } else {
            0.0
        };

        let steward_internal_lamports = match steward_state_account
            .state
            .validator_lamport_balances
            .get(index)
        {
            Some(&LAMPORT_BALANCE_DEFAULT) | None => None,
            Some(&lamports) => Some(lamports),
        };

        let validator_details = ValidatorDetails {
            addresses: ValidatorAddresses {
                vote_account: vote_account.to_string(),
                stake_account: stake_address.to_string(),
                transient_stake_account: transient_stake_address.to_string(),
            },
            steward_list_index: index,
            overall_rank,
            score: *score,
            raw_score: *raw_score,
            passing_eligibility_criteria: eligibility_criteria,
            target_delegation_percent,
            is_instant_unstake: steward_state_account
                .state
                .instant_unstake
                .get(index)
                .unwrap_or_default(),
            validator_history_output: history_info.as_ref().map(|info| ValidatorHistoryOutput {
                index: info.index,
                is_blacklisted: config_account
                    .validator_history_blacklist
                    .get_unsafe(info.index as usize),
            }),
            lamports: ValidatorLamports {
                active: LamportBalance::new(u64::from(validator.active_stake_lamports)),
                transient: LamportBalance::new(u64::from(validator.transient_stake_lamports)),
                steward_internal: steward_internal_lamports,
            },
            status: StakeStatus::try_from(validator.status).unwrap().into(),
            marked_for_removal: steward_state_account
                .state
                .validators_to_remove
                .get(index)
                .unwrap_or_default(),
            marked_for_immediate_removal: steward_state_account
                .state
                .validators_for_immediate_removal
                .get(index)
                .unwrap_or_default(),
        };

        validators.push(validator_details);

        if *score != 0 {
            top_scores.push(RankedValidator {
                vote_account: vote_account.to_string(),
                score: *score,
            });
        }
    }

    // Sort top scores by score (descending)
    top_scores.sort_by(|a, b| b.score.cmp(&a.score));

    let ranked_validators = if maybe_vote_account.is_none() {
        Some(RankedValidatorsSummary {
            count: top_scores.len(),
            validators: top_scores,
        })
    } else {
        None
    };

    VerboseStateOutput {
        validators,
        ranked_validators,
    }
}

/// Display the information of [`VerboseStateOutput`]
fn _print_verbose_state(
    steward_state_account: &StewardStateAccountV2,
    config_account: &Config,
    validator_list_account: &ValidatorList,
    validator_histories: &HashMap<Pubkey, Option<Account>>,
    maybe_vote_account: Option<Pubkey>,
    print_json: bool,
) {
    if print_json {
        let output = build_verbose_state_output(
            steward_state_account,
            config_account,
            validator_list_account,
            validator_histories,
            maybe_vote_account,
        );
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        let mut top_scores: Vec<(Pubkey, u64)> = vec![];
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
                "Raw Score: {}\n",
                steward_state_account
                    .state
                    .raw_scores
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
