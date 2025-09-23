# Bug Report: SlotHistoryOutOfDate Error During Stake Account Updates

## Issue Summary
The keeper is encountering error 3007 (`SlotHistoryOutOfDate`) when attempting to update stake accounts through the validator history program. This prevents proper stake account tracking and updates.

## Error Details
```
[2025-09-20T18:45:30Z INFO  stakenet_keeper] Updating stake accounts...
[2025-09-20T18:45:30Z ERROR stakenet_sdk::utils::transactions] Could not simulate instruction: Error { request: None, kind: TransactionError(InstructionError(2, Custom(3007))) }
```

## Error Code
- **Error Code**: 3007
- **Error Name**: `SlotHistoryOutOfDate`
- **Error Message**: "Slot history sysvar is not containing expected slots"
- **Program**: Validator History Program
- **Location**: `programs/validator-history/src/errors.rs:29-30`

## Root Cause Analysis

### Where It Occurs
The error is thrown in `programs/validator-history/src/instructions/copy_cluster_info.rs` when the program checks if required slots are present in the slot history sysvar.

### Why It Happens
1. **RPC Node Sync Issues**: The RPC node being used may not be fully synced or may have incomplete slot history
2. **Timing Mismatch**: The program expects certain recent slots to be in the history, but they haven't been included yet
3. **Network Latency**: Delays between slot production and slot history updates on the RPC node
4. **Node Catching Up**: If the RPC node was recently restarted or is catching up, its slot history might be incomplete

## Impact
- **Severity**: MEDIUM
- **Affected Operations**: Stake account updates, validator history tracking
- **User Impact**:
  - Validator stake information may be outdated
  - Historical data collection is disrupted
  - Metrics and monitoring may show stale data

## Current Behavior
When the keeper attempts to update stake accounts:
1. It calls into the validator history program
2. The program checks the slot history sysvar for required slots
3. Expected slots are not found in the history
4. Transaction fails with error 3007
5. Stake account updates are skipped

## Potential Solutions

### Immediate Workarounds
1. **Switch RPC Endpoint**: Use a more reliable or better-synced RPC endpoint
2. **Add Retry Logic**: Implement exponential backoff retry for this specific error
3. **Delay Execution**: Wait a few slots before attempting stake account updates

### Long-term Fixes
1. **Enhanced RPC Selection**:
   ```rust
   // Add RPC health checks before operations
   async fn verify_rpc_sync_status(client: &RpcClient) -> Result<bool> {
       let slot = client.get_slot().await?;
       let slot_history = client.get_account(&slot_history::id()).await?;
       // Verify slot history contains recent slots
   }
   ```

2. **Graceful Degradation**:
   - Skip stake account updates if slot history is incomplete
   - Log warning but continue with other operations
   - Retry in next iteration

3. **Multiple RPC Fallback**:
   - Configure multiple RPC endpoints
   - Fallback to secondary RPC if primary has slot history issues

## Reproduction Steps
1. Start keeper with an RPC node that's catching up or has incomplete slot history
2. Wait for stake account update operation to trigger
3. Observe error 3007 in logs

## Detection
Monitor for:
- Error code 3007 in keeper logs
- Pattern: `TransactionError(InstructionError(2, Custom(3007)))`
- Failed "Updating stake accounts..." operations

## Related Code

### Error Definition
```rust
// programs/validator-history/src/errors.rs
#[error_code]
pub enum ValidatorHistoryError {
    // ...
    #[msg("Slot history sysvar is not containing expected slots")]
    SlotHistoryOutOfDate,  // Error 3007 (position 7 in enum)
    // ...
}
```

### Error Usage
```rust
// programs/validator-history/src/instructions/copy_cluster_info.rs
if !slot_history.contains_slot(expected_slot) {
    return Err(ValidatorHistoryError::SlotHistoryOutOfDate.into());
}
```

## Recommendations

### For Operators
1. Use enterprise-grade RPC endpoints with guaranteed uptime and sync status
2. Monitor RPC health metrics alongside keeper operations
3. Configure redundant RPC URLs in keeper configuration
4. Consider running a local RPC node for critical operations

### For Developers
1. Add RPC health checks before critical operations
2. Implement smart retry logic with exponential backoff for this error
3. Add metrics to track frequency of this error
4. Consider caching slot history locally to detect issues early

## Monitoring
Add alerts for:
- Repeated 3007 errors (more than 5 in 10 minutes)
- Stake account updates failing for more than 1 epoch
- RPC node falling behind by more than 100 slots

## Notes
- This is different from the epoch maintenance loop bug
- The error is typically transient and resolves when RPC sync improves
- May occur more frequently during network congestion or validator restarts