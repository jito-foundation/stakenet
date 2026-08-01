use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anchor_lang::AccountDeserialize;
use anyhow::{anyhow, Result};
use jito_steward::{
    constants::TVC_ACTIVATION_EPOCH, score::validator_score, select_validators_to_delegate, Config,
};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{account::Account, native_token::lamports_to_sol, pubkey::Pubkey};
use stakenet_sdk::utils::accounts::{
    get_all_steward_accounts, get_cluster_history_address, get_validator_history_address,
};
use validator_history::{ClusterHistory, ValidatorHistory};

use crate::commands::command_args::DryRun;

/// Order matters only for display; names mirror the binary filters in `ScoreComponentsV5`.
const FILTER_NAMES: [&str; 10] = [
    "mev_commission",
    "commission",
    "historical_commission",
    "blacklist",
    "superminority",
    "delinquency",
    "running_bam",
    "merkle_root_upload_authority",
    "priority_fee_commission",
    "priority_fee_merkle_root_upload_authority",
];

/// Result of a single binary eligibility filter.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct FilterResult {
    name: String,
    passed: bool,
}

/// A single validator's predicted outcome for the scoring cycle.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct ValidatorPrediction {
    /// 1-based rank by (score desc, raw_score desc)
    rank: usize,

    /// Index in the pool validator list
    list_index: usize,

    vote_account: String,

    /// Final score (0 if any binary filter fails)
    score: u64,

    /// 4-tier encoded score before binary filters
    raw_score: u64,

    /// Will this validator receive a delegation this cycle (top-N by score)?
    selected: bool,

    /// Predicted target share of the pool (%), 0 if not selected
    target_percent: f64,

    /// Predicted target stake (SOL), 0 if not selected
    target_sol: f64,

    /// Current active stake (SOL)
    current_sol: f64,

    /// target_sol - current_sol (positive => stake should flow in, capped per cycle)
    delta_sol: f64,

    /// Is this validator currently a delegation target on-chain?
    currently_delegated: bool,

    /// Max inflation commission used in scoring (0-100)
    commission_max: u8,

    /// Average MEV commission used in scoring (bps)
    mev_commission_avg_bps: u16,

    /// Epochs with non-zero vote credits
    validator_age: u32,

    /// Scaled average vote-credit ratio
    vote_credits_avg: u32,

    /// Ratio that (if below threshold) trips the delinquency filter
    delinquency_ratio: f64,

    /// Binary filters that FAILED (empty => passes every eligibility check)
    failing_filters: Vec<String>,

    /// Full per-filter breakdown
    filters: Vec<FilterResult>,

    /// Set when the validator could not be scored (e.g. missing validator history)
    note: Option<String>,
}

/// Timing context: when the predicted selection actually takes effect.
#[derive(Serialize, Deserialize, Debug)]
struct CycleTiming {
    /// Current on-chain epoch
    current_epoch: u64,

    /// Epoch the scores were computed as-of
    scoring_epoch: u64,

    /// Epoch at which the steward next recomputes scores + delegations
    next_cycle_epoch: u64,

    /// next_cycle_epoch - current_epoch
    epochs_until_next_cycle: u64,

    /// How many epochs between scoring cycles
    num_epochs_between_scoring: u64,

    /// Fraction into an epoch when ComputeScores runs
    compute_score_epoch_progress: f64,

    /// True when scoring_epoch is in the future (results are not reliable)
    is_projection: bool,
}

/// Diff between the predicted delegation set and the current on-chain one.
#[derive(Serialize, Deserialize, Debug)]
struct DelegationDiff {
    /// Predicted to receive stake but not currently delegated
    newly_added: Vec<String>,

    /// Currently delegated but predicted to be dropped
    dropped: Vec<String>,

    /// In both the predicted and current sets
    unchanged_count: usize,
}

/// Complete dry-run output (serialized directly with `--print-json`).
#[derive(Serialize, Deserialize, Debug)]
struct DryRunOutput {
    timing: CycleTiming,

    /// Configured cap on how many validators receive stake
    num_delegation_validators: usize,

    /// How many validators are actually selected this cycle (<= cap)
    num_selected: usize,

    /// Lowest score that still makes the delegation set this cycle
    cutoff_score: u64,

    /// Validators successfully scored (had a history account + no score error)
    num_scored: usize,

