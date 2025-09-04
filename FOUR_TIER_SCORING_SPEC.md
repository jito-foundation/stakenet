# Four-Tier Validator Scoring System Specification

## Overview
Implementation of a new 4-tier tiebreaker scoring system for validator ranking.
The new system requires migrating from u32 to u64 score storage to accommodate the necessary bit precision for all four ranking tiers.

## The 4-Tier Ranking System

The system ranks validators by four metrics in priority order:
1. **Inflation Commission** (lower is better) - Existing metric
2. **MEV Commission** (lower is better) - Existing metric  
3. **Validator Age** (higher is better) - **NEW metric**
4. **Epoch Credits** (higher is better) - Existing metric (modified from ratio to direct value)

## Why u64 is Required

### Initial u32 Attempt (Insufficient)
We initially attempted to encode all four metrics into a u32 value using aggressive bit packing and bucketing strategies:

```
Initial 32-bit layout (NOT ENOUGH PRECISION):
┌────────────┬──────────────┬─────────────────┬──────────────────────┐
│ Bits 28-31 │ Bits 18-27   │ Bits 10-17      │ Bits 0-9             │
│ (4 bits)   │ (10 bits)    │ (8 bits)        │ (10 bits)            │
├────────────┼──────────────┼─────────────────┼──────────────────────┤
│ Inflation  │ MEV          │ Validator Age   │ Vote Credits         │
│ Commission │ Commission   │ (÷16 bucketed)  │ Ratio (0.90-1.00)    │
└────────────┴──────────────┴─────────────────┴──────────────────────┘
```

**Problems with u32:**
- Inflation commission: 4 bits sufficient for 0-10% range (values 0-10)
- MEV commission: 10 bits adequate for 0-10% range (0-1000 basis points)
- Validator age: Forced to bucket by 16 epochs - loses granularity
- **Epoch credits: 10 bits insufficient for meaningful discrimination**
  - Max epoch credits: 6,912,000 (requires 23 bits minimum)
  - With only 10 bits (1024 values), would need to bucket by ~6,750 credits
  - Loses ability to distinguish between validators with similar performance
  - Critical for fair stake distribution among high-performing validators

The critical issue is **epoch credits** requiring at least 23 bits to represent the full range (0-6,912,000). With only 10 bits available in u32, the bucketing would be too coarse to fairly rank validators, leaving insufficient bits for the other three metrics.

### u64 Bit Encoding Strategy

With u64, we can properly encode all four metrics with full precision for commissions and appropriate precision for age and vote credits:

```
64-bit layout:
┌──────────────┬──────────────┬──────────────────┬────────────────────┐
│ Bits 56-63   │ Bits 42-55   │ Bits 25-41       │ Bits 0-24          │
│ (8 bits)     │ (14 bits)    │ (17 bits)        │ (25 bits)          │
├──────────────┼──────────────┼──────────────────┼────────────────────┤
│ Inflation    │ MEV          │ Validator Age    │ Epoch Credits      │
│ Commission   │ Commission   │ (epochs)         │ (direct value)     │
└──────────────┴──────────────┴──────────────────┴────────────────────┘
```

### u64 Field Specifications

1. **Inflation Commission (8 bits)**
   - Range: 0-255 (full u8 range, covers 0-100% with full precision)
   - Inverted: `100 - min(commission, 100)` (lower commission = higher value)
   - Full precision matching the underlying u8 type

2. **MEV Commission (14 bits)**  
   - Range: 0-16,383 (covers full 0-10,000 basis points range)
   - Inverted: `10000 - min(mev_commission_bps, 10000)` (lower commission = higher value)
   - Full precision for all possible basis point values

3. **Validator Age (17 bits)**
   - Range: 0-131,071 epochs directly (no bucketing needed!)
   - Direct encoding (older = higher value)
   - Covers ~716 years at 2 days/epoch (far exceeds 100-year requirement)
   - NEW METRIC: Counts epochs with non-zero vote credits
   - No bucketing required - exact epoch count preserved

