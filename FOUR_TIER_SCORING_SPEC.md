# Four-Tier Validator Scoring System Specification

## Overview
Implementation of a new 4-tier tiebreaker scoring system for validator ranking.
The new system requires migrating from u32 to u64 score storage to accommodate the necessary bit precision for all four ranking tiers.

## The 4-Tier Ranking System

The system ranks validators by four metrics in priority order:
1. **Inflation Commission** (lower is better) - Existing metric
2. **MEV Commission** (lower is better) - Existing metric  
3. **Validator Age** (higher is better) - **NEW metric**
4. **Vote Credits Ratio** (higher is better) - Existing metric

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
- **Vote credits: Only 10 bits cannot achieve 6 decimals of precision** 
  - Even bucketing all values < 0.9 into a single value
  - 10 bits = 1024 values for 0.9-1.0 range
  - Only provides ~0.0001 precision (4 decimals)
  - Need 6 decimals (0.000001) to distinguish 0.999884 from 0.999883

The critical issue is **vote credits ratio** requiring 6 decimals of precision for the 0.9-1.0 range. This would need at least 20 bits (1,000,000 values), leaving insufficient bits for the other three metrics in a u32.

### u64 Bit Encoding Strategy

With u64, we can properly encode all four metrics with appropriate precision:

```
64-bit layout:
┌──────────────┬──────────────┬──────────────────┬────────────────────┐
│ Bits 60-63   │ Bits 50-59   │ Bits 30-49       │ Bits 0-29          │
│ (4 bits)     │ (10 bits)    │ (20 bits)        │ (30 bits)          │
├──────────────┼──────────────┼──────────────────┼────────────────────┤
│ Inflation    │ MEV          │ Validator Age    │ Vote Credits       │
│ Commission   │ Commission   │ (epochs)         │ Ratio              │
└──────────────┴──────────────┴──────────────────┴────────────────────┘
```

### u64 Field Specifications

1. **Inflation Commission (4 bits)**
   - Range: 0-15 (covers 0-10% range, values above 10% are filtered)
   - Inverted: `15 - min(commission, 10)` (lower commission = higher value)
   - Same as u32 version - sufficient for our needs

2. **MEV Commission (10 bits)**  
   - Range: 0-1023 (covers 0-1000 basis points = 0-10%)
   - Inverted: `1023 - min(mev_commission_bps, 1000)` (lower commission = higher value)
   - Same as u32 version - sufficient for our needs

3. **Validator Age (20 bits)**
   - Range: 0-1,048,575 epochs directly (no bucketing needed!)
   - Direct encoding (older = higher value)
   - Covers ~5,700 years at 2 days/epoch
   - NEW METRIC: Counts epochs with non-zero vote credits
   - No bucketing required - exact epoch count preserved

4. **Vote Credits Ratio (30 bits)**
   - Range: 0-1,073,741,823 levels
   - For 0.9-1.0 range: `((ratio - 0.9) * 10_000_000) as u64`
   - For < 0.9: encoded as 0
   - Precision: 7 decimals (0.0000001) - exceeds 6 decimal requirement
   - Can distinguish 0.9500000 from 0.9500001

## Migration Strategy: Two Options

Since the existing StewardState uses `scores: [u32; MAX_VALIDATORS]` arrays (where MAX_VALIDATORS = 5,000), we need to migrate to u64. Two approaches are possible:

### Option 1: New Struct Version with Migration Instruction

Create a new `StewardStateV2` struct and implement a migration instruction to copy data from the old struct to the new one.

```rust
pub struct StewardStateV2 {
    pub state_tag: StewardStateEnum,
    pub validator_lamport_balances: [u64; MAX_VALIDATORS],
    pub scores: [u64; MAX_VALIDATORS],  // UPGRADED from u32
    pub sorted_score_indices: [u16; MAX_VALIDATORS],
    pub yield_scores: [u64; MAX_VALIDATORS],  // UPGRADED from u32
    pub sorted_yield_score_indices: [u16; MAX_VALIDATORS],
    // ... rest of fields remain the same ...
}
```

**Implementation:**
1. Deploy new program with both struct versions
2. Add `migrate_steward_state` instruction
3. Migration instruction:
   - Reallocates account with additional space
   - Copies all fields from V1 to V2
   - Converts u32 scores to u64
   - Updates discriminator
4. All subsequent instructions use V2

**Pros:**
- Clean separation between versions
- Optimal struct layout (no wasted space)
- Clear migration point

**Cons:**
- Complex migration logic required
- Need to maintain two struct versions temporarily

### Option 2: Add New Fields to Existing Struct

Keep the same `StewardState` struct but add new u64 arrays at the end, deprecating the old u32 arrays.

```rust
pub struct StewardState {
    // ... existing fields remain unchanged ...
    
    /// DEPRECATED - use scores_v2 instead
    pub scores: [u32; MAX_VALIDATORS],
    
    /// DEPRECATED - use yield_scores_v2 instead
    pub yield_scores: [u32; MAX_VALIDATORS],
    
    // ... other existing fields ...
    
    pub _padding0: [u8; STATE_PADDING_0_SIZE],
    
    // NEW FIELDS ADDED AT END:
    /// Score array supporting 64-bit encoded 4-tier scoring
    pub scores_v2: [u64; MAX_VALIDATORS],
    
    /// Yield scores with 64-bit precision
    pub yield_scores_v2: [u64; MAX_VALIDATORS],
}
```

**Implementation:**
1. Realloc existing accounts to add space for new arrays
2. Update compute_score to write to new arrays
3. Update sorting/delegation logic to use new arrays
4. Old arrays remain for backward compatibility

**Pros:**
- Simpler implementation
- Backward compatible (old clients still work)
- Single struct version to maintain

**Cons:**
- Wasted space: 2 × 5,000 × 4 = 40,000 bytes (40KB) for deprecated fields
- Account size increase: 80KB total (2 × 5,000 × 8 bytes for new arrays)
- Struct becomes less clean (deprecated fields remain)
- Current account ~184KB → new size ~264KB (still well within Solana's 10MB limit)

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
- **u64 Required**: Allocates 4 bits for inflation commission, 10 bits for MEV commission, 20 bits for validator age, and 30 bits for vote credits ratio

### Migration Options
1. **Option 1**: Create new struct version with migration instruction - cleaner but more complex
2. **Option 2**: Add new u64 arrays to existing struct - simpler but wastes 40KB

Both options are viable within Solana's 10MB account limit (current ~184KB → new ~264KB).

### Validator Age Tracking
- Adds two fields to ValidatorHistory using existing padding (no reallocation needed)
- Idempotent initialization automatically backfills historical data
- Enables unbounded age tracking beyond the 512-epoch circular buffer limit

The design provides deterministic sorting through single u64 comparison while maintaining backward compatibility throughout the transition.