    /// Validators with a non-zero score (pass every eligibility filter)
    num_passing: usize,

    /// Total pool value used to size targets (SOL)
    pool_total_sol: f64,

    /// Equal target per selected validator (SOL)
    target_sol_each: f64,

    /// Whether cluster history is updated through the current epoch
    cluster_history_fresh: bool,

    diff: DelegationDiff,

    validators: Vec<ValidatorPrediction>,
}

/// Per-validator scratch data collected before ranking/selection.
struct Scratch {
    vote: Pubkey,
    current_lamports: u64,
    currently_delegated: bool,
    commission_max: u8,
    mev_commission_avg_bps: u16,
    validator_age: u32,
    vote_credits_avg: u32,
    delinquency_ratio: f64,
    filters: Vec<FilterResult>,
    failing_filters: Vec<String>,
    note: Option<String>,
}

/// Dry-run the scoring cycle off-chain and report which validators would receive stake.
///
/// Re-runs the on-chain `validator_score` and `select_validators_to_delegate` against live
/// account data, so the "which validators get stake" result is exact for the current cycle.
/// It sends no transactions and mutates no on-chain state.
pub async fn command_dry_run(
    args: DryRun,
    client: &Arc<RpcClient>,
    steward_program_id: Pubkey,
    validator_history_program_id: Pubkey,
) -> Result<()> {
    let print_json = args.view_parameters.print_json;
    if !print_json {
        println!(
            "Fetching accounts (validator histories + cluster history) — a custom RPC is recommended"
        );
    }

    let steward_config = args.view_parameters.steward_config;
    let all = get_all_steward_accounts(client, &steward_program_id, &steward_config).await?;
    let config: &Config = &all.config_account;
    let state = &all.state_account.state;

    // Cluster history is needed to normalize vote credits during scoring.
    let cluster_history_address = get_cluster_history_address(&validator_history_program_id);
    let cluster_history_account = client.get_account(&cluster_history_address).await?;
    let cluster_history =
        ClusterHistory::try_deserialize(&mut cluster_history_account.data.as_slice())
            .map_err(|e| anyhow!("Failed to deserialize cluster history: {e}"))?;

    // Which epoch to score as-of.
    let epoch_info = client.get_epoch_info().await?;
    let current_epoch = epoch_info.epoch;
    let scoring_epoch = args.epoch.unwrap_or(current_epoch);
    let is_projection = scoring_epoch > current_epoch;
    if is_projection && !print_json {
        println!(
            "⚠️  --epoch {scoring_epoch} is in the future (current epoch {current_epoch}). \
             Future validator-history data does not exist yet, so scores below are NOT a reliable \
             projection."
        );
    }

    // Freshness of cluster history relative to the current epoch (on-chain scoring requires this).
    let epoch_schedule = client.get_epoch_schedule().await?;
    let first_slot_current_epoch = epoch_schedule.get_first_slot_in_epoch(current_epoch);
    let cluster_history_fresh =
        cluster_history.cluster_history_last_update_slot >= first_slot_current_epoch;
    if !cluster_history_fresh && !print_json {
        println!(
            "⚠️  Cluster history is not yet updated this epoch — current-epoch inputs may be \
             incomplete (check `validator-history-cli cluster-history-status`)."
        );
    }

    // Fetch every validator-history account, keyed by vote account.
    let validators = &all.validator_list_account.validators;
    let vote_accounts: Vec<Pubkey> = validators.iter().map(|v| v.vote_account_address).collect();
    let history_addresses: Vec<Pubkey> = vote_accounts
        .iter()
        .map(|va| get_validator_history_address(va, &validator_history_program_id))
        .collect();
    let mut raw_history_accounts: Vec<Option<Account>> = Vec::with_capacity(history_addresses.len());
    for chunk in history_addresses.chunks(100) {
        raw_history_accounts.extend(client.get_multiple_accounts(chunk).await?);
    }
    let history_map: HashMap<Pubkey, Option<Account>> = vote_accounts
        .iter()
        .copied()
        .zip(raw_history_accounts)
        .collect();

    let num_delegation_validators = config.parameters.num_delegation_validators as usize;
    let n = validators.len();
    let pool_total_lamports = all.stake_pool_account.total_lamports;

    // Score every validator off-chain, exactly as the on-chain instruction would.
    let mut scores: Vec<u64> = vec![0; n];
    let mut raw_scores: Vec<u64> = vec![0; n];
    let mut scratch: Vec<Scratch> = Vec::with_capacity(n);

    for (i, validator) in validators.iter().enumerate() {
        let vote = validator.vote_account_address;
        let mut s = Scratch {
            vote,
            current_lamports: u64::from(validator.active_stake_lamports),
            currently_delegated: state.delegations.get(i).is_some_and(|d| d.numerator > 0),
            commission_max: 0,
            mev_commission_avg_bps: 0,
            validator_age: 0,
            vote_credits_avg: 0,
            delinquency_ratio: 0.0,
            filters: Vec::new(),
            failing_filters: Vec::new(),
            note: None,
        };

        let maybe_hist = history_map
            .get(&vote)
            .and_then(|a| a.as_ref())
            .and_then(|a| ValidatorHistory::try_deserialize(&mut a.data.as_slice()).ok());

        match maybe_hist {
            None => s.note = Some("no validator history account".to_string()),
            Some(hist) => match validator_score(
                &hist,
                &cluster_history,
                config,
                scoring_epoch as u16,
                TVC_ACTIVATION_EPOCH,
            ) {
                Err(e) => s.note = Some(format!("score error: {e}")),
                Ok(sc) => {
                    scores[i] = sc.score;
                    raw_scores[i] = sc.raw_score;
                    s.commission_max = sc.commission_max;
                    s.mev_commission_avg_bps = sc.mev_commission_avg;
                    s.validator_age = sc.validator_age;
                    s.vote_credits_avg = sc.vote_credits_avg;
                    s.delinquency_ratio = sc.details.delinquency_ratio;

                    let filter_vals = [
                        sc.mev_commission_score,
                        sc.commission_score,
                        sc.historical_commission_score,
                        sc.blacklisted_score,
                        sc.superminority_score,
                        sc.delinquency_score,
                        sc.running_bam_score,
                        sc.merkle_root_upload_authority_score,
                        sc.priority_fee_commission_score,
                        sc.priority_fee_merkle_root_upload_authority_score,
                    ];
                    for (name, val) in FILTER_NAMES.iter().zip(filter_vals) {
                        let passed = val == 1;
                        if !passed {
                            s.failing_filters.push((*name).to_string());
                        }
                        s.filters.push(FilterResult {
                            name: (*name).to_string(),
                            passed,
                        });
                    }
                }
            },
        }
        scratch.push(s);
    }

    // Rank by score desc, then raw_score desc, then index asc (stable, matches view-state).
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        scores[b]
            .cmp(&scores[a])
            .then_with(|| raw_scores[b].cmp(&raw_scores[a]))
            .then_with(|| a.cmp(&b))
    });
    let mut rank_of = vec![0usize; n];
    for (rank, &i) in order.iter().enumerate() {
        rank_of[i] = rank + 1;
    }

    // Selection via the real on-chain routine (top-N non-zero by score).
    let sorted_score_indices: Vec<u16> = order.iter().map(|&i| i as u16).collect();
    let selected = select_validators_to_delegate(
        &scores,
        &sorted_score_indices,
        num_delegation_validators,
    );
    let num_selected = selected.len();
    let selected_set: HashSet<u16> = selected.iter().copied().collect();
    let cutoff_score = selected
        .last()
        .map(|&i| scores[i as usize])
        .unwrap_or(0);

    let target_percent_each = if num_selected > 0 {
        100.0 / num_selected as f64
    } else {
        0.0
    };
    let target_lamports_each = if num_selected > 0 {
        pool_total_lamports / num_selected as u64
    } else {
        0
    };
    let target_sol_each = lamports_to_sol(target_lamports_each);

    // Build predictions + diff.
    let mut predictions: Vec<ValidatorPrediction> = Vec::with_capacity(n);
    let mut newly_added = Vec::new();
    let mut dropped = Vec::new();
    let mut unchanged_count = 0usize;

    for (i, s) in scratch.iter().enumerate() {
        let selected = selected_set.contains(&(i as u16));
        match (selected, s.currently_delegated) {
            (true, false) => newly_added.push(s.vote.to_string()),
            (false, true) => dropped.push(s.vote.to_string()),
            (true, true) => unchanged_count += 1,
            (false, false) => {}
        }

        let target_sol = if selected { target_sol_each } else { 0.0 };
        let current_sol = lamports_to_sol(s.current_lamports);
        predictions.push(ValidatorPrediction {
            rank: rank_of[i],
            list_index: i,
            vote_account: s.vote.to_string(),
            score: scores[i],
            raw_score: raw_scores[i],
            selected,
            target_percent: if selected { target_percent_each } else { 0.0 },
            target_sol,
            current_sol,
            delta_sol: target_sol - current_sol,
            currently_delegated: s.currently_delegated,
            commission_max: s.commission_max,
            mev_commission_avg_bps: s.mev_commission_avg_bps,
            validator_age: s.validator_age,
            vote_credits_avg: s.vote_credits_avg,
            delinquency_ratio: s.delinquency_ratio,
            failing_filters: s.failing_filters.clone(),
            filters: s.filters.clone(),
            note: s.note.clone(),
        });
    }
    predictions.sort_by_key(|p| p.rank);

    let num_scored = scratch.iter().filter(|s| s.note.is_none()).count();
    let num_passing = scores.iter().filter(|&&s| s > 0).count();

    let focus = args.vote_account;
    let output_validators: Vec<ValidatorPrediction> = match focus {
        Some(va) => {
            let va = va.to_string();
            predictions.iter().filter(|p| p.vote_account == va).cloned().collect()
        }
        None => predictions.clone(),
    };

    let output = DryRunOutput {
        timing: CycleTiming {
            current_epoch,
            scoring_epoch,
            next_cycle_epoch: state.next_cycle_epoch,
            epochs_until_next_cycle: state.next_cycle_epoch.saturating_sub(current_epoch),
            num_epochs_between_scoring: config.parameters.num_epochs_between_scoring,
            compute_score_epoch_progress: config.parameters.compute_score_epoch_progress,
            is_projection,
        },
        num_delegation_validators,
        num_selected,
        cutoff_score,
        num_scored,
        num_passing,
        pool_total_sol: lamports_to_sol(pool_total_lamports),
        target_sol_each,
        cluster_history_fresh,
        diff: DelegationDiff {
            newly_added,
            dropped,
            unchanged_count,
        },
        validators: output_validators,
    };

    if print_json {
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    match focus {
        Some(va) => print_focus(&output, va),
        None => print_table(&output, args.limit),
    }
    Ok(())
}

