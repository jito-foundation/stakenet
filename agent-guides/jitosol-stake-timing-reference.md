# JitoSOL Delegation Timing — Reference for AI Agents

This document gives an AI agent everything it needs to work out where a validator stands in the
JitoSOL delegation queue, using only public on-chain data.

It is written to be read by a model, not a person. Pair it with the prompt in
`jitosol-stake-timing-prompt.md`.

---

## 0. The single most important instruction

**Do not produce a date or an epoch count for when stake will arrive.** It cannot be derived. The
inputs that would make it meaningful — the rate of deposits into and withdrawals out of JitoSOL,
every other validator's performance and commission decisions, and the re-scoring that happens at
each cycle boundary — are not knowable in advance. A stake pool is a dynamic system; a number that
looks like a forecast will be read as a promise and will usually be wrong.

Produce instead:

1. **The firm facts** — eligibility, target, rank, queue position, remaining budget, reset epoch.
2. **The binding constraint** — the specific reason stake is not arriving right now.
3. **A next-epoch statement** — what would arrive next epoch given the reserve as it stands.
4. **An explicit refusal** to extrapolate past the next scoring event, with the reason.

### Every number in this document is illustrative

Thresholds, window lengths, cycle length, set size, epoch-progress triggers and caps are all
DAO-settable parameters held in the Steward config. Where this document shows a value it is marked
"currently" and is there to make the prose readable — it is **not** a constant and may already be
stale by the time you read it.

**Backtick names are authoritative; bare numbers are not.** Read every parameter you rely on from
the live config, and if a figure you compute depends on one, say which value you read rather than
quoting this document.

## 1. How delegation works

- StakeNet redelegates on a **cycle** of `num_epochs_between_scoring` epochs (currently 10).
- Once `compute_score_epoch_progress` through the epoch that opens a cycle (currently 0.50, i.e.
  about halfway), every validator is scored and the delegation set is chosen. No validator *joins*
  the set again until the next scoring event — but see the caveat below: the set can still shrink.
- Later in each epoch, once `instant_unstake_epoch_progress` through it (currently 0.90),
  emergency-unstake criteria are evaluated. A validator marked for instant unstake has its target
  dropped to zero for the remainder of the cycle.
- The set holds up to `num_delegation_validators` (currently 400). Every member gets an **equal
  share**: 1/N of the undirected pool, where N is the set size.
- Rank decides *whether* you are in the set and *what order* stake reaches you. It does not decide
  how much. Everyone in the set has the same target.

**The set is not frozen for the cycle, and N is not constant.** When a validator with a live
delegation is instant-unstaked, the program zeroes its numerator and decrements **every** other
member's denominator, so N falls and every remaining member's target *rises* mid-cycle. This is not
rare: at epoch 1012, 160 validators were marked for instant unstake and N had already moved from 312
to 311 partway through the cycle. Re-read N and the target rather than caching them, and never assume
a target computed earlier in a cycle is still current.

**Eligibility is pass/fail and comes first.** Failing any single filter sets the score to 0, which
means no delegation for the whole cycle — not a reduced amount:

| Filter | Threshold |
| :- | :- |
| Inflation commission | ≤ `commission_threshold`% in each of the last `commission_range` epochs |
| MEV commission | ≤ `mev_commission_bps_threshold` bps in each of the last `mev_commission_range` epochs |
| Historical commission | ≤ `historical_commission_threshold`% across all tracked history |
| Vote credits | > `scoring_delinquency_threshold_ratio` of cluster blocks in **every** epoch of the window |
| Superminority | must not be in it |
| Blacklist | must not be on it |
| Tip Distribution merkle root upload authority | must be TipRouter or legacy Jito |
| BAM | connected for ≥ `jito_bam_minimum_epochs` of the trailing `jito_bam_window_epochs + 1` epochs |

Running the BAM client is therefore a binary requirement for the pool: not connected means score
zero means no stake.

**The lookback windows are configuration, not constants.** `commission_range`,
`mev_commission_range` and `epoch_credits_range` are three *separate* DAO-settable parameters. They
happen to hold the same value at the time of writing, which makes it tempting to talk about "the
30-epoch window" — there is no such single window. Read each from the live config.

