# StakeNet "When Do I Get Stake?" — ETA Endpoint Spec

*Design spec for a validator-facing stake-arrival estimator. Derived entirely from public
on-chain state; no Steward program changes required. Every figure below is live CLI output
from mainnet at epoch 1012.*

---

## 0. What this can and cannot tell a validator

**It cannot promise stake.** The distinction to hold onto, and the one that has to survive into
any validator-facing wording:

| Confidence | What |
| :- | :- |
| **Firm** | Eligibility, the 1/N target, rank, SOL queued ahead, remaining churn budget and its reset epoch. Read directly from on-chain state. |
| **Reasonable** | What a validator would receive next epoch given the reserve exactly as it stands. |
| **Speculative** | Anything past the next cycle boundary. The set is re-scored, N changes, every target moves with TVL. |
| **Not modelled** | Deposit and withdrawal rate, changes in this or other validators' performance and commissions, instant-unstake events elsewhere. |

A projection 10 or 20 epochs out can differ substantially from what actually happens. The
implementation emits this caveat on every invocation — as a banner at the top *and* bottom of the
human output, and as a `disclaimer` array plus `speculative_beyond_epoch` in the JSON, so a
consumer cannot render the numbers without it. Schedule rows at or past the re-scoring boundary
are marked `(?)`.

The intended default answer to "when do I get stake" is therefore *"here is what is certain, here
is what it depends on, and here is why no one can give you a date"* — not a number.

## 1. Purpose and scope

Validators can already see **whether** they are eligible on the StakeNet steward page. The
unanswered question is **"in how many epochs should I expect stake, given today's
conditions?"** — and, for validators already in the set but below target, "why am I not at my
full share yet?"

In scope:

- Epochs-to-first-stake and epochs-to-target for a given vote account.
- Current TVL and the resulting target stake per eligible validator.
- The validator's position in the funding queue and how much SOL must be funded ahead of them.
- Remaining churn budget for the current cycle and the epoch it resets.

Out of scope (already served by the existing UI):

- Eligibility pass/fail and which filter is failing.
- Score and rank display.

## 2. Why this is a queue, not a schedule

Stake does not arrive on a timetable. It arrives when two independent things line up:
**supply exists in the reserve**, and **the validator is next in line for it**.

Increases are funded exclusively from the stake pool's reserve stake account. In each epoch's
rebalance step, the Steward walks validators in **descending score order**, giving each one
the smaller of (its shortfall to target) and (whatever reserve is left). A validator at rank
300 receives nothing until every under-target validator ranked above it has been topped up.

Supply reaches the reserve from exactly three places:

1. **Net new JitoSOL deposits** — exogenous, driven by demand for JitoSOL.
2. **Stake unstaked from over-target validators**, which is what funds rotation. Hard-capped
   at 7.5% of the undirected pool per 10-epoch cycle (`scoring_unstake_cap_bps = 750`).
3. **Stake recovered from instant-unstaked or removed validators**, under their own caps.

Unstaked lamports take roughly one epoch to cool down before they are spendable, so supply
created in epoch N is deliverable in epoch N+1 at the earliest.

The consequence that matters: **when the scoring-unstake budget for a cycle is spent,
rotation stops until the cycle resets**, regardless of how far below target anyone is. This
is the single biggest driver of "I'm eligible but nothing is happening," and it is fully
observable on-chain.

## 3. The model

### 3.1 Definitions

```
N              = number of validators in the delegation set (delegation denominator)
undirected     = stake pool total_lamports - directed_stake_meta.total_staked_lamports()
pool_adj       = undirected - (minimum_delegation + stake_rent) * n_validators
target         = pool_adj / N                       # equal share, same for every member
cur(v)         = active stake of v, minus base lamports, minus directed stake
short(v)       = max(0, target - cur(v))            # v's shortfall
rank(v)        = index of v in sorted_score_indices, restricted to set members
ahead(v)       = sum of short(u) for all u in set with rank(u) < rank(v)
need(v)        = ahead(v) + short(v)                # cumulative funding to reach v's target
```