/// Group a large integer with underscores for readability (e.g. 12_345_678).
fn group(n: u64) -> String {
    n.to_string()
        .chars()
        .rev()
        .enumerate()
        .fold(String::new(), |acc, (i, c)| {
            if i > 0 && i % 3 == 0 {
                format!("{c}_{acc}")
            } else {
                format!("{c}{acc}")
            }
        })
}

/// Print the shared summary + timing header used by both views.
fn print_header(o: &DryRunOutput) {
    let t = &o.timing;
    println!("═══════════════════ Steward Dry-Run ═══════════════════");
    println!(
        "Scoring as-of epoch : {}{}",
        t.scoring_epoch,
        if t.is_projection {
            "  (PROJECTION — future data missing)"
        } else {
            ""
        }
    );
    println!("Current epoch       : {}", t.current_epoch);
    if t.epochs_until_next_cycle == 0 {
        println!(
            "Next scoring cycle  : epoch {} (this cycle — selection applies now)",
            t.next_cycle_epoch
        );
    } else {
        println!(
            "Next scoring cycle  : epoch {} ({} epoch(s) away; every {} epochs, at {:.0}% into the epoch)",
            t.next_cycle_epoch,
            t.epochs_until_next_cycle,
            t.num_epochs_between_scoring,
            t.compute_score_epoch_progress * 100.0
        );
    }
    println!(
        "Pool value          : {:.1} ◎   |   delegates: {} of max {}   |   ~{:.1} ◎ each",
        o.pool_total_sol, o.num_selected, o.num_delegation_validators, o.target_sol_each
    );
    println!(
        "Scored / passing    : {} scored, {} pass all filters",
        o.num_scored, o.num_passing
    );
    if !o.cluster_history_fresh {
        println!("Cluster history     : ⚠️  stale for current epoch");
    }
    println!(
        "Note: selection is exact for this cycle; actual stake ramps toward targets over epochs \
         (unstake caps + reserve)."
    );
    println!("───────────────────────────────────────────────────────");
}

