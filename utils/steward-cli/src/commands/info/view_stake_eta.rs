use std::sync::Arc;

use anyhow::{anyhow, Result};
use jito_steward::{
    constants::BASIS_POINTS_MAX, utils::get_target_lamports, Config, DirectedStakeMeta,
    StewardStateV2,
};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{native_token::lamports_to_sol, pubkey::Pubkey, stake::state::StakeStateV2};
use spl_stake_pool::state::ValidatorStakeInfo;
use stakenet_sdk::utils::accounts::{get_all_steward_accounts, get_directed_stake_meta};

use crate::commands::command_args::ViewStakeEta;

/// Why a validator is not yet at its target share. Mirrors the enum in the
/// validator-facing spec so the UI can map each variant to plain language.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum LimitingFactor {
    /// Already at (or within one minimum delegation of) the target share
    AtTarget,
    /// Not in the current delegation set; nothing is owed until the next scoring event
    NotInSet,
    /// Stake is already in flight for this validator and lands next epoch
    AwaitingCooldown,
    /// The scoring churn budget for this cycle is spent, so rotation has stopped
    ScoringChurnBudgetExhausted,
    /// Higher-ranked validators must be funded first
    QueuePosition,
    /// Supply is available and nobody is ahead; expect stake at the next rebalance
    NextRebalance,
    /// Nothing undelegated is available; arrival depends on new deposits
    ReserveEmpty,
}