4. **Epoch Credits (25 bits)**
   - Range: 0-33,554,431 (covers full epoch credits range 0-6,912,000)
   - Direct encoding of epoch credits value (max: 16 × 432,000 = 6,912,000)
   - No ratio calculation needed - simpler implementation
   - Perfect precision: every single credit difference is preserved
   - Future proof: 4.85x headroom for any potential increases

## Migration Strategy: Memory Reuse with State Preservation

The migration leverages a perfect alignment in the existing memory layout that requires **NO account reallocation**.

### Memory Layout Discovery

**Current layout:**
- `.scores`: [u32; 5,000] = 20,000 bytes
- `.yield_scores`: [u32; 5,000] = 20,000 bytes
- `_padding0`: [u8; 40,002] = 40,002 bytes (MAX_VALIDATORS * 8 + 2)

**Reinterpreted layout:**
- Old `.scores` + `.yield_scores` space → new `.scores: [u64; 5,000]` = 40,000 bytes
- Old `_padding0` space → new `.yield_scores: [u64; 5,000]` = 40,000 bytes
- Remaining padding: 2 bytes

This perfect alignment means we can migrate without changing the account size!

### Struct Versions

```rust
// Version 1 (Current)
pub struct StewardStateV1 {
    pub state_tag: StewardStateEnum,
    pub validator_lamport_balances: [u64; MAX_VALIDATORS],
    pub scores: [u32; MAX_VALIDATORS],
    pub sorted_score_indices: [u16; MAX_VALIDATORS],
    pub yield_scores: [u32; MAX_VALIDATORS],
    pub sorted_yield_score_indices: [u16; MAX_VALIDATORS],
    // ... other fields ...
    pub _padding0: [u8; 40002],
}

// Version 2 (After Migration)
pub struct StewardStateV2 {
    pub state_tag: StewardStateEnum,
    pub validator_lamport_balances: [u64; MAX_VALIDATORS],
    pub scores: [u64; MAX_VALIDATORS],  // Reuses memory of old scores + yield_scores
    pub sorted_score_indices: [u16; MAX_VALIDATORS],
    pub yield_scores: [u64; MAX_VALIDATORS],  // Uses former padding space
    pub sorted_yield_score_indices: [u16; MAX_VALIDATORS],
    // ... other fields unchanged ...
    pub _padding0: [u8; 2],  // Reduced to 2 bytes
}
```

### Migration Instruction Logic

The migration preserves operational continuity while upgrading the scoring system:

```rust
fn migrate_steward_state_v1_to_v2(ctx: Context<MigrateStewardState>) -> Result<()> {
    // Read account data as V1
    let v1_state = StewardStateV1::try_from_slice(&ctx.accounts.steward_state.data.borrow())?;
    
    // Create V2 with preserved operational state
    let v2_state = StewardStateV2 {
        // Preserve state machine position - no disruption
        state_tag: v1_state.state_tag,
        
        // Preserve validator tracking
        validator_lamport_balances: v1_state.validator_lamport_balances,
        
        // Convert scores (preserves relative ordering)
        scores: v1_state.scores.map(|s| s as u64),
        sorted_score_indices: v1_state.sorted_score_indices,
        
        // Convert yield scores
        yield_scores: v1_state.yield_scores.map(|s| s as u64),
        sorted_yield_score_indices: v1_state.sorted_yield_score_indices,
        
        // Preserve all operational state
        delegations: v1_state.delegations,
        instant_unstake: v1_state.instant_unstake,
        progress: v1_state.progress,
        validators_for_immediate_removal: v1_state.validators_for_immediate_removal,
        validators_to_remove: v1_state.validators_to_remove,
        
        // Preserve cycle metadata
        start_computing_scores_slot: v1_state.start_computing_scores_slot,
        current_epoch: v1_state.current_epoch,
        next_cycle_epoch: v1_state.next_cycle_epoch,
        num_pool_validators: v1_state.num_pool_validators,
        scoring_unstake_total: v1_state.scoring_unstake_total,
        instant_unstake_total: v1_state.instant_unstake_total,
        stake_deposit_unstake_total: v1_state.stake_deposit_unstake_total,
        status_flags: v1_state.status_flags,
        validators_added: v1_state.validators_added,
        
        _padding0: [0u8; 2],
    };
    
    // Write back as V2 (same size, no realloc needed!)
    v2_state.serialize(&mut &mut ctx.accounts.steward_state.data.borrow_mut()[..])?;
    
    Ok(())
}
```