**The directed-stake subtraction is not optional.** `instructions/rebalance.rs:259` passes
`total_pool_lamports - directed_stake_meta.total_staked_lamports()` into `rebalance`, so
targets are computed on the undirected pool. Divide the gross pool by N instead and every
target is overstated by `directed_total / N` — currently 1,218 SOL per
validator — which aggregates to a phantom pool-wide shortfall equal to the entire directed
balance. This spec's earlier draft made exactly that error.

Because the consequence is a silently wrong answer rather than an error, the implementation treats an unreadable `DirectedStakeMeta` as fatal rather than defaulting to zero directed stake. `--assume-no-directed-stake` overrides this, and is only correct for a deployment that genuinely has none.

### 3.2 Supply per cycle

```
scoring_cap        = pool_adj * 750 / 10_000
scoring_remaining  = scoring_cap - scoring_unstake_total     # both on-chain
unstakeable        = sum of excess over target across all validators
                     + all stake held by validators no longer in the set

this_cycle_supply  = min(scoring_remaining, unstakeable) + usable_reserve
                     + deposits_remaining_this_cycle
future_cycle_supply= min(scoring_cap, unstakeable_then)      + deposits_per_cycle
```

Note that `scoring_cap` is recomputed from the *current* undirected pool every time
`rebalance` runs, while `scoring_unstake_total` accumulates across the cycle. If directed
stake grows mid-cycle the cap shrinks beneath the running total, so `scoring_remaining` can
saturate at zero with the total slightly exceeding the cap — which is the case right now
(717,368 used against a 717,230 cap). Clamp, don't assert.

### 3.3 The estimate

Walk cycles forward, drawing down `need(v)` against the supply available in each, and report
the epoch at which cumulative supply first covers it:

```
remaining = need(v)
remaining -= min(this_cycle_supply, remaining)
if remaining <= 0:
    eta = 1 epoch          # floored by cooldown
else:
    cycles = ceil(remaining / future_cycle_supply)
    eta    = epochs_until_reset + (cycles - 1) * num_epochs_between_scoring
```

`deposits_per_epoch` is the only input not readable from a single account fetch. Derive it
from a trailing average of pool `total_lamports` deltas — the keeper already emits pool TVL
per epoch, so this is a query against existing metrics, not new instrumentation. Report the
ETA as a band across a low / high deposit assumption rather than a single number.

## 4. Inputs

| Source | Fields used | Purpose |
| :- | :- | :- |
| Steward config<br>`jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv` | `scoring_unstake_cap_bps`, `instant_unstake_cap_bps`, `stake_deposit_unstake_cap_bps`, `num_epochs_between_scoring`, `num_delegation_validators`, `undirected_stake_ceiling_lamports` | Caps, cycle length, reserve ceiling |
| Steward state<br>`9BAmGVLGxzqct6bkgjWmKSv3BFB6iKYXNBQp8GWG1LDY` | `next_cycle_epoch`, `delegations[]`, `sorted_score_indices[]`, `sorted_raw_score_indices[]`, `scores[]`, `instant_unstake`, the three unstake totals, `num_pool_validators` | Set membership, N, funding rank, unstake rank, budget consumption, reset date |
| JitoSOL stake pool<br>`Jito4APyf642JPZPx3hGc6WWJ8zPKtRbRs4P815Awbb` | `total_lamports`, `reserve_stake` | TVL and immediate reserve availability |
| Validator list<br>`3R3nGZpQs2aZo5FDQvd2MUQ6R7KhAPainds6uT6uE2mn` | `active_stake_lamports`, `transient_stake_lamports`, `vote_account_address` per entry | Per-validator current stake, in-flight detection |
| DirectedStakeMeta | `total_staked_lamports()` and `directed_stake_lamports[]` | Subtract directed stake from **both** the pool and each balance |
| Keeper metrics / TVL history | pool `total_lamports` per epoch | Trailing deposit rate for the ETA band |

The four accounts are a single `getMultipleAccounts` call. The whole computation runs in well
under a second and needs no indexer.

## 5. Output schema