impl LimitingFactor {
    /// Plain-language explanation, suitable for showing to a validator operator
    pub fn explain(&self, resets_at_epoch: u64, sol_ahead: f64) -> String {
        match self {
            Self::AtTarget => "You are at your full share. Nothing further is owed.".to_string(),
            Self::NotInSet => format!(
                "You are not in the current delegation set. The next selection is at epoch {resets_at_epoch}."
            ),
            Self::AwaitingCooldown => {
                "Stake is cooling down for you and should land next epoch.".to_string()
            }
            Self::ScoringChurnBudgetExhausted => format!(
                "Rotation for this cycle is used up. Budgets reset at epoch {resets_at_epoch}."
            ),
            Self::QueuePosition => format!(
                "{sol_ahead:.0} SOL must be funded to higher-ranked validators before you. Improving your score moves you up."
            ),
            Self::NextRebalance => {
                "Undelegated stake is available and nobody is ahead of you; expect stake at the next rebalance."
                    .to_string()
            }
            Self::ReserveEmpty => {
                "There is no undelegated stake in the pool right now; arrival depends on new deposits."
                    .to_string()
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PoolSummary {
    pub tvl_sol: f64,
    /// Pool lamports available for delegation, after per-validator rent and minimum delegation
    pub delegatable_sol: f64,
    pub delegation_set_size: u32,
    pub set_size_cap: u32,
    pub capacity_constrained: bool,
    pub target_stake_sol: f64,
    pub reserve_sol: f64,
    pub validators_in_pool: usize,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BudgetSummary {
    pub scoring_churn_cap_sol: f64,
    pub scoring_churn_used_sol: f64,
    pub scoring_churn_remaining_sol: f64,
    pub pct_consumed: f64,
    pub instant_unstake_remaining_sol: f64,
    pub stake_deposit_unstake_remaining_sol: f64,
    pub resets_at_epoch: u64,
    pub epochs_until_reset: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SupplySummary {
    /// Stake sitting above target, plus everything held by validators no longer in the set
    pub unstakeable_sol: f64,
    /// Total shortfall across every under-target set member
    pub total_shortfall_sol: f64,
    pub validators_under_target: usize,
    pub validators_over_target: usize,
    /// Set members holding effectively no stake — these need a full target each
    pub new_entrants: usize,
    pub new_entrant_demand_sol: f64,
    /// What rotation can still fund before the cycle resets
    pub fundable_this_cycle_sol: f64,
    pub fundable_per_future_cycle_sol: f64,
    /// Directed stake across the whole pool. Excluded from algorithmic targets, so it is not
    /// reducible by the scoring-unstake path.
    pub directed_total_sol: f64,
    pub validators_with_directed_stake: usize,
}

/// One line of the over-target / directed-stake listings
#[derive(Serialize, Deserialize, Debug)]
pub struct ValidatorLine {
    pub vote_account: String,
    pub rank: Option<usize>,
    pub in_delegation_set: bool,
    /// Full active stake as the stake pool sees it
    pub active_sol: f64,
    pub directed_sol: f64,
    /// What the algorithm actually measures against target
    pub undirected_sol: f64,
    pub target_sol: f64,
    /// Undirected stake above target — the only part the scoring-unstake path can remove
    pub excess_sol: f64,
    pub marked_instant_unstake: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct QueuePosition {
    pub rank: usize,
    pub of: usize,
    pub current_stake_sol: f64,
    pub directed_stake_sol: f64,
    pub shortfall_sol: f64,
    pub sol_ahead_in_queue: f64,
    pub transient_sol: f64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EtaBand {
    pub low_epochs: Option<u64>,
    pub high_epochs: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Eta {
    pub first_stake: EtaBand,
    pub full_target: EtaBand,
    pub limiting_factor: LimitingFactor,
    pub explanation: String,
    pub confidence: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StakeEtaOutput {
    pub vote_account: Option<String>,
    pub as_of_epoch: u64,
    pub in_delegation_set: bool,
    pub pool: PoolSummary,
    pub budget: BudgetSummary,
    pub supply: SupplySummary,
    pub position: Option<QueuePosition>,
    pub eta: Option<Eta>,
    /// Per-epoch deposit assumptions used for the low/high band, in SOL
    pub deposit_rate_assumption_sol: [f64; 2],
    /// Populated when --top is set
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub most_over_target: Vec<ValidatorLine>,
    /// Populated when --top is set
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub largest_directed_holders: Vec<ValidatorLine>,
    /// Populated when --schedule-epochs is set
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub schedule: Option<Schedule>,
}

/// One validator's position relative to its target
struct Row {
    vote_account: Pubkey,
    in_set: bool,
    rank: Option<usize>,
    /// Full active stake, as the stake pool reports it
    active: u64,
    /// Undirected active stake, net of base lamports — what targets are measured against
    current: u64,
    directed: u64,
    transient: u64,
    target: u64,
    marked_instant_unstake: bool,
    /// Position in sorted_raw_score_indices. Decreases are served lowest-raw-score first,
    /// so this drives unstake order, independent of `rank`.
    raw_rank: Option<usize>,
    /// Delegation denominator, i.e. N. Authoritative for set size.
    denominator: u32,
}

impl Row {
    fn to_line(&self) -> ValidatorLine {
        ValidatorLine {
            vote_account: self.vote_account.to_string(),
            rank: self.rank,
            in_delegation_set: self.in_set,
            active_sol: lamports_to_sol(self.active),
            directed_sol: lamports_to_sol(self.directed),
            undirected_sol: lamports_to_sol(self.current),
            target_sol: lamports_to_sol(self.target),
            excess_sol: lamports_to_sol(self.excess()),
            marked_instant_unstake: self.marked_instant_unstake,
        }
    }
}

impl Row {
    fn shortfall(&self) -> u64 {
        self.target.saturating_sub(self.current)
    }

    fn excess(&self) -> u64 {
        self.current.saturating_sub(self.target)
    }
}

/// Builds the per-validator table, sorted so that queue position can be read off directly.
fn build_rows(
    state: &StewardStateV2,
    validators: &[ValidatorStakeInfo],
    directed_stake_meta: Option<&DirectedStakeMeta>,
    delegatable_lamports: u64,
    base_lamport_balance: u64,
) -> Result<Vec<Row>> {
    let num_pool_validators = state.num_pool_validators as usize;

    // Rank is the position in sorted_score_indices, restricted to set members, so it matches
    // the order `increase_stake_calculation` walks when handing out reserve lamports.
    let mut rank_of_index = vec![None; num_pool_validators];
    let mut next_rank = 0usize;
    for &sorted_index in state.sorted_score_indices[..num_pool_validators].iter() {
        let index = sorted_index as usize;
        if index >= num_pool_validators {
            continue;
        }
        if state.delegations[index].numerator > 0 {
            rank_of_index[index] = Some(next_rank);
            next_rank += 1;
        }
    }

    // Decreases walk sorted_raw_score_indices in reverse, i.e. lowest raw score unstaked
    // first, and cover every pool validator rather than only set members.
    let mut raw_rank_of_index = vec![None; num_pool_validators];
    for (position, &sorted_index) in state.sorted_raw_score_indices[..num_pool_validators]
        .iter()
        .enumerate()
    {
        let index = sorted_index as usize;
        if index < num_pool_validators {
            raw_rank_of_index[index] = Some(position);
        }
    }

    let mut rows = Vec::with_capacity(num_pool_validators);
    for (index, validator) in validators.iter().enumerate().take(num_pool_validators) {
        let active = u64::from(validator.active_stake_lamports);
        let transient = u64::from(validator.transient_stake_lamports);
        let directed = directed_stake_meta
            .map(|meta| meta.directed_stake_lamports[index])
            .unwrap_or(0);

        // Targets are computed against undirected lamports, and the validator list includes
        // base lamports in active_stake_lamports.
        let current = active
            .saturating_sub(directed)
            .saturating_sub(base_lamport_balance);

        let delegation = &state.delegations[index];
        let target = get_target_lamports(delegation, delegatable_lamports)
            .map_err(|e| anyhow!("failed to compute target lamports: {e:?}"))?;

        rows.push(Row {
            vote_account: validator.vote_account_address,
            in_set: delegation.numerator > 0,
            rank: rank_of_index[index],
            active,
            current,
            directed,
            transient,
            target,
            marked_instant_unstake: state.instant_unstake.get(index).unwrap_or(false),
            raw_rank: raw_rank_of_index[index],
            denominator: delegation.denominator,
        });
    }

    Ok(rows)
}

/// Pool lamports that algorithmic targets divide.
///
/// Mirrors the program exactly: `instructions/rebalance.rs:259` hands `rebalance` the pool
/// **net of directed stake**, and `StewardStateV2::rebalance` then subtracts base lamports for
/// every validator. Dividing the gross pool by N instead overstates every target by
/// `directed_total / N`, which aggregates into a phantom pool-wide shortfall equal to the
/// entire directed balance.
fn delegatable_pool_lamports(
    total_lamports: u64,
    directed_total: u64,
    base_lamport_balance: u64,
    n_validators: u64,
) -> u64 {
    total_lamports
        .saturating_sub(directed_total)
        .saturating_sub(base_lamport_balance.saturating_mul(n_validators))
}

/// Pool-level demand and supply, derived purely from the per-validator rows so it can be
/// tested without RPC.
#[derive(Debug, PartialEq, Eq)]
struct Aggregates {
    /// Delegation set size, taken from the delegation denominator rather than by recounting
    /// members, since the denominator is what the program actually divides by.
    n: u32,
    target_stake: u64,
    /// Indices into `rows` of under-target set members, in the order the program funds them
    /// (descending score).
    under: Vec<usize>,
    total_shortfall: u64,
    over_target_count: usize,
    /// Stake above target, plus everything still held by validators that fell out of the set.
    unstakeable: u64,
    /// Set members holding effectively nothing — each needs close to a full target.
    new_entrants: usize,
    new_entrant_demand: u64,
}

fn aggregate(rows: &[Row], minimum_delegation: u64) -> Aggregates {
    let first_member = rows.iter().find(|r| r.in_set);
    let n = first_member.map(|r| r.denominator).unwrap_or(0);
    let target_stake = first_member.map(|r| r.target).unwrap_or(0);

    let mut under: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.in_set && r.shortfall() > minimum_delegation)
        .map(|(i, _)| i)
        .collect();
    under.sort_by_key(|&i| rows[i].rank.unwrap_or(usize::MAX));

    let total_shortfall = under.iter().map(|&i| rows[i].shortfall()).sum();
    let over_target_count = rows
        .iter()
        .filter(|r| r.in_set && r.excess() > minimum_delegation)
        .count();
    // For an out-of-set row `excess()` already equals `current`, since `build_rows` gives a
    // zero target to any validator with a zero delegation numerator. The branch is kept
    // explicit so this stays correct if that ever changes.
    let unstakeable = rows
        .iter()
        .map(|r| if r.in_set { r.excess() } else { r.current })
        .sum();

    // "Effectively nothing" is within 10% of a full target's worth of shortfall.
    let entrant_threshold = target_stake.saturating_mul(9) / 10;
    let entrants: Vec<usize> = under
        .iter()
        .copied()
        .filter(|&i| rows[i].shortfall() > entrant_threshold)
        .collect();
    let new_entrant_demand = entrants.iter().map(|&i| rows[i].shortfall()).sum();

    Aggregates {
        n,
        target_stake,
        under,
        total_shortfall,
        over_target_count,
        unstakeable,
        new_entrants: entrants.len(),
        new_entrant_demand,
    }
}

fn bps_of(lamports: u64, bps: u32) -> u64 {
    ((lamports as u128) * (bps as u128) / (BASIS_POINTS_MAX as u128)) as u64
}

/// A single projected stake movement for one validator in one epoch
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScheduledChange {
    pub epoch: u64,
    pub vote_account: String,
    pub rank: Option<usize>,
    /// Positive for a delegation, negative for an unstake, in SOL
    pub change_sol: f64,
    /// Projected undirected balance after this movement
    pub balance_after_sol: f64,
    pub target_sol: f64,
    pub direction: String,
}

/// Per-validator rollup of everything projected over the horizon
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScheduleSummaryRow {
    pub vote_account: String,
    pub rank: Option<usize>,
    pub in_delegation_set: bool,
    pub direction: String,
    pub current_sol: f64,
    pub target_sol: f64,
    /// Total movement still due to reach target, regardless of horizon
    pub total_due_sol: f64,
    /// How much of that the projection actually delivers inside the horizon
    pub projected_sol: f64,
    pub first_epoch: Option<u64>,
    pub last_epoch: Option<u64>,
    /// True when the horizon ends before the validator reaches target
    pub incomplete: bool,
}

/// Aggregate movement in a single projected epoch
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EpochTotals {
    pub epoch: u64,
    pub increased_sol: f64,
    pub decreased_sol: f64,
    pub validators_increased: usize,
    pub validators_decreased: usize,
    pub reserve_at_start_sol: f64,
    pub scoring_budget_remaining_sol: f64,
    /// True when this epoch begins a new cycle, at which point budgets reset and the
    /// delegation set is re-scored
    pub cycle_boundary: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Schedule {
    pub horizon_epochs: u64,
    pub min_change_sol: f64,
    pub deposit_rate_sol_per_epoch: f64,
    pub changes: Vec<ScheduledChange>,
    pub per_validator: Vec<ScheduleSummaryRow>,
    pub per_epoch: Vec<EpochTotals>,
    pub notes: Vec<String>,
}

/// Mutable simulation state for one validator
struct SimRow {
    vote_account: String,
    rank: Option<usize>,
    raw_rank: Option<usize>,
    in_set: bool,
    balance: u64,
    target: u64,
    marked_instant_unstake: bool,
    /// Stake in flight. The program skips validators with transient stake, so a validator
    /// touched in one epoch is not touched again in the next.
    transient: u64,
}

/// Projects stake movement epoch by epoch, replaying the program's ordering rules:
/// decreases lowest-raw-score first against the cycle unstake budgets, increases
/// highest-score first out of the reserve, with a one-epoch cooldown between the two.
#[allow(clippy::too_many_arguments)]
fn project_schedule(
    rows: &[Row],
    current_epoch: u64,
    next_cycle_epoch: u64,
    cycle_length: u64,
    horizon: u64,
    min_change: u64,
    minimum_delegation: u64,
    scoring_cap: u64,
    instant_cap: u64,
    scoring_used: u64,
    instant_used: u64,
    reserve_start: u64,
    deposit_per_epoch: u64,
) -> Schedule {
    let mut sim: Vec<SimRow> = rows
        .iter()
        .map(|r| SimRow {
            vote_account: r.vote_account.to_string(),
            rank: r.rank,
            raw_rank: r.raw_rank,
            in_set: r.in_set,
            balance: r.current,
            // A validator marked for instant unstake has an effective target of zero, which
            // is what `decrease_stake_calculation` uses.
            target: if r.marked_instant_unstake {
                0
            } else {
                r.target
            },
            marked_instant_unstake: r.marked_instant_unstake,
            transient: r.transient,
        })
        .collect();

    // Original shortfall/excess, for the "total due" column.
    let total_due: Vec<i128> = sim
        .iter()
        .map(|s| s.target as i128 - s.balance as i128)
        .collect();

    // Funding order: descending score. Unstake order: ascending raw score.
    let mut increase_order: Vec<usize> = (0..sim.len()).collect();
    increase_order.sort_by_key(|&i| sim[i].rank.unwrap_or(usize::MAX));
    let mut decrease_order: Vec<usize> = (0..sim.len()).collect();
    decrease_order.sort_by_key(|&i| sim[i].raw_rank.unwrap_or(usize::MAX));
    decrease_order.reverse();

    let mut scoring_remaining = scoring_cap.saturating_sub(scoring_used);
    let mut instant_remaining = instant_cap.saturating_sub(instant_used);
    let mut reserve = reserve_start;
    // Lamports unstaked this epoch, which become spendable next epoch.
    let mut cooling: u64 = 0;

    let mut changes: Vec<ScheduledChange> = Vec::new();
    let mut per_epoch: Vec<EpochTotals> = Vec::new();
    let mut first_epoch: Vec<Option<u64>> = vec![None; sim.len()];
    let mut last_epoch: Vec<Option<u64>> = vec![None; sim.len()];
    let mut projected: Vec<i128> = vec![0; sim.len()];

    for step in 1..=horizon {
        let epoch = current_epoch + step;
        let cycle_boundary =
            epoch >= next_cycle_epoch && (epoch - next_cycle_epoch) % cycle_length == 0;
        if cycle_boundary {
            scoring_remaining = scoring_cap;
            instant_remaining = instant_cap;
        }

        // Last epoch's unstakes have cooled down; deposits arrive.
        reserve = reserve
            .saturating_add(cooling)
            .saturating_add(deposit_per_epoch);
        cooling = 0;

        let reserve_at_start = reserve;
        let scoring_at_start = scoring_remaining;
        let mut increased = 0u64;
        let mut decreased = 0u64;
        let mut n_inc = 0usize;
        let mut n_dec = 0usize;

        // Clear in-flight stake from the previous epoch before deciding this epoch's moves.
        let touched_last: Vec<bool> = sim.iter().map(|s| s.transient > 0).collect();
        for s in sim.iter_mut() {
            s.transient = 0;
        }

        // ---- Decreases: lowest raw score first, against the cycle budgets ----
        for &i in decrease_order.iter() {
            if touched_last[i] {
                continue;
            }
            let s = &sim[i];
            if s.balance <= s.target {
                continue;
            }
            let excess = s.balance - s.target;
            let budget = if s.marked_instant_unstake {
                &mut instant_remaining
            } else {
                &mut scoring_remaining
            };
            if *budget <= minimum_delegation {
                continue;
            }
            let amount = excess.min(*budget);
            if amount <= minimum_delegation {
                continue;
            }
            *budget -= amount;
            let s = &mut sim[i];
            s.balance -= amount;
            s.transient = amount;
            cooling += amount;
            decreased += amount;
            n_dec += 1;
            projected[i] -= amount as i128;
            first_epoch[i] = first_epoch[i].or(Some(epoch));
            last_epoch[i] = Some(epoch);
            changes.push(ScheduledChange {
                epoch,
                vote_account: s.vote_account.clone(),
                rank: s.rank,
                change_sol: -lamports_to_sol(amount),
                balance_after_sol: lamports_to_sol(s.balance),
                target_sol: lamports_to_sol(s.target),
                direction: if s.marked_instant_unstake {
                    "instant-unstake".to_string()
                } else {
                    "decrease".to_string()
                },
            });
        }

        // ---- Increases: highest score first, out of the reserve ----
        for &i in increase_order.iter() {
            if reserve <= minimum_delegation {
                break;
            }
            if touched_last[i] || sim[i].transient > 0 {
                continue;
            }
            let s = &sim[i];
            if !s.in_set || s.balance >= s.target {
                continue;
            }
            let shortfall = s.target - s.balance;
            let amount = shortfall.min(reserve);
            if amount <= minimum_delegation {
                continue;
            }
            reserve -= amount;
            let s = &mut sim[i];
            s.balance += amount;
            s.transient = amount;
            increased += amount;
            n_inc += 1;
            projected[i] += amount as i128;
            first_epoch[i] = first_epoch[i].or(Some(epoch));
            last_epoch[i] = Some(epoch);
            changes.push(ScheduledChange {
                epoch,
                vote_account: s.vote_account.clone(),
                rank: s.rank,
                change_sol: lamports_to_sol(amount),
                balance_after_sol: lamports_to_sol(s.balance),
                target_sol: lamports_to_sol(s.target),
                direction: "increase".to_string(),
            });
        }

        per_epoch.push(EpochTotals {
            epoch,
            increased_sol: lamports_to_sol(increased),
            decreased_sol: lamports_to_sol(decreased),
            validators_increased: n_inc,
            validators_decreased: n_dec,
            reserve_at_start_sol: lamports_to_sol(reserve_at_start),
            scoring_budget_remaining_sol: lamports_to_sol(scoring_at_start),
            cycle_boundary,
        });
    }

    // Keep only validators whose total movement clears the noise threshold.
    let mut per_validator: Vec<ScheduleSummaryRow> = (0..sim.len())
        .filter(|&i| {
            total_due[i].unsigned_abs() as u64 >= min_change
                || projected[i].unsigned_abs() as u64 >= min_change
        })
        .map(|i| {
            let s = &sim[i];
            ScheduleSummaryRow {
                vote_account: s.vote_account.clone(),
                rank: s.rank,
                in_delegation_set: s.in_set,
                direction: if total_due[i] > 0 {
                    "increase".to_string()
                } else if s.marked_instant_unstake {
                    "instant-unstake".to_string()
                } else {
                    "decrease".to_string()
                },
                current_sol: lamports_to_sol(rows[i].current),
                target_sol: lamports_to_sol(s.target),
                total_due_sol: total_due[i] as f64 / 1e9,
                projected_sol: projected[i] as f64 / 1e9,
                first_epoch: first_epoch[i],
                last_epoch: last_epoch[i],
                incomplete: projected[i].unsigned_abs() < total_due[i].unsigned_abs(),
            }
        })
        .collect();
    per_validator.sort_by(|a, b| {
        b.total_due_sol
            .abs()
            .partial_cmp(&a.total_due_sol.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Drop the individual movements for validators we filtered out.
    let kept: std::collections::HashSet<&str> = per_validator
        .iter()
        .map(|r| r.vote_account.as_str())
        .collect();
    let changes = changes
        .iter()
        .filter(|c| kept.contains(c.vote_account.as_str()))
        .cloned()
        .collect();

    Schedule {
        horizon_epochs: horizon,
        min_change_sol: lamports_to_sol(min_change),
        deposit_rate_sol_per_epoch: lamports_to_sol(deposit_per_epoch),
        changes,
        per_validator,
        per_epoch,
        notes: vec![
            "Targets, the delegation set and N are held fixed across the horizon. At each cycle boundary the program re-scores and both can change, so movements past the first boundary are indicative only.".to_string(),
            "Unstaked lamports are modelled as cooling down for one epoch before becoming spendable, and a validator touched in one epoch is skipped in the next, matching the program's transient-stake check.".to_string(),
            "Stake-deposit unstaking is folded into the scoring budget, since distinguishing deposit-driven excess needs per-validator internal balances.".to_string(),
            "Directed stake is excluded throughout; balances shown are undirected.".to_string(),
        ],
    }
}

/// Walks cycles forward, drawing `need` down against per-cycle supply, and returns the
/// number of epochs from `current_epoch` until it is covered.
fn epochs_until_funded(
    need: u64,
    fundable_this_cycle: u64,
    fundable_per_cycle: u64,
    epochs_until_reset: u64,
    cycle_length: u64,
) -> Option<u64> {
    if need == 0 {
        return Some(0);
    }
    if need <= fundable_this_cycle {
        // Supply already exists inside this cycle, so the only remaining wait is the one
        // epoch it takes unstaked lamports to cool down and be re-delegated.
        return Some(1);
    }
    if fundable_per_cycle == 0 {
        return None;
    }
    let remaining = need.saturating_sub(fundable_this_cycle);
    // Number of whole future cycles required to cover the remainder.
    let cycles = remaining.div_ceil(fundable_per_cycle);
    Some(epochs_until_reset + (cycles.saturating_sub(1)) * cycle_length)
}

#[allow(clippy::too_many_arguments)]
fn classify(
    row: Option<&Row>,
    minimum_delegation: u64,
    scoring_remaining: u64,
    reserve_available: u64,
    sol_ahead: u64,
) -> LimitingFactor {
    let Some(row) = row else {
        return LimitingFactor::NotInSet;
    };
    if !row.in_set {
        return LimitingFactor::NotInSet;
    }
    if row.shortfall() <= minimum_delegation {
        return LimitingFactor::AtTarget;
    }
    if row.transient > 0 {
        return LimitingFactor::AwaitingCooldown;
    }
    if scoring_remaining <= minimum_delegation && reserve_available <= minimum_delegation {
        return LimitingFactor::ScoringChurnBudgetExhausted;
    }
    // Everything queued ahead already fits in the available reserve, so this validator is
    // next in line rather than blocked.
    if sol_ahead.saturating_add(minimum_delegation) <= reserve_available {
        return LimitingFactor::NextRebalance;
    }
    if reserve_available <= minimum_delegation {
        return LimitingFactor::ReserveEmpty;
    }
    LimitingFactor::QueuePosition
}

pub async fn command_view_stake_eta(
    args: ViewStakeEta,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.view_parameters.steward_config;
    let print_json = args.view_parameters.print_json;

    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;
    // Do NOT fall back to "no directed stake" on error. Targets are computed net of directed
    // stake, so a failed fetch would silently overstate every target by directed_total / N and
    // invent a pool-wide shortfall that does not exist.
    let directed_stake_meta =
        match get_directed_stake_meta(client.clone(), &steward_config, &program_id).await {
            Ok(meta) => Some(meta),
            Err(e) if args.assume_no_directed_stake => {
                eprintln!(
                    "warning: could not read DirectedStakeMeta ({e}); \
                 continuing with --assume-no-directed-stake, targets assume zero directed stake"
                );
                None
            }
            Err(e) => {
                return Err(anyhow!(
                    "could not read DirectedStakeMeta: {e}\n\
                 Targets are computed net of directed stake, so proceeding without it would \
                 overstate every validator's target and report a shortfall that does not exist. \
                 Pass --assume-no-directed-stake only if this deployment genuinely has none."
                ))
            }
        };

    let epoch_info = client.get_epoch_info().await?;
    let current_epoch = epoch_info.epoch;

    // Mirror the program's own arithmetic: `minimum_delegation(get_minimum_delegation())`
    // plus rent for a stake account.
    let stake_minimum_delegation = client.get_stake_minimum_delegation().await?;
    let minimum_delegation = spl_stake_pool::minimum_delegation(stake_minimum_delegation);
    let stake_rent = client
        .get_minimum_balance_for_rent_exemption(std::mem::size_of::<StakeStateV2>())
        .await?;
    let base_lamport_balance = minimum_delegation
        .checked_add(stake_rent)
        .ok_or_else(|| anyhow!("overflow computing base lamport balance"))?;

    let state_account = &all_steward_accounts.state_account;
    let state = &state_account.state;
    let config: &Config = &all_steward_accounts.config_account;
    let params = &config.parameters;
    let validator_list = &all_steward_accounts.validator_list_account;
    let validators_in_pool = validator_list.as_ref().validators.len();

    let total_lamports = all_steward_accounts.stake_pool_account.total_lamports;

    // Targets are computed against the UNDIRECTED pool. `rebalance` is handed
    // `total_pool_lamports - directed_stake_meta.total_staked_lamports()`
    // (instructions/rebalance.rs:259) and subtracts base lamports from that, so directed
    // stake must come out before dividing by N or every target is overstated.
    let directed_total = directed_stake_meta
        .as_ref()
        .map(|meta| meta.total_staked_lamports())
        .unwrap_or(0);
    let undirected_pool_lamports = total_lamports.saturating_sub(directed_total);
    let delegatable_lamports = delegatable_pool_lamports(
        total_lamports,
        directed_total,
        base_lamport_balance,
        validators_in_pool as u64,
    );

    let rows = build_rows(
        state,
        &validator_list.as_ref().validators,
        directed_stake_meta.as_deref(),
        delegatable_lamports,
        base_lamport_balance,
    )?;

    let agg = aggregate(&rows, minimum_delegation);
    let Aggregates {
        n,
        target_stake,
        ref under,
        total_shortfall,
        over_target_count,
        unstakeable,
        new_entrants,
        new_entrant_demand,
    } = agg;
    // Positions in `rows`, in the order the program funds them.
    let under: Vec<&Row> = under.iter().map(|&i| &rows[i]).collect();

    let scoring_cap = bps_of(delegatable_lamports, params.scoring_unstake_cap_bps);
    let instant_cap = bps_of(delegatable_lamports, params.instant_unstake_cap_bps);
    let deposit_cap = bps_of(delegatable_lamports, params.stake_deposit_unstake_cap_bps);
    let scoring_remaining = scoring_cap.saturating_sub(state.scoring_unstake_total);

    let epochs_until_reset = state.next_cycle_epoch.saturating_sub(current_epoch);
    let cycle_length = params.num_epochs_between_scoring;

    // Reserve must keep rent for every validator plus one transient account, matching
    // the reservation made in `StewardStateV2::rebalance`.
    let reserve_lamports = all_steward_accounts.reserve_stake_account.lamports;
    // The program zeroes the usable reserve once undirected TVL reaches the ceiling, and
    // otherwise caps it to the remaining headroom (instructions/rebalance.rs:262-271).
    let stake_ceiling = params.undirected_stake_ceiling_lamports();
    let capped_reserve = if undirected_pool_lamports >= stake_ceiling {
        0
    } else {
        reserve_lamports.min(stake_ceiling.saturating_sub(undirected_pool_lamports))
    };
    let reserve_available =
        capped_reserve.saturating_sub(stake_rent.saturating_mul(validators_in_pool as u64 + 1));

    // Deposit flow is the one input that is not on-chain. Band it: the low case assumes no
    // net deposits at all, the high case uses the operator-supplied rate.
    let deposit_low = 0f64;
    let deposit_high = args.deposit_rate_sol;
    let deposit_high_lamports = (deposit_high * 1e9) as u64;

    let rotation_this_cycle = scoring_remaining.min(unstakeable);
    let fundable_this_cycle_low = rotation_this_cycle.saturating_add(reserve_available);
    let fundable_this_cycle_high = fundable_this_cycle_low
        .saturating_add(deposit_high_lamports.saturating_mul(epochs_until_reset));
    let fundable_per_cycle_low = scoring_cap.min(unstakeable);
    let fundable_per_cycle_high =
        fundable_per_cycle_low.saturating_add(deposit_high_lamports.saturating_mul(cycle_length));

    let pool = PoolSummary {
        tvl_sol: lamports_to_sol(total_lamports),
        delegatable_sol: lamports_to_sol(delegatable_lamports),
        delegation_set_size: n,
        set_size_cap: params.num_delegation_validators,
        capacity_constrained: n >= params.num_delegation_validators,
        target_stake_sol: lamports_to_sol(target_stake),
        reserve_sol: lamports_to_sol(reserve_lamports),
        validators_in_pool,
    };

    let budget = BudgetSummary {
        scoring_churn_cap_sol: lamports_to_sol(scoring_cap),
        scoring_churn_used_sol: lamports_to_sol(state.scoring_unstake_total),
        scoring_churn_remaining_sol: lamports_to_sol(scoring_remaining),
        pct_consumed: if scoring_cap == 0 {
            0.
        } else {
            state.scoring_unstake_total as f64 / scoring_cap as f64 * 100.
        },
        instant_unstake_remaining_sol: lamports_to_sol(
            instant_cap.saturating_sub(state.instant_unstake_total),
        ),
        stake_deposit_unstake_remaining_sol: lamports_to_sol(
            deposit_cap.saturating_sub(state.stake_deposit_unstake_total),
        ),
        resets_at_epoch: state.next_cycle_epoch,
        epochs_until_reset,
    };

    let supply = SupplySummary {
        unstakeable_sol: lamports_to_sol(unstakeable),
        total_shortfall_sol: lamports_to_sol(total_shortfall),
        validators_under_target: under.len(),
        validators_over_target: over_target_count,
        new_entrants,
        new_entrant_demand_sol: lamports_to_sol(new_entrant_demand),
        fundable_this_cycle_sol: lamports_to_sol(fundable_this_cycle_low),
        fundable_per_future_cycle_sol: lamports_to_sol(fundable_per_cycle_low),
        directed_total_sol: lamports_to_sol(directed_total),
        validators_with_directed_stake: rows.iter().filter(|r| r.directed > 0).count(),
    };

    // Listings that show how much of the pool's excess is directed, and therefore outside
    // the reach of the scoring-unstake path.
    let (most_over_target, largest_directed_holders) = if args.top == 0 {
        (Vec::new(), Vec::new())
    } else {
        let mut by_excess: Vec<&Row> = rows.iter().filter(|r| r.excess() > 0).collect();
        by_excess.sort_by_key(|r| std::cmp::Reverse(r.excess()));

        let mut by_directed: Vec<&Row> = rows.iter().filter(|r| r.directed > 0).collect();
        by_directed.sort_by_key(|r| std::cmp::Reverse(r.directed));

        (
            by_excess
                .iter()
                .take(args.top)
                .map(|r| r.to_line())
                .collect(),
            by_directed
                .iter()
                .take(args.top)
                .map(|r| r.to_line())
                .collect(),
        )
    };

    // Per-validator view, when a vote account was supplied.
    let (position, eta, in_set) = match args.vote_account {
        None => (None, None, false),
        Some(vote_account) => {
            let row = rows
                .iter()
                .find(|r| r.vote_account == vote_account)
                .ok_or_else(|| {
                    anyhow!("vote account {vote_account} is not in the stake pool validator list")
                })?;

            // Cumulative shortfall of every under-target member the program serves before this
            // validator. Computed from rank rather than by position in `under`, so it stays
            // correct when this validator is not itself under target.
            let row_rank = row.rank.unwrap_or(usize::MAX);
            let ahead: u64 = under
                .iter()
                .filter(|r| r.rank.unwrap_or(usize::MAX) < row_rank)
                .map(|r| r.shortfall())
                .sum();
            let need_first = ahead.saturating_add(minimum_delegation);
            let need_full = ahead.saturating_add(row.shortfall());

            let limiting_factor = classify(
                Some(row),
                minimum_delegation,
                scoring_remaining,
                reserve_available,
                ahead,
            );

            let eta = if limiting_factor == LimitingFactor::AtTarget {
                Eta {
                    first_stake: EtaBand {
                        low_epochs: Some(0),
                        high_epochs: Some(0),
                    },
                    full_target: EtaBand {
                        low_epochs: Some(0),
                        high_epochs: Some(0),
                    },
                    limiting_factor,
                    explanation: limiting_factor
                        .explain(state.next_cycle_epoch, lamports_to_sol(ahead)),
                    confidence: "high".to_string(),
                }
            } else {
                // The high-deposit assumption produces the optimistic (low epoch) bound.
                let first_low = epochs_until_funded(
                    need_first,
                    fundable_this_cycle_high,
                    fundable_per_cycle_high,
                    epochs_until_reset,
                    cycle_length,
                );
                let first_high = epochs_until_funded(
                    need_first,
                    fundable_this_cycle_low,
                    fundable_per_cycle_low,
                    epochs_until_reset,
                    cycle_length,
                );
                let full_low = epochs_until_funded(
                    need_full,
                    fundable_this_cycle_high,
                    fundable_per_cycle_high,
                    epochs_until_reset,
                    cycle_length,
                );
                let full_high = epochs_until_funded(
                    need_full,
                    fundable_this_cycle_low,
                    fundable_per_cycle_low,
                    epochs_until_reset,
                    cycle_length,
                );
                Eta {
                    first_stake: EtaBand {
                        low_epochs: first_low,
                        high_epochs: first_high,
                    },
                    full_target: EtaBand {
                        low_epochs: full_low,
                        high_epochs: full_high,
                    },
                    limiting_factor,
                    explanation: limiting_factor
                        .explain(state.next_cycle_epoch, lamports_to_sol(ahead)),
                    // Anything that has to cross a cycle boundary is re-derived at that
                    // boundary, because the set and N both move.
                    confidence: if full_high.is_some_and(|e| e <= epochs_until_reset) {
                        "medium".to_string()
                    } else {
                        "low".to_string()
                    },
                }
            };

            (
                Some(QueuePosition {
                    rank: row.rank.unwrap_or(0),
                    of: n as usize,
                    current_stake_sol: lamports_to_sol(row.current),
                    directed_stake_sol: lamports_to_sol(row.directed),
                    shortfall_sol: lamports_to_sol(row.shortfall()),
                    // Nothing is owed to an at-target validator, so a queue figure would only
                    // mislead.
                    sol_ahead_in_queue: if limiting_factor == LimitingFactor::AtTarget {
                        0.
                    } else {
                        lamports_to_sol(ahead)
                    },
                    transient_sol: lamports_to_sol(row.transient),
                }),
                Some(eta),
                row.in_set,
            )
        }
    };

    let schedule = if args.schedule_epochs == 0 {
        None
    } else {
        Some(project_schedule(
            &rows,
            current_epoch,
            state.next_cycle_epoch,
            cycle_length,
            args.schedule_epochs,
            (args.min_change_sol * 1e9) as u64,
            minimum_delegation,
            scoring_cap,
            instant_cap,
            state.scoring_unstake_total,
            state.instant_unstake_total,
            reserve_available,
            deposit_high_lamports,
        ))
    };

    let output = StakeEtaOutput {
        vote_account: args.vote_account.map(|v| v.to_string()),
        as_of_epoch: current_epoch,
        in_delegation_set: in_set,
        pool,
        budget,
        supply,
        position,
        eta,
        deposit_rate_assumption_sol: [deposit_low, deposit_high],
        most_over_target,
        largest_directed_holders,
        schedule,
    };

    if print_json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_human(&output);
    }

    Ok(())
}

fn fmt_band(band: &EtaBand) -> String {
    match (band.low_epochs, band.high_epochs) {
        (Some(0), Some(0)) => "now".to_string(),
        (Some(low), Some(high)) if low == high => format!("~{low} epochs"),
        (Some(low), Some(high)) => format!("~{low}–{high} epochs"),
        (Some(low), None) => format!("at least {low} epochs"),
        _ => "unknown — no supply available under current conditions".to_string(),
    }
}

fn print_lines(title: &str, lines: &[ValidatorLine]) {
    println!("\n━━━ {title} ━━━");
    println!(
        "  {:<45} {:>5} {:>11} {:>11} {:>11} {:>11} {:>11}",
        "vote account", "rank", "active", "directed", "undirected", "target", "excess"
    );
    for l in lines {
        println!(
            "  {:<45} {:>5} {:>11.0} {:>11.0} {:>11.0} {:>11.0} {:>11.0}{}{}",
            l.vote_account,
            l.rank.map(|r| r.to_string()).unwrap_or("—".to_string()),
            l.active_sol,
            l.directed_sol,
            l.undirected_sol,
            l.target_sol,
            l.excess_sol,
            if l.in_delegation_set {
                ""
            } else {
                "  [not in set]"
            },
            if l.marked_instant_unstake {
                "  [instant-unstake]"
            } else {
                ""
            },
        );
    }
}

fn print_schedule(s: &Schedule) {
    println!(
        "\n━━━ Expected stake change by epoch — next {} epochs, changes ≥ {:.0} SOL ━━━",
        s.horizon_epochs, s.min_change_sol
    );
    if s.per_validator.is_empty() {
        println!("  No validator has a change of that size due.");
        return;
    }

    println!(
        "\n  {:<8} {:>5} {:<44} {:>12} {:>13} {:>12}",
        "epoch", "rank", "vote account", "change", "balance after", "target"
    );
    let mut last_epoch = 0u64;
    for c in &s.changes {
        if c.epoch != last_epoch {
            if last_epoch != 0 {
                println!();
            }
            last_epoch = c.epoch;
        }
        println!(
            "  {:<8} {:>5} {:<44} {:>+12.0} {:>13.0} {:>12.0}  {}",
            c.epoch,
            c.rank.map(|r| r.to_string()).unwrap_or("—".to_string()),
            c.vote_account,
            c.change_sol,
            c.balance_after_sol,
            c.target_sol,
            c.direction,
        );
    }

    println!("\n━━━ Per-validator rollup ━━━");
    println!(
        "  {:<44} {:>5} {:>11} {:>11} {:>11} {:>11} {:>7} {:>6}",
        "vote account", "rank", "current", "target", "total due", "projected", "epochs", "done"
    );
    for r in &s.per_validator {
        let epochs = match (r.first_epoch, r.last_epoch) {
            (Some(f), Some(l)) if f == l => format!("{f}"),
            (Some(f), Some(l)) => format!("{f}-{l}"),
            _ => "none".to_string(),
        };
        println!(
            "  {:<44} {:>5} {:>11.0} {:>11.0} {:>+11.0} {:>+11.0} {:>7} {:>6}{}",
            r.vote_account,
            r.rank.map(|x| x.to_string()).unwrap_or("—".to_string()),
            r.current_sol,
            r.target_sol,
            r.total_due_sol,
            r.projected_sol,
            epochs,
            if r.incomplete { "no" } else { "yes" },
            if r.in_delegation_set {
                ""
            } else {
                "  [not in set]"
            },
        );
    }

    println!("\n━━━ Per-epoch totals ━━━");
    println!(
        "  {:<8} {:>12} {:>12} {:>6} {:>6} {:>14} {:>14}",
        "epoch", "increased", "decreased", "up", "down", "reserve start", "churn left"
    );
    for e in &s.per_epoch {
        println!(
            "  {:<8} {:>12.0} {:>12.0} {:>6} {:>6} {:>14.0} {:>14.0}{}",
            e.epoch,
            e.increased_sol,
            e.decreased_sol,
            e.validators_increased,
            e.validators_decreased,
            e.reserve_at_start_sol,
            e.scoring_budget_remaining_sol,
            if e.cycle_boundary {
                "   ← new cycle, budgets reset & set re-scored"
            } else {
                ""
            },
        );
    }

    println!("\n  Assumptions:");
    for n in &s.notes {
        println!("    - {n}");
    }
}

fn print_human(o: &StakeEtaOutput) {
    println!("\n━━━ Pool (epoch {}) ━━━", o.as_of_epoch);
    println!("  TVL:                    {:>14.0} SOL", o.pool.tvl_sol);
    println!(
        "  Delegatable:            {:>14.0} SOL",
        o.pool.delegatable_sol
    );
    println!(
        "  Delegation set:         {:>14} of {} cap{}",
        o.pool.delegation_set_size,
        o.pool.set_size_cap,
        if o.pool.capacity_constrained {
            "  (AT CAP — rank gates membership)"
        } else {
            "  (below cap — eligibility gates membership, not rank)"
        }
    );
    println!(
        "  Target per validator:   {:>14.0} SOL",
        o.pool.target_stake_sol
    );
    println!("  Reserve:                {:>14.0} SOL", o.pool.reserve_sol);

    println!("\n━━━ Churn budget (per cycle) ━━━");
    println!(
        "  Scoring:                {:>14.0} / {:.0} SOL used  ({:.1}%)",
        o.budget.scoring_churn_used_sol, o.budget.scoring_churn_cap_sol, o.budget.pct_consumed
    );
    println!(
        "  Scoring remaining:      {:>14.0} SOL",
        o.budget.scoring_churn_remaining_sol
    );
    println!(
        "  Instant remaining:      {:>14.0} SOL",
        o.budget.instant_unstake_remaining_sol
    );
    println!(
        "  Deposit remaining:      {:>14.0} SOL",
        o.budget.stake_deposit_unstake_remaining_sol
    );
    println!(
        "  Resets at epoch:        {:>14}  ({} epochs away)",
        o.budget.resets_at_epoch, o.budget.epochs_until_reset
    );

    println!("\n━━━ Demand and supply ━━━");
    println!(
        "  Under target:           {:>14} validators, {:.0} SOL short",
        o.supply.validators_under_target, o.supply.total_shortfall_sol
    );
    println!(
        "    of which new entrants:{:>14} validators, {:.0} SOL",
        o.supply.new_entrants, o.supply.new_entrant_demand_sol
    );
    println!(
        "  Over target:            {:>14} validators",
        o.supply.validators_over_target
    );
    println!(
        "  Unstakeable supply:     {:>14.0} SOL",
        o.supply.unstakeable_sol
    );
    println!(
        "  Fundable this cycle:    {:>14.0} SOL",
        o.supply.fundable_this_cycle_sol
    );
    println!(
        "  Fundable per cycle:     {:>14.0} SOL",
        o.supply.fundable_per_future_cycle_sol
    );
    println!(
        "  Directed stake:         {:>14.0} SOL across {} validators  (excluded from targets)",
        o.supply.directed_total_sol, o.supply.validators_with_directed_stake
    );

    if !o.most_over_target.is_empty() {
        print_lines(
            "Furthest over target (excess is undirected, so scoring-unstake can remove it)",
            &o.most_over_target,
        );
    }
    if !o.largest_directed_holders.is_empty() {
        print_lines(
            "Largest directed-stake holders (directed portion is NOT algorithmically reducible)",
            &o.largest_directed_holders,
        );
    }

    if let Some(sched) = &o.schedule {
        print_schedule(sched);
    }

    if let (Some(p), Some(e)) = (&o.position, &o.eta) {
        println!(
            "\n━━━ {} ━━━",
            o.vote_account.as_deref().unwrap_or("validator")
        );
        println!(
            "  In delegation set:      {:>14}",
            if o.in_delegation_set { "yes" } else { "no" }
        );
        println!("  Rank:                   {:>14} of {}", p.rank, p.of);
        println!(
            "  Current stake:          {:>14.0} SOL",
            p.current_stake_sol
        );
        if p.directed_stake_sol > 0. {
            println!(
                "    (directed, excluded): {:>14.0} SOL",
                p.directed_stake_sol
            );
        }
        println!("  Shortfall to target:    {:>14.0} SOL", p.shortfall_sol);
        println!(
            "  SOL ahead in queue:     {:>14.0} SOL",
            p.sol_ahead_in_queue
        );
        if p.transient_sol > 0. {
            println!("  In flight:              {:>14.0} SOL", p.transient_sol);
        }
        println!("\n  First stake:  {}", fmt_band(&e.first_stake));
        println!("  Full target:  {}", fmt_band(&e.full_target));
        println!("  Why:          {}", e.explanation);
        println!(
            "  Confidence:   {}  (deposit assumption {:.0}–{:.0} SOL/epoch)",
            e.confidence, o.deposit_rate_assumption_sol[0], o.deposit_rate_assumption_sol[1]
        );
        println!(
            "\n  Note: estimates crossing epoch {} assume the set and N are unchanged; both are",
            o.budget.resets_at_epoch
        );
        println!("  re-derived at each cycle boundary.");
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    use jito_steward::{
        constants::{MAX_VALIDATORS, SORTED_INDEX_DEFAULT},
        delegation::RebalanceType,
        state::directed_stake::DirectedStakeTarget,
        BitMask, Delegation, Parameters, StewardStateEnum,
    };
    use spl_stake_pool::{
        big_vec::BigVec,
        state::{PodStakeStatus, StakeStatus},
    };

    const SOL: u64 = 1_000_000_000;

    /// Byte layout of `spl_stake_pool::state::ValidatorList`'s vec, so the program's `BigVec`
    /// reader sees the same thing it sees on chain. Mirrors `tests/tests/steward/mod.rs`.
    fn serialize_validator_list(validators: &[ValidatorStakeInfo]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&(validators.len() as u32).to_le_bytes());
        for v in validators {
            data.extend_from_slice(&u64::from(v.active_stake_lamports).to_le_bytes());
            data.extend_from_slice(&u64::from(v.transient_stake_lamports).to_le_bytes());
            data.extend_from_slice(&u64::from(v.last_update_epoch).to_le_bytes());
            data.extend_from_slice(&u64::from(v.transient_seed_suffix).to_le_bytes());
            data.extend_from_slice(&u32::from(v.unused).to_le_bytes());
            data.extend_from_slice(&u32::from(v.validator_seed_suffix).to_le_bytes());
            // PodStakeStatus is a repr(transparent) wrapper around u8.
            let status_byte = unsafe { *(&v.status as *const PodStakeStatus as *const u8) };
            data.push(status_byte);
            data.extend_from_slice(v.vote_account_address.as_ref());
        }
        data
    }

    /// A pool of `actives.len()` validators, all in the delegation set with an equal 1/N share,
    /// scored highest-first in index order.
    fn fixture(actives: &[u64]) -> (Box<StewardStateV2>, Vec<ValidatorStakeInfo>, Parameters) {
        let n = actives.len();
        let mut sorted = [SORTED_INDEX_DEFAULT; MAX_VALIDATORS];
        let mut scores = [0u64; MAX_VALIDATORS];
        let mut delegations = [Delegation::default(); MAX_VALIDATORS];
        let mut balances = [0u64; MAX_VALIDATORS];
        for i in 0..n {
            sorted[i] = i as u16;
            scores[i] = (n - i) as u64 * 1_000_000;
            delegations[i] = Delegation::new(1, n as u32);
            // Internal balance == actual, so no stake-deposit excess is inferred.
            balances[i] = actives[i];
        }

        let state = Box::new(StewardStateV2 {
            state_tag: StewardStateEnum::Rebalance,
            validator_lamport_balances: balances,
            scores,
            sorted_score_indices: sorted,
            raw_scores: scores,
            sorted_raw_score_indices: sorted,
            delegations,
            instant_unstake: BitMask::default(),
            progress: BitMask::default(),
            validators_for_immediate_removal: BitMask::default(),
            validators_to_remove: BitMask::default(),
            start_computing_scores_slot: 0,
            current_epoch: 100,
            next_cycle_epoch: 110,
            num_pool_validators: n as u64,
            scoring_unstake_total: 0,
            instant_unstake_total: 0,
            stake_deposit_unstake_total: 0,
            status_flags: 0,
            validators_added: 0,
            _padding0: [0; 2],
        });

        let validators: Vec<ValidatorStakeInfo> = actives
            .iter()
            .map(|a| ValidatorStakeInfo {
                active_stake_lamports: (*a).into(),
                transient_stake_lamports: 0.into(),
                status: StakeStatus::Active.into(),
                vote_account_address: Pubkey::new_unique(),
                ..ValidatorStakeInfo::default()
            })
            .collect();

        // Uncapped, so these tests isolate target arithmetic rather than budget rationing.
        let params = Parameters {
            scoring_unstake_cap_bps: 10_000,
            instant_unstake_cap_bps: 10_000,
            stake_deposit_unstake_cap_bps: 10_000,
            ..Parameters::default()
        };

        (state, validators, params)
    }

    /// Directed stake recorded only pool-wide, via the targets array. Per-validator indices stay
    /// at u64::MAX so no individual balance is adjusted — this isolates the denominator.
    fn directed_pool_only(amount: u64) -> Box<DirectedStakeMeta> {
        let mut meta = Box::new(DirectedStakeMeta::default());
        meta.total_stake_targets = 1;
        meta.targets[0] = DirectedStakeTarget {
            total_staked_lamports: amount,
            total_target_lamports: amount,
            ..DirectedStakeTarget::default()
        };
        meta
    }

    /// Build the rows the way the command does, from a fixture.
    fn rows_for(
        state: &StewardStateV2,
        validators: &[ValidatorStakeInfo],
        meta: &DirectedStakeMeta,
        total_pool: u64,
        base: u64,
    ) -> Vec<Row> {
        let pool = delegatable_pool_lamports(
            total_pool,
            meta.total_staked_lamports(),
            base,
            validators.len() as u64,
        );
        build_rows(state, validators, Some(meta), pool, base).expect("build_rows")
    }

    // ---------------------------------------------------------------- build_rows

    /// Rank must follow `sorted_score_indices` restricted to set members, since that is the
    /// order `increase_stake_calculation` walks. Index order is deliberately NOT score order
    /// here, so a test that confused the two would fail.
    #[test]
    fn build_rows_derives_rank_from_score_order_not_index_order() {
        let actives = [100 * SOL, 200 * SOL, 300 * SOL, 400 * SOL];
        let (mut state, validators, _) = fixture(&actives);
        // Score order: v2 best, then v0, then v3, then v1.
        state.sorted_score_indices[..4].copy_from_slice(&[2, 0, 3, 1]);
        // Unstake order is independent: v1 worst-raw-score, so it is unstaked first.
        state.sorted_raw_score_indices[..4].copy_from_slice(&[3, 0, 2, 1]);

        let rows = rows_for(
            &state,
            &validators,
            &DirectedStakeMeta::default(),
            4_000 * SOL,
            0,
        );

        assert_eq!(rows[2].rank, Some(0));
        assert_eq!(rows[0].rank, Some(1));
        assert_eq!(rows[3].rank, Some(2));
        assert_eq!(rows[1].rank, Some(3));

        // raw_rank is the position in sorted_raw_score_indices, untouched by funding order.
        assert_eq!(rows[3].raw_rank, Some(0));
        assert_eq!(rows[1].raw_rank, Some(3));
    }

    /// A validator outside the delegation set has no rank, a zero target, and its whole balance
    /// counts as reducible supply.
    #[test]
    fn build_rows_marks_validators_outside_the_set() {
        let actives = [500 * SOL, 500 * SOL, 500 * SOL];
        let (mut state, validators, _) = fixture(&actives);
        // Drop v1 from the set.
        state.delegations[1] = Delegation::new(0, 1);
        state.sorted_score_indices[..3].copy_from_slice(&[0, 2, 1]);

        let rows = rows_for(
            &state,
            &validators,
            &DirectedStakeMeta::default(),
            1_500 * SOL,
            0,
        );

        assert!(!rows[1].in_set);
        assert_eq!(rows[1].rank, None);
        assert_eq!(rows[1].target, 0);
        assert_eq!(rows[1].excess(), 500 * SOL);
        // Ranks skip the non-member entirely.
        assert_eq!(rows[0].rank, Some(0));
        assert_eq!(rows[2].rank, Some(1));
    }

    /// `current` is what targets are measured against: active stake minus directed minus base.
    #[test]
    fn build_rows_nets_directed_stake_and_base_lamports_out_of_current() {
        let actives = [1_000 * SOL, 1_000 * SOL];
        let base = 3_282_880u64;
        let (state, validators, _) = fixture(&actives);

        let mut meta = directed_pool_only(400 * SOL);
        meta.directed_stake_lamports[0] = 400 * SOL;

        let rows = rows_for(&state, &validators, &meta, 2_400 * SOL, base);

        assert_eq!(rows[0].active, 1_000 * SOL);
        assert_eq!(rows[0].directed, 400 * SOL);
        assert_eq!(rows[0].current, 1_000 * SOL - 400 * SOL - base);
        // v1 holds no directed stake, so only base comes off.
        assert_eq!(rows[1].current, 1_000 * SOL - base);
    }

    // ---------------------------------------------------------------- aggregate

    #[test]
    fn aggregate_orders_the_funding_queue_by_rank_and_totals_correctly() {
        let target = 1_000 * SOL;
        let actives = [
            1_500 * SOL, // over by 500
            50 * SOL,    // under by 950 -> a "new entrant" (>90% of target)
            990 * SOL,   // under by 10, below the minimum delegation -> ignored
            600 * SOL,   // under by 400
        ];
        let (mut state, validators, _) = fixture(&actives);
        // Funding order v3, v1, v0, v2 — deliberately not index order.
        state.sorted_score_indices[..4].copy_from_slice(&[3, 1, 0, 2]);
        let rows = rows_for(
            &state,
            &validators,
            &DirectedStakeMeta::default(),
            4_000 * SOL,
            0,
        );
        assert_eq!(
            rows[0].target, target,
            "fixture should give a 1000 SOL target"
        );

        let agg = aggregate(&rows, 100 * SOL);

        assert_eq!(agg.n, 4);
        assert_eq!(agg.target_stake, target);
        // v2's 10 SOL shortfall is under the minimum delegation, so it is not queued.
        assert_eq!(agg.under, vec![3, 1]);
        assert_eq!(agg.total_shortfall, 950 * SOL + 400 * SOL);
        assert_eq!(agg.over_target_count, 1);
        assert_eq!(agg.unstakeable, 500 * SOL);
        // Only v1 is short by more than 90% of a target.
        assert_eq!(agg.new_entrants, 1);
        assert_eq!(agg.new_entrant_demand, 950 * SOL);
    }

    #[test]
    fn aggregate_counts_out_of_set_balances_as_reducible_supply() {
        let actives = [1_200 * SOL, 300 * SOL];
        let (mut state, validators, _) = fixture(&actives);
        state.delegations[1] = Delegation::new(0, 1);
        let rows = rows_for(
            &state,
            &validators,
            &DirectedStakeMeta::default(),
            1_500 * SOL,
            0,
        );
        // Target is 1500/2 = 750 for the remaining member; v0 is 450 over.
        let agg = aggregate(&rows, SOL);
        assert_eq!(agg.unstakeable, 450 * SOL + 300 * SOL);
    }

    #[test]
    fn aggregate_on_an_empty_set_is_zeroed_rather_than_panicking() {
        let actives = [500 * SOL];
        let (mut state, validators, _) = fixture(&actives);
        state.delegations[0] = Delegation::new(0, 1);
        let rows = rows_for(
            &state,
            &validators,
            &DirectedStakeMeta::default(),
            500 * SOL,
            0,
        );
        let agg = aggregate(&rows, SOL);
        assert_eq!(agg.n, 0);
        assert_eq!(agg.target_stake, 0);
        assert!(agg.under.is_empty());
    }

    // ---------------------------------------------------------------- project_schedule

    #[allow(clippy::too_many_arguments)]
    fn project(
        rows: &[Row],
        horizon: u64,
        scoring_cap: u64,
        scoring_used: u64,
        reserve: u64,
        deposits: u64,
    ) -> Schedule {
        project_schedule(
            rows,
            100, // current epoch
            110, // next cycle
            10,  // cycle length
            horizon,
            0,   // min_change: keep everything
            SOL, // minimum delegation
            scoring_cap,
            u64::MAX / 4, // instant cap: irrelevant here
            scoring_used,
            0,
            reserve,
            deposits,
        )
    }

    /// The projection's first epoch must agree with what the program actually does. This is the
    /// same equivalence check as the target tests, but applied to the projection rather than to
    /// a single rebalance call.
    #[test]
    fn projection_first_epoch_matches_the_program() {
        let total_pool = 4_000 * SOL;
        // v0 over target, v1/v2 at target, v3 empty. Target is 1000 SOL.
        let actives = [1_600 * SOL, 1_000 * SOL, 1_000 * SOL, 0];
        let reserve = 400 * SOL;

        let (mut state, validators, params) = fixture(&actives);
        let meta = DirectedStakeMeta::default();
        let rows = rows_for(&state, &validators, &meta, total_pool, 0);
        assert_eq!(rows[0].target, 1_000 * SOL);

        let scoring_cap = bps_of(total_pool, params.scoring_unstake_cap_bps);
        let sched = project(&rows, 1, scoring_cap, 0, reserve, 0);

        // What the program does, one instruction per validator, from the same starting state.
        let mut bytes = serialize_validator_list(&validators);
        let list = BigVec { data: &mut bytes };
        for (index, active) in actives.iter().enumerate() {
            let program = state
                .rebalance(
                    &meta, 100, index, &list, total_pool, reserve, *active, 0, 0, &params,
                )
                .expect("rebalance");
            let projected = sched
                .changes
                .iter()
                .find(|c| c.vote_account == validators[index].vote_account_address.to_string());

            match program {
                RebalanceType::Increase(lamports) => {
                    let c = projected.unwrap_or_else(|| {
                        panic!(
                            "program increased v{index} by {lamports} but projection had nothing"
                        )
                    });
                    assert!(c.change_sol > 0., "v{index}: sign mismatch");
                    assert_eq!(
                        (c.change_sol * 1e9).round() as u64,
                        lamports,
                        "v{index}: increase amount"
                    );
                }
                RebalanceType::Decrease(comp) => {
                    let c = projected.unwrap_or_else(|| {
                        panic!("program decreased v{index} but projection had nothing")
                    });
                    assert!(c.change_sol < 0., "v{index}: sign mismatch");
                    assert_eq!(
                        (-c.change_sol * 1e9).round() as u64,
                        comp.total_unstake_lamports,
                        "v{index}: decrease amount"
                    );
                }
                RebalanceType::None => assert!(
                    projected.is_none(),
                    "program did nothing for v{index} but projection moved it"
                ),
            }
        }
    }

    /// An exhausted budget stops rotation until the cycle resets, which is the single most
    /// common reason a validator sees no movement.
    #[test]
    fn projection_resumes_only_when_the_cycle_budget_resets() {
        let actives = [2_000 * SOL, 0];
        let (state, validators, _) = fixture(&actives);
        let rows = rows_for(
            &state,
            &validators,
            &DirectedStakeMeta::default(),
            2_000 * SOL,
            0,
        );
        let cap = 500 * SOL;

        // Budget fully spent, empty reserve: nothing can move before the reset at epoch 110.
        let sched = project(&rows, 15, cap, cap, 0, 0);
        let first = sched
            .changes
            .first()
            .expect("something must eventually move");
        assert_eq!(
            first.epoch, 110,
            "movement must wait for the cycle boundary at 110"
        );
        assert!(sched
            .per_epoch
            .iter()
            .filter(|e| e.epoch < 110)
            .all(|e| e.increased_sol == 0. && e.decreased_sol == 0.));
        assert!(sched.per_epoch.iter().any(|e| e.cycle_boundary));
    }

    /// Unstaked lamports are not spendable in the same epoch.
    #[test]
    fn projection_delays_funding_by_one_epoch_for_cooldown() {
        let actives = [2_000 * SOL, 0];
        let (state, validators, _) = fixture(&actives);
        let rows = rows_for(
            &state,
            &validators,
            &DirectedStakeMeta::default(),
            2_000 * SOL,
            0,
        );

        // Ample budget, empty reserve: v0 sheds first, v1 is funded from it the epoch after.
        let sched = project(&rows, 4, 10_000 * SOL, 0, 0, 0);
        let decrease = sched
            .changes
            .iter()
            .find(|c| c.change_sol < 0.)
            .expect("a decrease");
        let increase = sched
            .changes
            .iter()
            .find(|c| c.change_sol > 0.)
            .expect("an increase");
        assert_eq!(
            increase.epoch,
            decrease.epoch + 1,
            "funding must lag the unstake that pays for it by one epoch"
        );
    }

    /// With enough horizon and budget, every set member should reach target — no permanent
    /// shortfall. This is the property whose apparent violation exposed the denominator bug.
    #[test]
    fn projection_converges_with_no_permanent_shortfall() {
        let actives = [3_000 * SOL, 0, 0, 0];
        let (state, validators, _) = fixture(&actives);
        let rows = rows_for(
            &state,
            &validators,
            &DirectedStakeMeta::default(),
            3_000 * SOL,
            0,
        );
        let target = rows[0].target;

        let sched = project(&rows, 30, 10_000 * SOL, 0, 0, 0);

        for r in &sched.per_validator {
            let remaining = r.total_due_sol - r.projected_sol;
            assert!(
                remaining.abs() < 2.0,
                "{} still short by {remaining} SOL of target {}",
                r.vote_account,
                lamports_to_sol(target)
            );
        }
    }

    /// `--min-change-sol` must drop small movers from the report without distorting the
    /// queue, which is computed over every validator.
    #[test]
    fn projection_filters_small_movers_only_from_the_report() {
        let actives = [1_010 * SOL, 995 * SOL, 995 * SOL, 1_000 * SOL];
        let (state, validators, _) = fixture(&actives);
        let rows = rows_for(
            &state,
            &validators,
            &DirectedStakeMeta::default(),
            4_000 * SOL,
            0,
        );

        let unfiltered = project(&rows, 5, 10_000 * SOL, 0, 100 * SOL, 0);
        let filtered = project_schedule(
            &rows,
            100,
            110,
            10,
            5,
            50 * SOL, // min_change
            SOL,
            10_000 * SOL,
            u64::MAX / 4,
            0,
            0,
            100 * SOL,
            0,
        );

        assert!(!unfiltered.per_validator.is_empty());
        assert!(
            filtered.per_validator.is_empty(),
            "every mover here is under 50 SOL, so none should be reported"
        );
        // Per-epoch totals are computed before filtering, so they still show the movement.
        let unfiltered_moved: f64 = unfiltered.per_epoch.iter().map(|e| e.decreased_sol).sum();
        let filtered_moved: f64 = filtered.per_epoch.iter().map(|e| e.decreased_sol).sum();
        assert_eq!(unfiltered_moved, filtered_moved);
    }

    /// The integration test that would have caught the target-denominator bug: run the real
    /// `StewardStateV2::rebalance` and confirm the amount it hands out equals the target this
    /// module computes — and is NOT the larger figure the gross pool would produce.
    #[test]
    fn program_target_matches_model_and_nets_out_directed_stake() {
        let total_pool = 4_000 * SOL;
        let directed = 400 * SOL;
        // v0 empty; v1..v3 exactly at the 900 SOL target; remainder sits in the reserve.
        let actives = [0, 900 * SOL, 900 * SOL, 900 * SOL];
        let reserve = 1_300 * SOL;

        let (mut state, validators, params) = fixture(&actives);
        let meta = directed_pool_only(directed);
        let mut bytes = serialize_validator_list(&validators);
        let list = BigVec { data: &mut bytes };

        // What the model says the target is.
        let model_pool = delegatable_pool_lamports(total_pool, directed, 0, actives.len() as u64);
        let model_target = model_pool / actives.len() as u64;
        assert_eq!(model_target, 900 * SOL);

        // What the gross pool would have said — the bug.
        let gross_target = total_pool / actives.len() as u64;
        assert_eq!(gross_target, 1_000 * SOL);

        // The program is handed the pool net of directed stake, exactly as
        // instructions/rebalance.rs:259 does it.
        let undirected_pool = total_pool - meta.total_staked_lamports();
        let result = state
            .rebalance(
                &meta,
                100,
                0,
                &list,
                undirected_pool,
                reserve,
                actives[0],
                0,
                0,
                &params,
            )
            .expect("rebalance should succeed");

        match result {
            RebalanceType::Increase(lamports) => {
                assert_eq!(
                    lamports, model_target,
                    "program funded {lamports} but the model predicted {model_target}"
                );
                assert_ne!(
                    lamports, gross_target,
                    "program agreed with the gross-pool target, which would mean the directed \
                     subtraction is not happening where this module thinks it is"
                );
            }
            other => panic!("expected an Increase to target, got {other:?}"),
        }
    }

    /// The mirror case: an over-target validator is unstaked down to the model's target, not to
    /// the gross-pool target.
    #[test]
    fn program_unstakes_down_to_the_model_target() {
        let total_pool = 4_000 * SOL;
        let directed = 400 * SOL;
        // v0 over target by 600 SOL; the rest under, so they consume none of the budget.
        let actives = [1_500 * SOL, 700 * SOL, 700 * SOL, 700 * SOL];

        let (mut state, validators, params) = fixture(&actives);
        let meta = directed_pool_only(directed);
        let mut bytes = serialize_validator_list(&validators);
        let list = BigVec { data: &mut bytes };

        let model_target = delegatable_pool_lamports(total_pool, directed, 0, actives.len() as u64)
            / actives.len() as u64;
        let model_excess = actives[0] - model_target;
        assert_eq!(model_excess, 600 * SOL);

        let undirected_pool = total_pool - meta.total_staked_lamports();
        let result = state
            .rebalance(
                &meta,
                100,
                0,
                &list,
                undirected_pool,
                400 * SOL,
                actives[0],
                0,
                0,
                &params,
            )
            .expect("rebalance should succeed");

        match result {
            RebalanceType::Decrease(components) => {
                assert_eq!(
                    components.total_unstake_lamports, model_excess,
                    "program unstaked {} but the model predicted {model_excess}",
                    components.total_unstake_lamports
                );
                // Categorised as scoring, since internal balances match actuals.
                assert_eq!(components.scoring_unstake_lamports, model_excess);
                assert_eq!(components.stake_deposit_unstake_lamports, 0);
                assert_eq!(components.instant_unstake_lamports, 0);
            }
            other => panic!("expected a Decrease to target, got {other:?}"),
        }
    }

    /// A validator's own directed stake is excluded from its progress toward target. With 1000
    /// SOL active of which 400 is directed, the program must see 600 undirected against a 900
    /// target and INCREASE — the opposite of what it would do if directed stake counted.
    #[test]
    fn a_validators_own_directed_stake_does_not_count_toward_its_target() {
        let total_pool = 4_000 * SOL;
        let directed = 400 * SOL;
        let actives = [900 * SOL, 900 * SOL, 900 * SOL, 1_000 * SOL];
        let directed_index = 3usize;

        let (mut state, validators, params) = fixture(&actives);

        let mut meta = directed_pool_only(directed);
        // Wire the directed stake to v3 specifically this time.
        meta.directed_stake_lamports[directed_index] = directed;
        meta.directed_stake_meta_indices[directed_index] = 0;
        meta.targets[0].vote_pubkey = validators[directed_index].vote_account_address;

        let mut bytes = serialize_validator_list(&validators);
        let list = BigVec { data: &mut bytes };

        let model_target = delegatable_pool_lamports(total_pool, directed, 0, actives.len() as u64)
            / actives.len() as u64;
        let undirected = actives[directed_index] - directed;
        assert!(
            undirected < model_target && actives[directed_index] > model_target,
            "fixture must be over target on gross stake but under it on undirected stake"
        );
        let model_shortfall = model_target - undirected;

        let undirected_pool = total_pool - meta.total_staked_lamports();
        let result = state
            .rebalance(
                &meta,
                100,
                directed_index,
                &list,
                undirected_pool,
                400 * SOL,
                actives[directed_index],
                0,
                0,
                &params,
            )
            .expect("rebalance should succeed");

        match result {
            RebalanceType::Increase(lamports) => assert_eq!(
                lamports, model_shortfall,
                "program funded {lamports}, model predicted {model_shortfall}"
            ),
            other => panic!(
                "expected an Increase — directed stake must not count toward target — got {other:?}"
            ),
        }
    }

    /// Regression test for the bug that produced a phantom pool-wide shortfall: targets must
    /// divide the pool NET of directed stake, matching instructions/rebalance.rs:259.
    #[test]
    fn targets_divide_the_undirected_pool() {
        let total = 9_943_825 * SOL;
        let directed = 380_068 * SOL;
        let base = 3_283_000; // minimum_delegation + stake_rent
        let n_validators = 691;
        let set_size = 312;

        let pool = delegatable_pool_lamports(total, directed, base, n_validators);

        // Every term must be present. Dropping any one of them was the original bug.
        assert_eq!(pool, total - directed - base * n_validators);

        let target = pool / set_size;
        let gross_target = (total - base * n_validators) / set_size;

        // Using the gross pool overstates every target...
        assert!(gross_target > target);
        // ...and the aggregate overstatement is the entire directed balance, which is why it
        // looked like a structural shortfall rather than an arithmetic slip.
        let aggregate = (gross_target - target) * set_size;
        assert!(
            aggregate.abs_diff(directed) < set_size,
            "aggregate overstatement {aggregate} should equal directed total {directed} \
             up to integer-division rounding"
        );

        // Ballpark guard so a units error (lamports vs SOL) still fails loudly.
        assert!((30_000..31_000).contains(&(target / SOL)));
    }

    #[test]
    fn zero_directed_stake_leaves_the_pool_unchanged() {
        let total = 1_000_000 * SOL;
        let base = 3_283_000;
        assert_eq!(
            delegatable_pool_lamports(total, 0, base, 100),
            total - base * 100
        );
    }

    #[test]
    fn delegatable_pool_saturates_rather_than_underflowing() {
        // Directed larger than the pool is nonsensical, but must not panic.
        assert_eq!(delegatable_pool_lamports(10, 999, 1, 5), 0);
    }

    #[test]
    fn funded_within_this_cycle_is_floored_at_one_epoch() {
        // Supply already covers the need, so the only wait is cooldown.
        assert_eq!(epochs_until_funded(100, 1_000, 5_000, 8, 10), Some(1));
    }

    #[test]
    fn zero_need_returns_zero() {
        assert_eq!(epochs_until_funded(0, 0, 0, 8, 10), Some(0));
    }

    #[test]
    fn spills_into_the_next_cycle() {
        // 1_500 needed, 500 available now, 1_000 per future cycle -> exactly one future cycle,
        // so funding lands when the budget resets.
        assert_eq!(epochs_until_funded(1_500, 500, 1_000, 8, 10), Some(8));
    }

    #[test]
    fn spills_across_multiple_cycles() {
        // 2_500 needed, 500 now, 1_000 per cycle -> 2 future cycles: reset + one full cycle.
        assert_eq!(epochs_until_funded(2_500, 500, 1_000, 8, 10), Some(18));
        // 3_500 needed -> 3 future cycles.
        assert_eq!(epochs_until_funded(3_500, 500, 1_000, 8, 10), Some(28));
    }

    #[test]
    fn no_supply_means_no_estimate() {
        assert_eq!(epochs_until_funded(1_000, 0, 0, 8, 10), None);
    }

    #[test]
    fn exhausted_budget_and_empty_reserve_is_reported_as_such() {
        let row = Row {
            vote_account: Pubkey::new_unique(),
            in_set: true,
            active: 0,
            marked_instant_unstake: false,
            raw_rank: None,
            denominator: 1,
            rank: Some(5),
            current: 0,
            directed: 0,
            transient: 0,
            target: 1_000_000_000,
        };
        assert_eq!(
            classify(Some(&row), 1_000_000, 0, 0, 500_000_000),
            LimitingFactor::ScoringChurnBudgetExhausted
        );
    }

    #[test]
    fn in_flight_stake_reports_cooldown() {
        let row = Row {
            vote_account: Pubkey::new_unique(),
            in_set: true,
            active: 0,
            marked_instant_unstake: false,
            raw_rank: None,
            denominator: 1,
            rank: Some(5),
            current: 0,
            directed: 0,
            transient: 500_000_000,
            target: 1_000_000_000,
        };
        assert_eq!(
            classify(Some(&row), 1_000_000, 0, 0, 0),
            LimitingFactor::AwaitingCooldown
        );
    }

    #[test]
    fn at_target_within_minimum_delegation() {
        let row = Row {
            vote_account: Pubkey::new_unique(),
            in_set: true,
            active: 0,
            marked_instant_unstake: false,
            raw_rank: None,
            denominator: 1,
            rank: Some(5),
            current: 999_500_000,
            directed: 0,
            transient: 0,
            target: 1_000_000_000,
        };
        assert_eq!(
            classify(Some(&row), 1_000_000, 100, 100, 0),
            LimitingFactor::AtTarget
        );
    }

    #[test]
    fn nobody_ahead_and_reserve_funded_means_next_rebalance() {
        let row = Row {
            vote_account: Pubkey::new_unique(),
            in_set: true,
            active: 0,
            marked_instant_unstake: false,
            raw_rank: None,
            denominator: 1,
            rank: Some(0),
            current: 0,
            directed: 0,
            transient: 0,
            target: 1_000_000_000,
        };
        // Top of the queue with a funded reserve: not "blocked by queue position".
        assert_eq!(
            classify(Some(&row), 1_000_000, 10_000_000_000, 5_000_000_000, 0),
            LimitingFactor::NextRebalance
        );
    }

    /// `sol_ahead` must be derived from rank, not from position within the under-target list,
    /// or an at-target validator sums the entire list and reports the pool-wide shortfall.
    #[test]
    fn ahead_of_rank_ignores_lower_ranked_validators() {
        let shortfalls = [(0usize, 100u64), (5, 200), (10, 400)];
        let ahead_of = |rank: usize| -> u64 {
            shortfalls
                .iter()
                .filter(|(r, _)| *r < rank)
                .map(|(_, s)| *s)
                .sum()
        };
        assert_eq!(ahead_of(0), 0);
        assert_eq!(ahead_of(5), 100);
        assert_eq!(ahead_of(10), 300);
        // A validator ranked between the entries still only counts those above it.
        assert_eq!(ahead_of(7), 300);
    }

    #[test]
    fn queue_position_when_others_are_ahead() {
        let row = Row {
            vote_account: Pubkey::new_unique(),
            in_set: true,
            active: 0,
            marked_instant_unstake: false,
            raw_rank: None,
            denominator: 1,
            rank: Some(200),
            current: 0,
            directed: 0,
            transient: 0,
            target: 1_000_000_000,
        };
        // Budget and reserve both have room, but 500 SOL is queued ahead of only 1 SOL of reserve.
        assert_eq!(
            classify(
                Some(&row),
                1_000_000,
                10_000_000_000,
                1_000_000_000,
                500_000_000_000
            ),
            LimitingFactor::QueuePosition
        );
    }
}