### Computing when a failure expires

This is the one genuinely exact forward-looking answer available, so get it right. The history
windows are inclusive of both endpoints, so an offending data point in epoch `E` stops counting at:

```
first_clear_epoch = E + <that filter's range parameter> + 1
```

Use the range that belongs to the filter that actually failed:

| Failing filter | Range parameter | Window the program reads |
| :- | :- | :- |
| Inflation commission | `commission_range` | `[current - commission_range, current]` |
| MEV commission | `mev_commission_range` | `[current - mev_commission_range, current]` |
| Vote credits / delinquency | `epoch_credits_range` | `[current - epoch_credits_range, current - 1]` |
| Historical commission | **none — see below** | first reliable epoch … `current` |

Worked example, using whatever `commission_range` actually is rather than an assumed value: if it
reads 30 and the validator ran commission above the threshold in epoch 995, then at epoch 1025 the
window is `[995, 1025]` and still contains the offence. At epoch 1026 it is `[996, 1026]` and does
not. So `first_clear_epoch = 995 + 30 + 1 = 1026`. Off-by-one errors here are easy — check that the
epoch you report would actually exclude the offending epoch.

**A historical-commission failure does not expire.** That filter takes the maximum over all tracked
history rather than a rolling window, so once a validator has exceeded
`historical_commission_threshold` in any epoch after the first reliable epoch, it fails
permanently under the current parameters. Do not offer a clear-by epoch for it. Say plainly that
there is none, and that only a governance change to the threshold would alter it.

Two further filters exist in the program but are currently disabled by parameter values
(`priority_fee_scoring_start_epoch = 65535` and `priority_fee_max_commission_bps = 10000`). Check
the live config rather than assuming.

**Ranking**, among eligible validators, is a strict four-tier hierarchy — a difference in a higher
tier always dominates every lower one:

1. Inflation commission (lowest preferred; uses the **maximum** over the window)
2. MEV commission (lowest preferred; uses the **average** over the window)
3. Validator age (older preferred; epochs with non-zero vote credits)
4. Vote credits ratio (tiebreaker only)

## 2. Why arrival is a queue

Increases are funded from **one** place: the pool's reserve stake account. Each epoch the Steward
walks validators in **descending score order** and gives each the smaller of (its shortfall to
target) and (whatever reserve remains).

So a validator's wait is set by two quantities:

- **`sol_ahead`** — the summed shortfall of every under-target set member ranked above it.
- **Supply** — the reserve, fed by new deposits, by stake unstaked from over-target validators, and
  by stake recovered from validators that left the set.

Unstaked SOL takes about one epoch to cool down before it can be re-delegated. An unstake today is
not spendable today.

Rotation is deliberately throttled: only `scoring_unstake_cap_bps` (currently 750 = 7.5%) of the
pool may be unstaked **per cycle** for rebalancing. The cap exists because each redelegation costs
roughly two epochs of yield on the moved stake. **When that budget is spent, rotation stops until
the cycle resets, however far below target anyone is.** This is the most common reason an eligible
validator sees nothing happening, and it is directly observable.

Decreases are served in ascending **raw score** order (the score before eligibility filters), so
the weakest validators are unstaked first.

## 3. Directed stake — do not get this wrong

Under JIP-27, holders can direct the stake backing their JitoSOL to chosen validators. Directed
stake is excluded from the algorithmic maths on **both** sides:

- It is subtracted from pool TVL **before** dividing by N. Targets divide the *undirected* pool.
- It does not count toward satisfying a validator's target. Progress is measured on *undirected*
  balance only.

Two consequences an agent must handle:

- **If you divide gross TVL by N, every target is overstated** by `directed_total / N`, and you
  will invent a pool-wide shortfall equal to the entire directed balance. Always subtract directed
  stake first.
- **A validator holding directed stake can show a full-target shortfall while holding several times
  the target in total.** Report the undirected balance and the directed balance separately. Never
  describe such a validator as "near-empty" or "underfunded" without stating what it actually
  holds.

