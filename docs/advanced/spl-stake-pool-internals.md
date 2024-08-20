---
layout: default
title: SPL Stake Pool Internals
---

# SPL Stake Pool Overview

The SPL Stake Pool program allows users to delegate stake to multiple validators while maintaining liquidity through pool tokens.

## Key Accounts

1. **Stake Pool**: Central account holding pool configuration and metadata.
2. **Validator List**: Stores information about all validators in the pool.
3. **Reserve Stake**: Holds undelegated stake for the pool.
4. **Validator Stake Accounts**: Individual stake accounts for each validator.
5. **Transient Stake Accounts**: Temporary accounts used for stake adjustments.
6. **Pool Token Mint**: Mint for the pool's liquidity tokens.

## Authorities

1. **Manager**: Controls pool configuration and fees.
2. **Staker**: Manages validator list and stake distribution.
3. **Withdraw Authority**: Program-derived authority for all pool-controlled stake accounts.
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

Stake accounts in the pool must maintain minimum balances for two reasons:

1. **Rent-Exempt Reserve**: Every stake account must have enough lamports to be rent-exempt.
2. **Minimum Delegation**: Solana enforces a minimum delegation amount for stake accounts.

The pool uses the `minimum_stake_lamports` function to calculate this:

```rust
pub fn minimum_stake_lamports(meta: &Meta, stake_program_minimum_delegation: u64) -> u64 {
    meta.rent_exempt_reserve
        .saturating_add(minimum_delegation(stake_program_minimum_delegation))
}
```

This ensures that stake accounts always have enough lamports to remain valid and delegated.

## Stake Account Rent Funding

The rent for stake accounts comes from different sources depending on the operation:

1. **Validator Addition**: Funded from the pool's reserve account.
2. **User Deposits**: For stake deposits, the rent is part of the deposited stake account. For SOL deposits, it's taken from the deposited amount.
3. **Transient Stake Accounts**: Funded from the pool's reserve when created.

Example of funding a new validator stake account:

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

## Account Seeds and Their Usage

Account seeds are crucial for deterministic derivation of various accounts in the pool:

1. **Withdraw Authority**:
   Derived using `find_withdraw_authority_program_address`.

   ```rust
   let (withdraw_authority_key, stake_withdraw_bump_seed) =
       find_withdraw_authority_program_address(program_id, stake_pool_info.key);
   ```

2. **Validator Stake Account**:
   Derived using `find_stake_program_address`.

   ```rust
   let (stake_address, _) = find_stake_program_address(
       program_id,
       vote_account_address,
       stake_pool_address,
       seed,
   );
   ```

3. **Transient Stake Account**:
   Derived using `find_transient_stake_program_address`.
   ```rust
   let (transient_stake_address, _) = find_transient_stake_program_address(
       program_id,
       vote_account_address,
       stake_pool_address,
       seed,
   );
   ```

These seeds serve several purposes:

- **Deterministic Derivation**: Allows the program to consistently locate and verify accounts.
- **Authority Delegation**: Enables the program to sign for operations on behalf of the pool.
- **Security**: Ensures that only the program can create and manage these accounts.

When performing operations like stake delegation or withdrawal, the program uses these derived addresses to authorize actions:

```rust
let authority_signature_seeds = [
    stake_pool.as_ref(),
    AUTHORITY_WITHDRAW,
    &[stake_withdraw_bump_seed],
];
let signers = &[&authority_signature_seeds[..]];

invoke_signed(&stake_instruction, &account_info, signers)?;
```

By using these seeds, the program can maintain control over all associated accounts without needing to store explicit keypairs, enhancing security and enabling atomic operations across multiple accounts.

Certainly. Here's a more concise explanation suitable for developer documentation:

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