/// Ranked table + delegation diff.
fn print_table(o: &DryRunOutput, limit: Option<usize>) {
    print_header(o);

    let limit = limit.unwrap_or(o.num_delegation_validators + 10);

    println!(
        "\n{:>4}  {:<6} {:<44} {:>20}  {:>8}  {:>10}  {:>10}  {}",
        "rank", "stake?", "vote_account", "score", "target%", "target◎", "delta◎", "flags"
    );
    for p in o.validators.iter().take(limit) {
        let marker = if p.selected { "★ yes" } else { "  no" };
        let flags = if let Some(note) = &p.note {
            note.clone()
        } else if !p.failing_filters.is_empty() {
            format!("✗ {}", p.failing_filters.join(","))
        } else if p.currently_delegated && !p.selected {
            "DROPPED".to_string()
        } else if p.selected && !p.currently_delegated {
            "NEW".to_string()
        } else {
            String::new()
        };
        println!(
            "{:>4}  {:<6} {:<44} {:>20}  {:>7.2}%  {:>10.1}  {:>+10.1}  {}",
            p.rank,
            marker,
            p.vote_account,
            group(p.score),
            p.target_percent,
            p.target_sol,
            p.delta_sol,
            flags
        );
    }
    if o.validators.len() > limit {
        println!("… {} more (use --limit to show more)", o.validators.len() - limit);
    }

    // Delegation diff — the "who changes" answer.
    println!("\n────────────── Change vs current delegations ──────────────");
    println!(
        "Unchanged: {}   |   Newly staked: {}   |   Dropped: {}",
        o.diff.unchanged_count,
        o.diff.newly_added.len(),
        o.diff.dropped.len()
    );
    if !o.diff.newly_added.is_empty() {
        println!("\n➕ Newly receiving stake ({}):", o.diff.newly_added.len());
        for v in &o.diff.newly_added {
            println!("   {v}");
        }
    }
    if !o.diff.dropped.is_empty() {
        println!("\n➖ Dropped from delegation ({}):", o.diff.dropped.len());
        for v in &o.diff.dropped {
            println!("   {v}");
        }
    }
}