## 4. Where to read the data

Mainnet accounts:

| What | Address |
| :- | :- |
| Steward program | `Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8` |
| Steward config (parameters, thresholds) | `jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv` |
| Steward state (scores, ranks, delegations, budgets, next cycle) | `9BAmGVLGxzqct6bkgjWmKSv3BFB6iKYXNBQp8GWG1LDY` |
| JitoSOL stake pool (TVL, reserve) | `Jito4APyf642JPZPx3hGc6WWJ8zPKtRbRs4P815Awbb` |
| Validator history program | `HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa` |

### Preferred route: the released `steward-cli`

From [github.com/jito-foundation/stakenet](https://github.com/jito-foundation/stakenet). Build it
with `make build-release`, or equivalently:

```bash
cargo build --release --features jito-steward/idl-build,validator-history/idl-build -p steward-cli
```

The bare `cargo build --release -p steward-cli` **does not work** — it fails to compile without those
feature flags. The binary lands at `target/release/steward-cli`. These commands exist in the released
tool:

```bash
# Parameters and thresholds — every value referenced under "How delegation works"
steward-cli --json-rpc-url <RPC> view-config \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv

# Pool-level state: current state, validator count, next cycle epoch, unstake totals
steward-cli --json-rpc-url <RPC> view-state \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv

# One validator: rank, score, eligibility, target %, active and transient lamports
steward-cli --json-rpc-url <RPC> view-state \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv \
  --vote-account <YOUR_VOTE_ACCOUNT>

# Every validator — needed to compute queue position. Large output.
steward-cli --json-rpc-url <RPC> view-state \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv --verbose

# Directed stake totals and per-target amounts
steward-cli --json-rpc-url <RPC> view-directed-stake-meta \
  --steward-config jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
```

Add `--print-json` where supported for machine-readable output. Use a private RPC if you have one;
the public endpoint rate-limits and these commands fetch many accounts.

### Fallback: raw RPC

If the CLI is unavailable, the accounts can be read with `getAccountInfo` and decoded. The layouts
below were verified empirically against mainnet, but **they are not a stable interface** — a
program upgrade can move them, and the IDL in the repo is authoritative. Sanity-check every decoded
value for plausibility before using it.

- **Config**: parameters begin at byte `8 + (5 × 32) + (313 × 8) = 2672` (discriminator, five
  pubkeys, then a 313-u64 blacklist bitmask). Fields follow the `Parameters` struct order.
- **Stake pool**: `total_lamports` at byte 258 (u64 LE); `reserve_stake` pubkey at byte 162.
- **Validator list**: 9-byte header, then 73 bytes per entry — `active_stake_lamports` (u64),
  `transient_stake_lamports` (u64), `last_update_epoch` (u64), `transient_seed_suffix` (u64),
  `unused` (u32), `validator_seed_suffix` (u32), `status` (u8), `vote_account_address` (32 bytes).
- **Steward state**: after the 8-byte discriminator and an 8-byte state tag, five
  `[u64; 5000]`-scale arrays in order — `validator_lamport_balances`, `scores`,
  `sorted_score_indices` (u16), `sorted_raw_score_indices` (u16), `delegations` (two u32 per
  entry) — then four 632-byte bitmasks, then `start_computing_scores_slot`, `current_epoch`,
  `next_cycle_epoch`, `num_pool_validators`, `scoring_unstake_total`, `instant_unstake_total`,
  `stake_deposit_unstake_total` as consecutive u64s.

## 4a. MANDATORY self-check before reporting anything

Byte offsets are not a stable interface, and a decode that lands one field off produces numbers
that look plausible and are wrong. **Run every check below before you report a single figure. If
any check fails, stop and tell the user the decode is unreliable — do not report results anyway.**

### Range checks — structural limits, not current settings

| Value | Must be |
| :- | :- |
| `num_pool_validators` | 1 … 5000 |
| `num_delegation_validators` | 1 … 5000 |
| `num_epochs_between_scoring` | 1 … 100 |
| any `*_cap_bps` | 0 … 10000 |
| `commission_threshold`, `historical_commission_threshold` | 0 … 100 |
| `scoring_delinquency_threshold_ratio` | 0.0 … 1.0 |
| `jito_bam_minimum_epochs` | ≤ `jito_bam_window_epochs` |
| `next_cycle_epoch` | ≥ current epoch, and within 100 epochs of it |
| `state.current_epoch` | equals the epoch from `getEpochInfo`, or exactly one behind during epoch maintenance |
| `target` | > 0 and < the whole pool |
| N | equals the number of set members you counted, and ≤ `num_delegation_validators` |

### The reconciliation invariant — the check that matters most

Across all validators in the pool:

```
(total_shortfall - total_excess) + clamp_loss  ==  reserve + transient

where  total_shortfall = Σ max(0, target - current)   over set members
       total_excess    = Σ max(0, current - target)   over set members
                       + Σ current                   over validators no longer in the set
       clamp_loss      = Σ [ current - (active - directed - base) ]   over all pool validators
```

`clamp_loss` is the information the saturating subtractions throw away, and it is **not optional** —
omit it and the identity misses by thousands of SOL on a healthy pool. For each validator it is the
difference between the saturated `current` and the signed `active - directed - base`. It is non-zero
whenever a balance would have gone negative, which happens in two ordinary situations:

- **Near-empty validators**, where `active - directed` is below `base`. There are typically hundreds
  of these, each contributing up to `base`.
- **Directed stake recorded above active stake.** A known accounting drift — the repo has a
  `sync-directed-stake-lamports` action for it — and it can contribute thousands of SOL on its own.

Verified against mainnet at epoch 1012: left side 129,430 SOL, right side 129,366 SOL, residual
64 SOL on a 9.95M pool. Allow a tolerance of roughly `base × (validator_list_len - num_pool_validators)`
plus rounding; treat a residual above a few hundred SOL as a real failure.

This identity holds because the pool is closed: someone below target means someone else is above.
**If it does not hold, your numbers are wrong.** Three failure signatures worth recognising:

- **The difference comes out ≈ `reserve + transient + directed_total`.** You divided *gross* TVL by
  N instead of the undirected pool. Every target is overstated by `directed_total / N`, and you
  have invented a pool-wide shortfall equal to the entire directed balance. This is the single
  easiest mistake to make here, it looks exactly like a real structural finding, and it will
  survive casual sanity-checking. Subtract directed stake and recompute.
- **The difference is a few thousand SOL and you omitted `clamp_loss`.** Add the term before
  concluding anything is wrong; this is the most likely cause of a first failed check.
- **The difference is large and unrelated to any of these.** Your field offsets are wrong, or you
  are reading a stale account. Fall back to the CLI.

### Cross-validation

- If both routes are available, decode via raw RPC **and** run `view-config` / `view-state`, then
  confirm they agree on `num_delegation_validators`, `num_epochs_between_scoring`,
  `next_cycle_epoch` and `num_pool_validators`. Disagreement means the layouts have moved.
- Check the pool figures against <https://www.jito.network/stakenet/steward/>. Order-of-magnitude
  disagreement means stop.
- Sum the per-validator targets. They should total the delegatable pool, not the gross TVL.

### If a check fails

Say so plainly and stop. For example:

> I decoded the Steward state directly over RPC, but the reconciliation invariant does not hold
> (shortfall − excess came out ~380k SOL larger than reserve + transient, which is the signature of
> dividing gross TVL instead of the undirected pool). I am not going to report position figures
> from this. Install `steward-cli` from the stakenet repo and re-run, or point me at the current
> IDL so I can fix the offsets.

A refusal with the reason is a good outcome here. A confident wrong number is the failure mode this
document exists to prevent.

## 5. The calculation

Read `minimum_delegation` from `getStakeMinimumDelegation` and `stake_rent` from
`getMinimumBalanceForRentExemption(200)`. Do not hardcode either: the stake program's minimum
delegation is a cluster-level value and currently returns **1 SOL** on mainnet, so `base` is about
1.0023 SOL per validator — roughly 700 SOL across the whole pool, not the sub-SOL figure you get if
you assume the smaller SPL floor.

`sat_sub(a, b)` below means **saturating** subtraction — `max(0, a - b)`, never negative. The
program uses saturating subtraction at each of these points, so a literal signed subtraction can
produce a negative intermediate that then propagates into a nonsense target or shortfall.

```
# Pool level
directed_total   = sum of directed stake across the pool
undirected_pool  = sat_sub(stake_pool.total_lamports, directed_total)
base             = minimum_delegation + stake_rent          # per validator, ~1.0023 SOL today
delegatable      = undirected_pool - (base × validator_count)
N                = delegation denominator (equals the count of set members)
target           = delegatable / N                          # same for every member

# Per validator
current    = sat_sub(sat_sub(active_stake, directed_stake), base)     # undirected only
shortfall  = sat_sub(target, current)
rank       = position in sorted_score_indices, counting only set members (0-based)
sol_ahead  = sum of shortfall over set members with a lower rank number

# Supply
scoring_cap        = delegatable × scoring_unstake_cap_bps / 10_000
scoring_remaining  = sat_sub(scoring_cap, scoring_unstake_total)
usable_reserve     = sat_sub(reserve_lamports, stake_rent × (validator_count + 1 - processed))
```

Three of these need care beyond the saturation:

**`rank` here is not the same number `view-state` prints.** This document's rank is 0-based and
counts only delegation-set members. The CLI's `Overall Rank` is 1-based and ranks *every* pool
validator by score, so it reads a few positions higher for the same validator — at epoch 1012 a
validator at set-rank 199 showed as Overall Rank 202. Both are correct; they measure different
things. Say which one you are quoting, because an operator comparing them will otherwise assume one
is broken.

**`usable_reserve` is a conservative lower bound, not an exact figure.** The rent buffer the program
withholds is `stake_rent × (validator_count + 1 - processed)`, where `processed` is the number of
validators already rebalanced in the current epoch's pass. The buffer therefore *shrinks* as the pass
proceeds and usable reserve grows. Setting `processed = 0` — the start-of-pass value, which is what
you can observe between passes — gives the largest buffer and so the smallest usable reserve. Use
that, treat the result as a floor, and do not present it as the exact amount available.

**`delegatable` is a checked subtraction on-chain, not a saturating one.** If `base × validator_count`
exceeded the undirected pool the program would error rather than clamp. In practice it never comes
close, but if your arithmetic produces a negative here, something upstream is wrong — investigate
rather than clamping to zero.

**`scoring_remaining` can hit zero with the total slightly above the cap.** `scoring_cap` is
recomputed from the current undirected pool on every rebalance while `scoring_unstake_total`
accumulates across the cycle, so if directed stake grew mid-cycle the accumulated total can exceed
the present cap. The saturation handles it; do not report it as an error.

### Determine the binding constraint

In order — the first that matches is the answer:

| Condition | Constraint | What it means |
| :- | :- | :- |
| Not eligible (score 0) | `not_eligible` | Name the failing filter and the epoch its offending data leaves the window |
| Not in the set but eligible | `awaiting_next_scoring` | Next selection is at `next_cycle_epoch` |
| `shortfall ≤ minimum_delegation` | `at_target` | Nothing is owed |
| `transient_stake > 0` | `awaiting_cooldown` | Stake is already in flight; expect it next epoch |
| `scoring_remaining ≈ 0` and `usable_reserve ≈ 0` | `budget_exhausted` | Rotation has stopped until `next_cycle_epoch` |
| `sol_ahead > usable_reserve` | `queue_position` | Higher-ranked validators must be funded first |
| otherwise | `next_rebalance` | Supply exists and nobody is ahead |

### What you may and may not say

- **May state as fact**: eligibility and which filter fails; the epoch a failing data point expires;
  target; rank; `sol_ahead`; `scoring_remaining`; `next_cycle_epoch`; the binding constraint.
- **May state as a conditional**: "if the reserve is still at X next epoch and nobody above you
  needs it, you would receive Y" — the reserve changes continuously, so keep it conditional.
- **Must refuse**: any date, epoch count, or "you'll be at target by epoch N" that extends past
  `next_cycle_epoch`. Explain that the set is re-scored there, N changes, and every target moves
  with TVL.

## 6. What the validator actually controls

In order of leverage:

1. **Clear every eligibility filter.** Binary, and it dominates everything else. If BAM is not
   connected, that is the whole problem.
2. **Lower inflation and MEV commission.** Funding order is by score, so this moves the validator
   *up the queue*, not merely into the set — the one lever with real time value attached.
3. **Keep vote credits above the threshold every epoch.** One bad epoch stays in the window for the
   whole `epoch_credits_range`.

Pool size, deposit flow, and other operators' behaviour are **not** controllable, and are a large
part of why no date can be given.

## 7. Answering the question without running anything

Not every agent asked "when will I get stake?" has an RPC endpoint or a built CLI. If you cannot
read live data, **do not guess and do not hedge into vagueness.** Explain the shape of the answer
instead. The mechanism is deterministic; the timing is not, and saying exactly that is more useful
than an estimate.

### Canonical answer — safe to paste as-is

> There is no date, and anyone who gives you one is guessing. Here is why, and what you can know
> instead.
>
> JitoSOL delegation is decided on a fixed cycle — currently 10 epochs. At the start of a cycle every validator is
> scored, the delegation set is chosen, and each member is assigned an equal share of the pool.
> That set is then fixed until the next cycle.
>
> Whether you are in the set is **pass/fail** on a list of filters — commissions, vote credits,
> superminority, blacklist, tip-distribution authority, and running the BAM client. Fail any one and
> your score is zero, which means no stake for the whole cycle. That part is fully determined and
> you can check it yourself.
>
> When stake actually *arrives* is a queue, not a schedule. Increases are funded from the pool's
> reserve in descending score order, so your wait depends on how much is queued ahead of you and how
> much supply exists. Rotation is capped — currently at 7.5% of the pool per cycle — to limit the
> yield lost to moving stake, and when that budget is spent nothing moves until the cycle resets.
>
> What nobody can predict: the rate of deposits into and withdrawals out of JitoSOL, other
> validators' commission and performance changes, and the re-scoring that reshuffles the set every
> cycle. Those dominate the timing, which is why a projection 10 or 20 epochs out can be very
> different from what happens.
>
> So the useful questions are: *am I eligible, and if not which filter am I failing and when does it
> clear?* Both are answerable exactly, from your own ValidatorHistory account.

### Rules for any agent answering this question

Compact enough to paste into a system prompt:

```
When asked when a validator will receive JitoSOL stake:
- Never give a date, an epoch, or a countdown to reaching target. It is not derivable.
- Do say: eligibility is pass/fail and checkable; the delegation set is fixed for the cycle;
  arrival is a reserve-funded queue served in descending score order; rotation is capped per
  cycle (currently 7.5% of the pool).
- Any figure you quote from memory is a current DAO-set value, not a constant. Say so, or
  omit the number.
- Do say what is unpredictable and why it dominates: deposit/withdrawal flow, other
  validators' behaviour, and per-cycle re-scoring.
- Redirect to what is answerable exactly: which filter is failing, and the epoch the
  offending data leaves that filter's lookback window. Read the window length from the
  config; do not assume a value.
- Never state that a validator holding directed stake is underfunded based on its
  undirected balance alone.
- Never present a projection as an expectation or as a commitment from Jito.
```

## 8. Caveats to include in any answer

Reproduce the substance of these whenever reporting results:

- This is not a guarantee of stake and not a commitment from Jito.
- Firm, from on-chain state: eligibility, target, rank, SOL queued ahead, remaining budget.
- Reasonable: what would arrive next epoch given the reserve as it stands.
- Speculative: anything past the next scoring event, when the set is re-scored and N changes.
- Not modelled at all: deposit and withdrawal rate, changes in this or other validators'
  performance and commissions, emergency unstakes elsewhere.
- Every figure is a snapshot. TVL and the reserve move continuously, so amounts drift within an
  epoch even when the conclusions hold.

*Parameters are set by the Jito DAO and can change. The on-chain Steward config is always the
source of truth — read it rather than trusting any value quoted in this document.*