```json
{
  "vote_account": "AuGwcQWqWuNbnrkgERSp8VN6kZeJiM4AZuLMrwbkzg9m",
  "as_of_epoch": 1012,
  "in_delegation_set": true,
  "pool": {
    "tvl_sol": 9943825,
    "undirected_pool_sol": 9563065,
    "delegation_set_size": 312,
    "set_size_cap": 400,
    "capacity_constrained": false,
    "target_stake_sol": 30651
  },
  "position": {
    "rank": 272,
    "of": 312,
    "current_stake_sol": 278,
    "shortfall_sol": 30373,
    "sol_ahead_in_queue": 24677
  },
  "budget": {
    "scoring_churn_cap_sol": 717230,
    "scoring_churn_used_sol": 717368,
    "scoring_churn_remaining_sol": 0,
    "pct_consumed": 100.0,
    "resets_at_epoch": 1020
  },
  "eta": {
    "first_stake": {
      "low_epochs": 1,
      "high_epochs": 1
    },
    "full_target": {
      "low_epochs": 1,
      "high_epochs": 1
    },
    "limiting_factor": "next_rebalance",
    "confidence": "medium"
  }
}
```

`limiting_factor` is the field that does the most work for the validator. It should be one of
a small enumerated set, each mapping to a plain-language explanation:

| `limiting_factor` | What the validator is told |
| :- | :- |
| `at_target` | "You are at your full share. Nothing further is owed." |
| `next_rebalance` | "Undelegated stake is available and nobody is ahead of you; expect stake at the next rebalance." |
| `queue_position` | "N SOL must be funded to higher-ranked validators before you. Improving your score moves you up." |
| `scoring_churn_budget_exhausted` | "Rotation for this cycle is used up. Budgets reset at epoch X." |
| `awaiting_cooldown` | "Stake has been unstaked for you and is cooling down; expect it next epoch." |
| `reserve_empty` | "There is no undelegated stake in the pool right now; arrival depends on new deposits." |
| `not_in_set` | "You are not in the current delegation set; the next selection is at epoch X." |

## 6. Worked example — mainnet at epoch 1012

| Quantity | Value |
| :- | :- |
| Pool TVL | 9,943,825 SOL |
| Undirected pool (the target denominator) | 9,563,065 SOL |
| Directed stake | 380,068 SOL across 2 validators |
| Validators in pool / in set (N) | 691 / 312 |
| Set size cap | 400 — not binding |
| Target per set member | **30,651 SOL** |
| Under target | 88 validators, 1,067,702 SOL of shortfall |
| Over target | 223 validators |
| Reducible excess in total | 1,005,199 SOL |
| Held by validators no longer in the set | 160,493 SOL across 68 validators |
| Scoring churn budget | 717,230 SOL per cycle |
| Scoring churn consumed | 717,368 SOL — 100%, exhausted |
| Fundable rest of this cycle | 62,187 SOL (reserve only) |
| Reserve stake balance | 62,189 SOL |
| Next cycle | epoch 1020 (8 epochs out) |

The shortfall is **strongly bimodal**, which is why a single pool-wide answer is useless:

| Cohort | Count | Needs | Typical shortfall |
| :- | :- | :- | :- |
| Existing members topping up | 54 | 27,474 SOL | 95 SOL (median) |
| New entrants with ~no stake | 34 | 1,040,228 SOL | 30,651 SOL (full target) |

The 34 new entrants occupy ranks 272–311 — the bottom of the set — so they
sit behind the entire top-up cohort **and** behind each other. Their individual answers differ
by an order of magnitude:

| Rank | Cumulative funding needed to reach them | Funded in | Epochs from 1012 |
| :- | :- | :- | :- |
| 272 (first new entrant) | 55,050 SOL | epoch 1013, from the current reserve | 1 |
| 284 | 391,306 SOL | epoch 1021, after the cycle-1020 unstake cools down | 9 |
| 298 | 729,249 SOL | epoch 1021, at the margin of that tranche | 9 |
| 311 (last) | 1,067,702 SOL | epoch 1031, needs the cycle-1030 budget too | 19 |

Meanwhile a rank-50 member short by ~95 SOL is answered completely differently: near the front
of the queue, needs a trivial amount, and is topped up in the first epoch the reserve holds
anything. Both facts are worth saying.

### 6.1 The reconciliation invariant

With directed stake correctly excluded from both sides, the accounting closes on reserve and
in-flight stake alone:

```
total_shortfall - total_excess = reserve + transient

verified against mainnet at epoch 1012:
  1,067,702 - 1,005,199 = 62,502 SOL
  reserve 62,189 + transient ~300      = ~62,489 SOL   OK
```

