---
layout: default
title: SPL Stake Pool Internals
---

# SPL Stake Pool: Overview

The SPL Stake Pool program allows users to delegate stake to multiple validators while maintaining liquidity through pool tokens.
Thorough documentation of the SPL Stake Pool program can be found [here](https://spl.solana.com/stake-pool/overview). This is meant to be a quick primer, and the Advanced Concepts section contains nuances relevant to the Steward program.

## Key Accounts

1. **Stake Pool**: Main account holding pool configuration and metadata.
2. **Validator List**: Stores information about all validators in the pool.
3. **Reserve Stake**: Holds undelegated stake for the pool.
4. **Validator Stake Accounts**: Individual stake accounts for each validator.
5. **Transient Stake Accounts**: Temporary accounts used for stake adjustments.
6. **Pool Token Mint**: Mint for the pool's liquidity tokens.

## Authorities

1. **Manager**: Controls pool configuration and fees.
2. **Staker**: Manages validator set and stake distribution.
3. **Withdraw Authority**: Program-derived authority for the pool's stake accounts.
4. **Deposit Authority** (optional): Controls stake deposits into the pool.

## Stake Flow

- **Deposit**: Users deposit SOL or existing stake accounts, receiving pool tokens in return.
- **Withdrawal**: Users burn pool tokens to withdraw SOL or activated stake accounts.

## Validator Management

- **Addition**: Staker adds new validators, creating associated stake accounts.
- **Removal**:
  1. Staker initiates removal, deactivating the validator's stake.
  2. Over subsequent epochs, deactivated stake is merged into the reserve.
  3. Validator is removed from the list once all associated stake is reclaimed.

## Epoch Update Process

1. **Update Validator List Balances**:

   - Merges transient stakes with validator stakes or reserve.
   - Updates stake balances for each validator.

2. **Update Stake Pool Balance**:

   - Calculates total pool value.
   - Applies and distributes fees.

3. **Cleanup Removed Validator Entries**:
   - Removes fully deactivated validators from the list.

## Interaction with Stake Program

The pool uses Solana's Stake program for core staking operations:

- Creating and closing stake accounts
- Delegating, activating, and deactivating stakes
- Splitting and merging stakes
- Withdrawing from stake accounts

These operations are performed via Cross-Program Invocation (CPI) calls from the Stake Pool program to the Stake program.

---

# SPL Stake Pool: Advanced Concepts

## Minimum Lamport Balances

Stake accounts in the pool must maintain minimum balances that cover rent and stake minimums:

1. **Rent-Exempt Reserve**: Every stake account must have enough lamports to be rent-exempt (2282880 lamports).
2. **Minimum Delegation**: Solana enforces a minimum delegation amount for stake accounts, which is currently 1 lamport. There is a deactivated feature that will increase this to 1 SOL (1 SOL = 10^9 lamports), but there are no current plans to activate this. spl-stake-pool's own MINIMUM_ACTIVE_STAKE constant is 1_000_000 lamports.

The pool uses the `minimum_stake_lamports` function to calculate this:

```rust
pub fn minimum_stake_lamports(meta: &Meta, stake_program_minimum_delegation: u64) -> u64 {
    meta.rent_exempt_reserve
        .saturating_add(minimum_delegation(stake_program_minimum_delegation))
}

pub fn minimum_delegation(stake_program_minimum_delegation: u64) -> u64 {
    std::cmp::max(stake_program_minimum_delegation, MINIMUM_ACTIVE_STAKE)
}
```

This ensures that stake accounts always have enough lamports to remain valid and delegated. You cannot decrease or increase a validator account's stake with fewer lamports than the minimum_delegation, because the transient stake account would not have enough lamports to remain valid.

As of August 2024, the minimum lamports for a stake account is 3_282_880 lamports.

## Stake Account Rent Funding

When keeping track of all lamports in the pool, it's important to note the rent for stake accounts in the Stake Pool comes from the reserve account.

1. **Validator Addition**:
   When adding a new validator, the rent for the validator's stake account is funded from the pool's reserve account.
   ```rust
   let required_lamports = minimum_stake_lamports(&meta, stake_minimum_delegation);
   Self::stake_split(
       stake_pool_info.key,
       reserve_stake_info.clone(),
       withdraw_authority_info.clone(),
       AUTHORITY_WITHDRAW,
       stake_pool.stake_withdraw_bump_seed,
       required_lamports,
       stake_info.clone(),
   )?;
   ```
2. **Transient Stake Accounts**:
   When creating a transient stake account (used for rebalancing), the rent is funded from the pool's reserve account.

   ```rust
   let required_lamports = stake_rent.saturating_add(lamports);
   Self::stake_split(
       stake_pool_info.key,
       reserve_stake_info.clone(),
       withdraw_authority_info.clone(),
       AUTHORITY_WITHDRAW,
       stake_pool.stake_withdraw_bump_seed,
       required_lamports,
       transient_stake_account_info.clone(),
   )?;
   ```

The Stake Pool always ensures that any stake account it creates or manages has sufficient lamports to remain rent-exempt.

## Validator Removal Process in Stake Pool

The removal of a validator from the stake pool involves several state transitions, driven by two main operations: `RemoveValidatorFromPool` and subsequent `UpdateValidatorListBalance` calls.

### State Transitions

```
Active -> DeactivatingValidator -> ReadyForRemoval
   or
Active -> DeactivatingAll -> DeactivatingTransient -> ReadyForRemoval
```

### Process Overview

1. **Removal Initiation** (`RemoveValidatorFromPool`):

   - Sets status to `DeactivatingValidator` or `DeactivatingAll` (if transient stake exists)
   - Deactivates the validator's stake account

2. **Stake Deactivation** (`UpdateValidatorListBalance`):

   - Occurs over subsequent epochs
   - Merges deactivated stakes into the reserve
   - Updates validator status based on deactivation progress

3. **Final Removal** (`CleanupRemovedValidatorEntries`):
   - Removes validators with `ReadyForRemoval` status from the list

### Key State Transitions

- `DeactivatingValidator` -> `ReadyForRemoval`:
  When validator stake is fully deactivated and merged

- `DeactivatingAll` -> `DeactivatingTransient`:
  When validator stake is deactivated, but transient stake remains

- `DeactivatingTransient` -> `ReadyForRemoval`:
  When both validator and transient stakes are deactivated and merged

### Implementation Notes

- Status updates occur in `UpdateValidatorListBalance`
- Separate handling for validator and transient stakes
- Use of `PodStakeStatus` for efficient status management

Note that this process may span multiple epochs, ensuring all stake is properly deactivated and reclaimed before final removal.

---

## Validator Removal Process

When a validator is removed from the stake pool, the process involves several steps and may take multiple epochs to complete. The exact path depends on the current state of the validator's stake accounts and when the `UpdateValidatorListBalance` instruction is called.

### Initial Removal

1. The `RemoveValidatorFromPool` instruction is called by the stake pool's staker.
2. The validator's `StakeStatus` is changed to `DeactivatingValidator`.
3. The validator's active stake account is deactivated.

### Subsequent Updates

The completion of the removal process depends on when `UpdateValidatorListBalance` is called and the state of the validator's stake accounts. This instruction can be called in the same epoch as the removal or in later epochs.

#### Scenario 1: No Transient Stake

If the validator has no transient stake when removed:

1. First `UpdateValidatorListBalance` call:

   - If the active stake is fully deactivated (cooldown complete):
     - The stake is merged into the reserve.
     - The `StakeStatus` changes to `ReadyForRemoval`.
   - If the active stake is still cooling down:
     - No change occurs.
     - The `StakeStatus` remains `DeactivatingValidator`.

2. Subsequent `UpdateValidatorListBalance` calls:

   - If the status is still `DeactivatingValidator`:
     - Check if cooldown is complete and merge into reserve if so.
   - If the status is `ReadyForRemoval`:
     - The validator entry is removed from the list.

#### Scenario 2: With Transient Stake

If the validator has transient stake when removed:

1. First `UpdateValidatorListBalance` call:

   - The `StakeStatus` changes to `DeactivatingAll`.
   - Both active and transient stakes begin deactivation (if not already deactivating).

2. Subsequent `UpdateValidatorListBalance` calls:

   - If transient stake cooldown completes first:
     - Transient stake is merged into reserve.
     - `StakeStatus` changes to `DeactivatingValidator`.
   - If active stake cooldown completes first:
     - Active stake is merged into reserve.
     - `StakeStatus` changes to `DeactivatingTransient`.

3. Final `UpdateValidatorListBalance` call:
   - When both active and transient stakes are fully deactivated and merged:
     - `StakeStatus` changes to `ReadyForRemoval`.
   - On the next call after `ReadyForRemoval`:
     - The validator entry is removed from the list.

### Important Notes

- The entire process can take multiple epochs due to Solana's stake cooldown period.
- `UpdateValidatorListBalance` must be called regularly to progress the removal process.
- Validators in any deactivating state (`DeactivatingValidator`, `DeactivatingAll`, `DeactivatingTransient`) cannot receive new stake.
- Withdrawals can still occur from deactivating validator stake accounts, potentially accelerating the removal process.
- If a validator is removed and re-added before the removal process completes, the process resets and the validator becomes active again.