/// Detailed single-validator breakdown + plain-English verdict.
fn print_focus(o: &DryRunOutput, vote_account: Pubkey) {
    print_header(o);

    let Some(p) = o.validators.first() else {
        println!("\nVote account {vote_account} not found in the pool validator list.");
        return;
    };

    println!("\nVote account : {}", p.vote_account);
    println!("List index   : {}", p.list_index);
    println!("Rank         : {} of {} scored", p.rank, o.num_scored);
    println!("Score        : {} (raw {})", group(p.score), group(p.raw_score));
    println!("Currently delegated : {}", p.currently_delegated);

    println!("\nScore components:");
    println!("   inflation commission (max) : {}%", p.commission_max);
    println!("   mev commission (avg)       : {} bps", p.mev_commission_avg_bps);
    println!("   validator age              : {} epochs", p.validator_age);
    println!("   vote-credit ratio (scaled) : {}", p.vote_credits_avg);
    println!("   delinquency ratio          : {:.4}", p.delinquency_ratio);

    if let Some(note) = &p.note {
        println!("\nVerdict: ⚠️  could not score — {note}");
        return;
    }

    println!("\nEligibility filters:");
    for f in &p.filters {
        println!("   {} {}", if f.passed { "✓" } else { "✗" }, f.name);
    }

    println!();
    if !p.failing_filters.is_empty() {
        println!(
            "Verdict: ✗ NOT eligible this cycle — failing: {}",
            p.failing_filters.join(", ")
        );
    } else if p.selected {
        println!(
            "Verdict: ★ SELECTED — target ~{:.2}% (~{:.1} ◎). Current {:.1} ◎, delta {:+.1} ◎.",
            p.target_percent, p.target_sol, p.current_sol, p.delta_sol
        );
        if o.timing.epochs_until_next_cycle > 0 {
            println!(
                "         Takes effect at the next scoring cycle (epoch {}); stake ramps over epochs due to caps.",
                o.timing.next_cycle_epoch
            );
        }
    } else {
        println!(
            "Verdict: eligible (score {}) but below the delegation cutoff (rank {}, need top {}). \
             Lowest selected score this cycle is {}.",
            group(p.score),
            p.rank,
            o.num_delegation_validators,
            group(o.cutoff_score)
        );
    }
}
