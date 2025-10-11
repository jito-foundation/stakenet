# Steward Program

The Steward Program is an Anchor program designed to manage the staking authority for a SPL Stake Pool. Using on-chain [validator history](https://github.com/jito-foundation/stakenet) the steward selects a set of high-performing validators to delegate to, maintains the desired level of stake on those validators over time, and continuously monitors and re-evaluates the validator set at a set cadence. Initially, the validator selection is customized for the JitoSOL stake pool criteria and will be deployed to manage that stake pool.

The core operations of the Steward Program are permissionless such that any cranker can operate the system. However there are some [admin/management functions](#admin-abilities) that allow for tweaking parameters and system maintenance.

**The Purpose**

The Steward Program was created to automatically manage the Jito Stake Pool. Using on-chain [validator history](https://github.com/jito-foundation/stakenet) data, the steward chooses who to stake to and how much by way of it's staking algorithm. Additionally, the steward surfaces this staking algorithm through variable parameters to be decided by [Jito DAO](https://gov.jito.network/dao/Jito). In turn, this greatly decentralizes the stake pool operations.

## Table of Contents

- [Steward Program](#steward-program)
  - [Table of Contents](#table-of-contents)
  - [Program Overview](#program-overview)
  - [State Machine](#state-machine)
    - [Compute Scores](#compute-scores)
    - [Compute Delegations](#compute-delegations)
    - [Idle](#idle)
    - [Compute Instant Unstake](#compute-instant-unstake)
    - [Rebalance](#rebalance)
    - [Rest of the cycle](#rest-of-the-cycle)
    - [Diagram](#diagram)
  - [Validator Management](#validator-management)
    - [Adding Validators](#adding-validators)
    - [Removing Validators](#removing-validators)
  - [Admin Abilities](#admin-abilities)
  - [Parameters](#parameters)
  - [Code and Tests](#code-and-tests)

## Program Overview

The Steward Program's main functions include:

1. **Validator Selection and Management**: The program selects validators, monitors their performance, and adjusts stake allocations to maintain optimal performance.
2. **[State Machine](#state-machine)**: A state machine representing the progress throughout a cycle (10-epoch period) for scoring and delegations.
3. **[Adding and Removing Validators](#validator-management)**: Automatically handles the addition and removal of validators based on predefined criteria.
4. **[Admin Abilities](#admin-abilities)**: Allows administrators to update parameters, manage a blacklist, pause/unpause the state machine, and execute passthrough instructions for SPL Stake Pool.

## State Machine

The state machine represents the progress throughout a cycle (10-epoch period for scoring and delegations).

### Compute Scores

At the start of a 10 epoch cycle ("the cycle"), all validators are scored. We save both the overall `score` (which includes binary eligibility filters) and the `raw_score` (the 4-tier hierarchical score before filters).

The `score` determines eligibility to be staked in the pool (must be non-zero). The `raw_score` determines the unstaking order (lower raw_score validators are unstaked first).

The scoring system uses a **4-tier hierarchical encoding** combined with **binary eligibility filters**.

### 4-Tier Score Components

The `raw_score` is a u64 value encoding four tiers in descending order of importance:

1. **Tier 1 (bits 56-63)**: Inflation commission (inverted, 0-100%) - Lower commission = higher score
2. **Tier 2 (bits 42-55)**: MEV commission average (inverted, 0-10000 bps) - Lower commission = higher score
3. **Tier 3 (bits 25-41)**: Validator age (direct, epochs with non-zero vote credits) - Older = higher score
4. **Tier 4 (bits 0-24)**: Vote credits ratio (normalized, scaled) - Higher performance = higher score

This hierarchical structure ensures that differences in higher-order tiers (e.g., inflation commission) dominate lower-order tiers when comparing validators.

### Binary Eligibility Filters

The following binary filters (0 or 1) are applied to the `raw_score`:

- `mev_commission_score`: If max MEV commission in `mev_commission_range` epochs is ≤ threshold, score is 1, else 0
- `commission_score`: If max commission in `commission_range` epochs is ≤ threshold, score is 1, else 0
- `historical_commission_score`: If max commission in all history is ≤ threshold, score is 1, else 0
- `blacklisted_score`: If validator is NOT blacklisted, score is 1, else 0
- `superminority_score`: If validator is NOT in the superminority, score is 1, else 0
- `delinquency_score`: If delinquency ratio is acceptable in all epochs, score is 1, else 0
- `running_jito_score`: If validator has any MEV commission in the last `mev_commission_range` epochs, score is 1, else 0
- `merkle_root_upload_authority_score`: If validator is using TipRouter or OldJito Tip Distribution merkle root upload authority, score is 1, else 0
- `priority_fee_merkle_root_upload_authority_score`: If validator is using acceptable priority fee merkle root upload authority, score is 1, else 0
- `priority_fee_commission_score`: If validator's realized priority fee commission is ≤ configured threshold over configured epoch range, score is 1, else 0

> Note: All data comes from the `ValidatorHistory` account for each validator.

### Score Formula

```rust
// Calculate 4-tier raw score
let raw_score = encode_validator_score(
    max_commission,           // Tier 1: 0-100 (inverted)
    mev_commission_avg,       // Tier 2: 0-10000 bps (inverted)
    validator_age,            // Tier 3: epochs with non-zero vote credits
    vote_credits_avg          // Tier 4: scaled normalized vote credits ratio
);

// Apply binary filters
let score = raw_score
    * mev_commission_score
    * commission_score
    * historical_commission_score
    * blacklisted_score
    * superminority_score
    * delinquency_score
    * running_jito_score
    * merkle_root_upload_authority_score
    * priority_fee_commission_score
    * priority_fee_merkle_root_upload_authority_score;
```

As a validator, to receive a high score for JitoSOL, you must meet all binary eligibility criteria (binary filters) AND optimize the 4-tier score components. The eligibility criteria ensure delegation to validators meeting important properties for decentralization, Solana network health, operator quality, and MEV sharing.

The 4-tier hierarchical system creates a clear priority order:
1. **Inflation commission** (most important) - Validators with lower commissions are always preferred
2. **MEV commission** - Among validators with equal inflation commission, lower MEV commission is preferred
3. **Validator age** - Among validators equal on commissions, older validators are preferred
4. **Vote credits** - Among validators equal on all above, higher performance is preferred

If any binary filter fails (equals 0), the final score becomes 0 regardless of the raw_score. The `raw_score` is also used for ranking validators during unstaking operations (lower raw_score validators are unstaked first).

For a breakdown of the formulas used for each score, see the Appendix.

Take a look at the implementation in [score.rs](./src/score.rs#L180)

### Compute Delegations

Once all the validators are scored, we need to calculate the stake distribution we will be aiming for during this cycle.

The top N validators by overall score will become our validator set (where N = `num_delegation_validators`, currently 400), with each receiving 1/N of the pool's stake. If there are fewer than N validators eligible (having a non-zero score), the "ideal" validators are all of the eligible validators.

At the end of this step, we have a list of target delegations, representing proportions of the share of the pool, not fixed lamport amounts.

### Idle

Once the delegation amounts are set, the Steward waits until we’ve reached the 95% point of the epoch to run the next step.

### Compute Instant Unstake

All validators are checked for a set of Instant Unstaking criteria, like commission rugs, delinquency, etc. If they hit the criteria, they are marked for the rest of the cycle.

The following criteria are used to determine if a validator should be instantly unstaked:

- `delinquency_check`: Checks if validator has missed > `instant_unstake_delinquency_threshold_ratio` of votes this epoch
- `commission_check`: Checks if validator has increased commission > `commission_threshold`
- `mev_commission_check`: Checks if validator has increased MEV commission > `mev_commission_bps_threshold`
- `is_blacklisted`: Checks if validator was added to blacklist
- `is_bad_merkle_root_upload_authority`: Checks if validator has an unacceptable Tip Distribution merkle root upload authority
- `is_bad_priority_fee_merkle_root_upload_authority`: Checks if validator has an unacceptable Priority Fee merkle root upload authority

If any of these criteria are true, we mark the validator for instant unstaking:

```rust
let instant_unstake =
    delinquency_check || commission_check || mev_commission_check || is_blacklisted
    || is_bad_merkle_root_upload_authority || is_bad_priority_fee_merkle_root_upload_authority;
```

Take a look at the implementation in [score.rs](./src/score.rs#L554)

### Rebalance

One instruction is called for each validator, to increase or decrease stake if a validator is not at the target delegation.

For each validator, we first check if the target balance is greater or less than the current balance.

If the target is less, we attempt to undelegate stake:

When undelegating stake, we want to protect against massive unstaking events due to bugs or network anomalies, to preserve yield. There are two considerations with this:
There are 3 main reasons we may want to unstake. We want to identify when each of these cases happen, and let some amount of unstaking happen for each case throughout a cycle.

- the pool is out of line with the ideal top 200 validators and should be rebalanced,
- a validator is marked for instant unstake, or
- a validator gets a stake deposit putting it far above the target delegation.

We want to run the rebalance step in parallel across all validators, meaning these instructions can be run in any order, but we need to have some notion of the unstaking priority/ordering so the worst validators can be unstaked before the cap is hit. Pre-calculating the balance changes is difficult since balances can change at any time due to user stake deposits and withdrawals, and the total lamports of the pool can change at any time.

To address 1: we set a cap for each unstake condition, and track the amount unstaked for that condition per cycle. (scoring_unstake_cap, instant_unstake_cap, stake_deposit_unstake_cap)

To address 2: in each instruction, we calculate how much this validator is able to be unstaked in real-time, based on the current balances of all validators, unstaking caps, and which validators would be “ahead” in priority for unstaking before caps are hit. (Lower yield_score = higher priority). If all the “worse” validators will be unstaked and hit the caps before this one can, no unstaking is done on this validator.

For each validator, its active stake balance in lamports is then saved. In the next epoch, any lamports above this can be assumed to be a stake deposit, and can be unstaked.

If the target is greater, we attempt to delegate stake:

In a similar vein to unstaking, we want to be able to customize the priority of staking so that instructions can be run in any order, but stake is going to better validators given a limited pool reserve.

We calculate how much this validator is able to be staked in real time, given the number of validators “ahead” in priority (based on overall score). If all validators who need stake are able to be filled and there is still stake left over in the reserve, this validator gets the stake it needs, either up to the target or until the reserve is empty, whichever is first.

If stake was delegated, the balance is updated.

Note that because of the 1-epoch delay in cooling down stake that’s unstaked from validators, there will be many instances where the reserve won’t have enough balance to match the stake needs for everyone, but the following epoch, it will (assuming no withdrawals).

If we are already at the target, nothing happens.

Progress is marked so this validator won’t be adjusted again this epoch. After all validators’ progress is marked “true”, we transition to idle.

### Rest of the cycle

After unstaking is done, the state machine moves back into Idle. In next epoch and in the rest of the epochs for the cycle, it repeats these steps:

- Compute Instant Unstake
- Rebalance
- Idle

At the start of the next cycle, we move back to Compute Scores, and all those pieces of state are reset to 0.

### Diagram

[View in Figma](https://www.figma.com/board/zBcpTOu1zqKIoEnd54sWqH/Steward-Program?node-id=0-1&t=1FK9C2TtNvFEfOU6-1)

![State Machine Diagram](./state-machine-diagram.png)

## Validator Management

### Adding Validators

The JitoSOL pool aims to have as many active validators as possible. Validators are added permissionlessly if they meet the following criteria:

- At least 5 epochs of voting.
- Minimum SOL stake of 5000.

There are approximately 1300 validators that meet these criteria today, with a capacity for 5000 validators.

### Removing Validators

Validators are removed if:

- The validator’s vote account closes.
- The validator stops voting for 5 epochs, leading to the deactivation of the stake account in the stake pool.

## Admin Abilities

Administrators can:

- Update parameters.
- Add/remove validators to/from the blacklist to prevent delegation or scoring.
- Pause/unpause the state machine, preventing progress when `config.paused` is true.
- Execute passthrough instructions for SPL Stake Pool requiring the staker as a signer.

## Parameters

| Parameter                                     | Value                        | Description                                                                                                                                                                                             |
| --------------------------------------------- | ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Scoring Parameters**                        |                              |                                                                                                                                                                                                         |
| `mev_commission_range`                        | 30                           | Number of recent epochs used to evaluate MEV commissions and running Jito for scoring                                                                                                                   |
| `epoch_credits_range`                         | 30                           | Number of recent epochs used to evaluate yield                                                                                                                                                          |
| `commission_range`                            | 30                           | Number of recent epochs used to evaluate commissions for scoring                                                                                                                                        |
| `mev_commission_bps_threshold`                | 1000                         | Maximum allowable MEV commission in mev_commission_range (stored in basis points)                                                                                                                       |
| `commission_threshold`                        | 5                            | Maximum allowable validator commission in commission_range (stored in percent)                                                                                                                          |
| `historical_commission_threshold`             | 50                           | Maximum allowable validator commission in all history (stored in percent)                                                                                                                               |
| `scoring_delinquency_threshold_ratio`         | 0.97                         | Minimum ratio of slots voted on for each epoch for a validator to be eligible for stake. Used as proxy for validator reliability/restart timeliness. Ratio is number of epoch_credits / blocks_produced |
|                                               |                              |                                                                                                                                                                                                         |
| **Priority Fee Scoring Parameters**           |                              |                                                                                                                                                                                                         |
| `priority_fee_lookback_epochs`                | 10                           | Number of epochs to look back for priority fee commission evaluation                                                                                                                                    |
| `priority_fee_lookback_offset`                | 2                            | Epoch offset for priority fee evaluation (look at epochs from current_epoch - offset - lookback to current_epoch - offset)                                                                             |
| `priority_fee_max_commission_bps`             | 10000                        | Maximum allowable average realized priority fee commission (in basis points). Validators exceeding this fail priority_fee_commission_score                                                              |
| `priority_fee_error_margin_bps`               | 500                          | Error margin for priority fee commission calculations (in basis points)                                                                                                                                 |
| `priority_fee_scoring_start_epoch`            | 65535                        | Epoch when priority fee scoring begins (scores default to 1 for all prior epochs)                                                                                                                       |
|                                               |                              |                                                                                                                                                                                                         |
| **Delegation Parameters**                     |                              |                                                                                                                                                                                                         |
| `instant_unstake_delinquency_threshold_ratio` | 0.70                         | Same as scoring_delinquency_threshold_ratio but evaluated every epoch                                                                                                                                   |
| `num_delegation_validators`                   | 400                          | Number of validators who are eligible for stake (validator set size)                                                                                                                                    |
| `scoring_unstake_cap_bps`                     | 750                          | Percent of total pool lamports that can be unstaked due to new delegation set (in basis points)                                                                                                         |
| `instant_unstake_cap_bps`                     | 1000                         | Percent of total pool lamports that can be unstaked due to instant unstaking (in basis points)                                                                                                          |
| `stake_deposit_unstake_cap_bps`               | 1000                         | Percent of total pool lamports that can be unstaked due to stake deposits above target lamports (in basis points)                                                                                       |
|                                               |                              |                                                                                                                                                                                                         |
| **State Machine Operation Parameters**        |                              |                                                                                                                                                                                                         |
| `compute_score_slot_range`                    | 10000                        | Scoring window such that the validators are all scored within a similar timeframe (in slots)                                                                                                            |
| `instant_unstake_epoch_progress`              | 0.90                         | Point in epoch progress before instant unstake can be computed                                                                                                                                          |
| `instant_unstake_inputs_epoch_progress`       | 0.50                         | Inputs to "Compute Instant Unstake" need to be updated past this point in epoch progress                                                                                                                |
| `num_epochs_between_scoring`                  | 10                           | Cycle length - Number of epochs to run the Monitor->Rebalance loop                                                                                                                                      |
| `minimum_stake_lamports`                      | 5,000,000,000,000 (5000 SOL) | Minimum number of stake lamports for a validator to be considered for the pool                                                                                                                          |
| `minimum_voting_epochs`                       | 5                            | Minimum number of consecutive epochs a validator has to vote before it can be considered for the pool                                                                                                   |

## Code and Tests

- **Code Repository**: [Steward Program Code](https://github.com/jito-foundation/stakenet/tree/steward)
- **Program**: [Steward Program](https://github.com/jito-foundation/stakenet/tree/steward/programs/steward)
- **Tests**: [Steward Program Tests](https://github.com/jito-foundation/stakenet/tree/steward/tests/tests/steward)

## Appendix

### Score Formulas

$`
\displaylines{
\text{mev\_commission\_score} = 
\begin{cases} 
1.0 & \text{if } \max(\text{mev\_commission}_{t_1, t_2}) \leq \text{mev\_commission\_bps\_threshold} \\
0.0 & \text{otherwise}
\end{cases} \\
\text{where } t_1 = \text{current\_epoch} - \text{mev\_commission\_range} \\
\text{and } t_2 = \text{current\_epoch}
}
`$

---

$`
\displaylines{
\text{running\_jito\_score} = 
\begin{cases} 
1.0 & \text{if any MEV commission exists in } t_1 \text{ to } t_2 \\
0.0 & \text{otherwise}
\end{cases} \\
\text{where } t_1 = \text{current\_epoch} - \text{mev\_commission\_range} \\
\text{and } t_2 = \text{current\_epoch}
}
`$

---

$`
\displaylines{
\text{delinquency\_score} = 
\begin{cases} 
1.0 & \text{if } \left( \frac{\text{vote\_credits}_t}{\text{total\_blocks}_t} \right) > \text{scoring\_delinquency\_threshold\_ratio} \text{ for all } t_1 \leq t \leq t_2 \\
0.0 & \text{otherwise}
\end{cases} \\
\text{where } t_1 = \text{current\_epoch} - \text{epoch\_credits\_range} \\
\text{and } t_2 = \text{current\_epoch} - 1
}
`$

---

$`
\displaylines{
\text{commission\_score} = 
\begin{cases} 
1.0 & \text{if } \max(\text{commission}_{t_1, t_2}) \leq \text{commission\_threshold} \\
0.0 & \text{otherwise}
\end{cases} \\
\text{where } t_1 = \text{current\_epoch} - \text{commission\_range} \\
\text{and } t_2 = \text{current\_epoch}
}
`$

---

$`
\displaylines{
\text{historical\_commission\_score} = 
\begin{cases} 
1.0 & \text{if } \max(\text{historical\_commission}_{t_1, t_2}) \leq \text{historical\_commission\_threshold} \\
0.0 & \text{otherwise}
\end{cases} \\
\text{where } t_1 = \text{first\_reliable\_epoch} = 520 \\
\text{and } t_2 = \text{current\_epoch}
}
`$

---

$`
\displaylines{
\text{blacklisted\_score} = 
\begin{cases} 
0.0 & \text{if blacklisted in current epoch} \\
1.0 & \text{otherwise}
\end{cases}
}
`$

---

$`
\displaylines{
\text{merkle\_root\_upload\_authority\_score} =
\begin{cases}
1.0 & \text{if Tip Distribution merkle root upload authority is acceptable in current epoch} \\
0.0 & \text{otherwise}
\end{cases}
}
`$

---

$`
\displaylines{
\text{priority\_fee\_merkle\_root\_upload\_authority\_score} =
\begin{cases}
1.0 & \text{if Priority Fee merkle root upload authority is acceptable in current epoch} \\
0.0 & \text{otherwise}
\end{cases}
}
`$

---

$`
\displaylines{
\text{superminority\_score} = 
\begin{cases} 
0.0 & \text{if in superminority in current epoch} \\
1.0 & \text{otherwise}
\end{cases} \\
}
`$

---

$`
\displaylines{
\text{vote\_credits\_ratio} = \frac{\sum_{t=t_1}^{t_2} \text{vote\_credits}_t}{\sum_{t=t_1}^{t_2} \text{total\_blocks}_t} \\
\text{where } t_1 = \text{current\_epoch} - \text{epoch\_credits\_range} \\
\text{and } t_2 = \text{current\_epoch} - 1
}
`$

Note: total_blocks is the field in ClusterHistory that tracks how many blocks were created by the cluster in a given epoch. This represents the maximum number of vote credits that a validator can earn. Vote credits are synonymous with epoch credits.

---

$`
\displaylines{
\text{avg\_mev\_commission} = \lceil \frac{\sum_{t=t_1}^{t_2} \text{mev\_commission}_t}{\text{count}(\text{mev\_commission}_t)} \rceil \\
\text{where } t_1 = \text{current\_epoch} - \text{mev\_commission\_range} \\
\text{and } t_2 = \text{current\_epoch} \\
\text{(ceiling division to be more strict)}
}
`$

Note: If no MEV commission data exists, defaults to BASIS_POINTS_MAX (10000).

---

### 4-Tier Encoding Formula

The raw score is encoded as a u64 with the following bit layout:

$`
\displaylines{
\text{raw\_score} = (\text{inflation\_tier} \ll 56) \mid (\text{mev\_tier} \ll 42) \mid (\text{age\_tier} \ll 25) \mid \text{credits\_tier} \\\\
\text{where:} \\
\text{inflation\_tier} = 100 - \min(\text{max\_commission}, 100) \text{ (8 bits)} \\
\text{mev\_tier} = 10000 - \min(\text{avg\_mev\_commission}, 10000) \text{ (14 bits)} \\
\text{age\_tier} = \min(\text{validator\_age}, 2^{17}-1) \text{ (17 bits)} \\
\text{credits\_tier} = \min(\text{vote\_credits\_ratio} \times 10^7, 2^{25}-1) \text{ (25 bits)}
}
`$

Note: Higher raw_score values indicate better validators due to inversion of commission values and direct encoding of age and credits.

---

$`
\displaylines{
\text{final\_score} = \text{raw\_score} \times \text{mev\_commission\_score} \times \text{commission\_score} \times \text{historical\_commission\_score} \\
\times \text{blacklisted\_score} \times \text{merkle\_root\_upload\_authority\_score} \times \text{superminority\_score} \\
\times \text{delinquency\_score} \times \text{running\_jito\_score} \times \text{priority\_fee\_commission\_score} \\
\times \text{priority\_fee\_merkle\_root\_upload\_authority\_score}
}
`$