### Key Benefits

1. **No Account Reallocation**: Reuses exact same memory footprint (184KB)
2. **No Service Disruption**: State machine continues from current position
3. **Preserved Stake Allocations**: Current delegations remain valid
4. **Maintained Validator State**: Removal marks and balances preserved
5. **Smooth Transition**: Next scoring cycle naturally uses new u64 encoding
6. **No Rebalancing Storm**: Avoids mass unstaking attempts

### Post-Migration Behavior

**Immediately after migration:**
- State machine continues in current state
- Existing u32 scores converted to u64 (simple zero-extension)
- Relative validator ordering preserved
- Current stake delegations unchanged

**At next scoring cycle (within 2-3 days):**
- New 4-tier bit encoding applied
- Validator age metric incorporated
- Full precision epoch credits
- Deterministic tiebreaking enabled

The migration provides seamless continuity with zero downtime or stake disruption

## Validator History Account Extension

### New Persistent Fields for Validator Age Tracking

To track validator age beyond the 512-epoch circular buffer limit, we add two fields to the ValidatorHistory account:

```rust
pub struct ValidatorHistory {
    // ... existing fields ...
    
    // Persistent validator age tracking
    pub validator_age: u32,                      // 4 bytes - Accumulator: total epochs with non-zero vote credits
    pub validator_age_last_updated_epoch: u16,   // 2 bytes - Last epoch when accumulator was updated
    
    pub _padding1: [u8; 226],  // Reduced from 232 bytes (was 232, now 226)
}
```

### How It Works

1. **Initial Observation (when `validator_age_last_updated_epoch == 0`):**
   - Scan the 512-epoch circular buffer
   - Count all epochs with non-zero vote credits
   - Set `validator_age` to this count
   - Set `validator_age_last_updated_epoch` to current epoch

2. **Subsequent Updates:**
   - Check if current epoch has non-zero vote credits
   - If yes and epoch > `validator_age_last_updated_epoch`:
     - Increment `validator_age`
     - Update `validator_age_last_updated_epoch`

3. **Idempotent Design:**
   - Safe to call multiple times in same epoch
   - Automatically initializes on first use
   - No migration needed - uses existing padding

### Key Benefits

- **No Migration Required**: Uses 6 bytes from existing padding
- **No Account Reallocation**: Account size remains exactly 65,848 bytes
- **Backward Compatible**: Old clients see new fields as padding
- **Idempotent Initialization**: Automatically backfills historical data on first observation
- **Unbounded Tracking**: Tracks validator age beyond 512-epoch buffer limit

## Summary

This specification outlines a 4-tier validator ranking system that requires migrating from u32 to u64 score storage:

### Key Findings
- **u32 Insufficient**: Even with aggressive bucketing, 32 bits cannot provide the required 6 decimals of precision for vote credits ratio while encoding three other metrics
- **u64 Required**: Allocates 8 bits for inflation commission (full u8 precision), 14 bits for MEV commission (full basis points range), 17 bits for validator age (716+ years), and 25 bits for epoch credits (full 0-6,912,000 range)

### Migration Strategy
- **Memory Reuse**: Leverages perfect alignment in existing layout - NO reallocation needed
- **State Preservation**: Maintains operational continuity by preserving validator state, delegations, and state machine position
- **Zero Downtime**: Seamless transition with scores converted from u32 to u64, preserving relative ordering
- **Account Size Unchanged**: Remains at ~184KB by reinterpreting existing memory

### Validator Age Tracking
- Adds two fields to ValidatorHistory using existing padding (no reallocation needed)
- Idempotent initialization automatically backfills historical data
- Enables unbounded age tracking beyond the 512-epoch circular buffer limit

The design provides deterministic sorting through single u64 comparison while ensuring a smooth migration with no service disruption or stake rebalancing storms.
