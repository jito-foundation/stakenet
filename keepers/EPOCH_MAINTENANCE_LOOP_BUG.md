# Critical Bug: Epoch Maintenance Infinite Loop

## Issue Summary
The testnet keeper is stuck in an infinite loop submitting epoch maintenance transactions due to a bug in the instant removal validator logic. The keeper cannot advance from epoch 785 to the current epoch 843.

## Root Cause
There's a critical bug in `_handle_instant_removal_validators` function in `keepers/stakenet-keeper/src/entries/crank_steward.rs:277`.

The bug occurs when validators have been removed from the SPL stake pool validator list but their corresponding bits remain set in the `validators_for_immediate_removal` bitmask.

## Current State (Testnet)
```
State Epoch: 785
Current Epoch: 843
Validators marked for immediate removal: 11
Validator list length: 2952
num_pool_validators: 2963
```

The 11 validators have already been removed from the validator list (2963 - 11 = 2952), but their bits remain set in the bitmask at indices 2952-2962.

## The Bug

### Location
`keepers/stakenet-keeper/src/entries/crank_steward.rs:277`

### Problematic Code
```rust
async fn _handle_instant_removal_validators(
    // ...
) -> Result<SubmitStats, JitoTransactionError> {
    // ...
    while validators_to_remove.count() != 0 {
        let mut validator_index_to_remove = None;
        // BUG: This only checks up to current validator list length
        for i in 0..all_steward_accounts.validator_list_account.validators.len() as u64 {
            if validators_to_remove.get(i as usize).map_err(|e| {
                // ...
            })? {
                validator_index_to_remove = Some(i);
                break;
            }
        }
        // ...
    }
}
```

### The Problem
1. The loop iterates from 0 to `validator_list_account.validators.len()` (2952)
2. But the bitmask contains bits set at indices 2952-2962 (for the 11 removed validators)
3. These indices are never checked, so `validator_index_to_remove` is always `None`
4. The instant removal can never complete
5. This blocks epoch maintenance from completing (epoch_maintenance.rs:95-101)

## Why It Creates an Infinite Loop

### Execution Flow
1. `crank_steward()` calls `_handle_instant_removal_validators()` (line 942)
2. Instant removal finds validators in bitmask but can't locate them (returns None)
3. Later, `_handle_epoch_maintenance()` is called (line 924)
4. Epoch maintenance checks if bitmasks are empty before advancing epoch:
   ```rust
   let okay_to_update = state_account.state.validators_to_remove.is_empty()
       && state_account.state.validators_for_immediate_removal.is_empty();

   if okay_to_update {
       state_account.state.current_epoch = clock.epoch;
   }
   ```
5. Since `validators_for_immediate_removal` is not empty, epoch never advances
6. Keeper continuously retries epoch maintenance

## The Fix

### Immediate Fix
Change line 277 in `crank_steward.rs`:
```rust
// Change from:
for i in 0..all_steward_accounts.validator_list_account.validators.len() as u64 {

// To:
for i in 0..num_validators {
```

This ensures the full range of possible validator indices is checked.

### Alternative Fix
Check up to the maximum of current validator list length and num_pool_validators:
```rust
let max_validators = std::cmp::max(
    all_steward_accounts.validator_list_account.validators.len() as u64,
    num_validators
);
for i in 0..max_validators {
```

## Impact
- **Severity**: CRITICAL
- **Affected Networks**: Testnet (confirmed), potentially Mainnet if same condition occurs
- **User Impact**: Scoring and rebalancing completely stalled since epoch 785
- **Recovery**: Requires either:
  1. Deploying the fix and restarting keeper
  2. Manual intervention to clear the stuck bitmask bits
  3. Admin instruction to force-clear `validators_for_immediate_removal`

## Symptoms in Logs
```
State Epoch: 785 | Current Epoch: 843
Validator Index to Remove: None
Submitting Epoch Maintenance
🟨 Submitted: <tx_signature>
🟩 Completed: <tx_signature>
```
This pattern repeats indefinitely every ~20 seconds.

## Prevention
1. Add bounds checking to ensure bitmask indices are always within valid ranges
2. Add detection for this condition in the keeper to alert operators
3. Consider adding a recovery mechanism for corrupted state
4. Add integration tests for validator removal edge cases

## Verification
To verify this issue exists:
```bash
cargo run --bin steward-cli -- --json-rpc-url <RPC_URL> view-state --steward-config <CONFIG_ADDRESS>
```
Look for:
- `current_epoch` significantly behind actual epoch
- Non-zero `validators_for_immediate_removal` count
- `validator_list_length` < `num_pool_validators`