Assert this in tests. Asserting instead that shortfall equals reducible supply will fail, and
asserting that the difference includes directed stake means the target denominator is wrong.

Projected forward, every member of the delegation set reaches its 30,651 SOL
share, with only a few hundred SOL of sub-minimum-delegation dust outstanding. **There is no
structural gap and no permanently unfundable cohort.**

## 7. Presentation guidance

- **Lead with the limiting factor, not the number.** "Rotation budget for this cycle is spent;
  resets epoch 1020" is more actionable and more honest than "9 epochs."
- **Always show a range.** A point estimate will be treated as a promise and will be wrong the
  moment deposit flow changes.
- **Show the queue explicitly.** "24,677 SOL must be funded
  ahead of you" makes the mechanism legible and makes the wait feel like arithmetic rather than
  a black box.
- **Say that rank is improvable.** Funding order is by score, so lowering inflation or MEV
  commission moves a validator up the queue — not merely into the set. This is the one lever
  validators actually control, and today's data puts it at ~18 epochs of difference between the
  top and bottom of the new-entrant cohort.
- **Distinguish first-stake from full-target.** A validator may receive a partial delegation
  several epochs before reaching 1/N.

## 8. Known limitations

- **Deposit flow is the dominant uncertainty.** In a strong inflow week, new entrants are
  funded from deposits without touching the churn budget at all, and the ETA collapses. The
  band must be wide enough to express this.
- **Instant-unstake events are unpredictable** and cut both ways: a commission rug elsewhere
  frees supply early, but also consumes budget.
- **The set can change under you at a cycle boundary.** A projection past `next_cycle_epoch`
  assumes the set and N are stable; both move. Flag any ETA crossing a cycle boundary as
  re-derived at that boundary.
- **Within a cycle the unstake budget is not rate-limited per epoch**, so a projection spends
  it as soon as the cycle opens. Real movement inside a cycle depends on when cranks run.
  Treat per-cycle totals as reliable and the exact epoch within a cycle as indicative.
- **Directed staking competes for the same reserve** on a per-epoch clock and is modelled here
  only by subtracting directed lamports from the pool and from each balance.
- **Cranker liveness** is assumed. If rebalance instructions are not cranked for every
  validator in an epoch, some validators are simply skipped.
- **Sub-minimum-delegation deltas never move**, so a validator within a hair of target may
  never formally reach it. Treat "within minimum delegation of target" as at-target.

## 9. Implementation

**Shipped:** `steward-cli view-stake-eta` implements this model.

```bash
# per-validator ETA
steward-cli view-stake-eta --steward-config <cfg> --vote-account <pubkey>

# pool-wide, with the over-target and directed-stake listings
steward-cli view-stake-eta --steward-config <cfg> --top 12

# forward projection of stake movement, filtering out trivial changes
steward-cli view-stake-eta --steward-config <cfg> --schedule-epochs 24 --min-change-sol 5000
```

Remaining work:

1. **Covered.** 28 tests in `view_stake_eta.rs`, of which four drive the real
   `StewardStateV2::rebalance` and assert the program's own increase/decrease amounts match this
   model — including that a validator's own directed stake does not count toward its target, and
   that the projection's first epoch reproduces the program instruction-for-instruction. The
   rest cover rank derivation, directed/base netting, the pool aggregation, and the projection's
   cycle-boundary reset, cooldown lag, convergence and filtering.

   Verified by mutation rather than by coverage alone. Each of these faults, injected
   deliberately, fails at least one test: dropping the directed subtraction (5 tests), deriving
   rank from index order instead of score order (2), removing the one-epoch cooldown (2),
   never resetting the cycle budget (1), and ignoring the minimum-delegation floor when
   queueing (1).
2. Have the keeper emit the aggregate fields once per epoch (TVL, undirected pool, N, target,
   budget consumed, total shortfall, size of the new-entrant cohort). That gives the trailing
   deposit rate for free and makes the pool-level story chartable.
3. Expose per-validator ETA on the steward page next to the existing eligibility display, led
   by `limiting_factor`.

Nothing here requires a program upgrade or a governance change. Every input is already
on-chain and public today.
