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
- Risk during migration (one-time operation)
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

## Recommendation: Option 2

We recommend **Option 2 (Add New Fields)** for the following reasons:

1. **Lower Risk**: No complex migration logic that could fail
2. **Simpler Implementation**: Just realloc and add fields
3. **Backward Compatible**: Old clients continue working during transition
4. **Gradual Rollout**: Can update clients over time
5. **Space Within Limits**: 40KB overhead and 264KB total size is well within Solana's 10MB account limit

The implementation would proceed as:
1. Deploy program update with new fields
2. Realloc accounts on first use
3. Start writing to both old and new arrays
4. Gradually update off-chain clients to read new arrays
5. Eventually stop writing to old arrays (keeping them for compatibility)

## Event Structure Update (ScoreComponentsV3 → ScoreComponentsV4)

### Updated Event Struct with u64 Fields
```rust
pub struct ScoreComponentsV4 {
    /// Encoded 4-tier tiebreaker score (now u64)
    pub score: u64,  // CHANGED from f64 to u64
    
    /// yield_score with full precision
    pub yield_score: u64,  // CHANGED from f64 to u64
    
    /// Binary scores (0 or 1)
    pub mev_commission_score: u8,  // CHANGED from f64 to u8
    pub blacklisted_score: u8,  // CHANGED from f64 to u8
    pub superminority_score: u8,  // CHANGED from f64 to u8
    pub delinquency_score: u8,  // CHANGED from f64 to u8
    pub running_jito_score: u8,  // CHANGED from f64 to u8
    pub commission_score: u8,  // CHANGED from f64 to u8
    pub historical_commission_score: u8,  // CHANGED from f64 to u8
    pub merkle_root_upload_authority_score: u8,  // CHANGED from f64 to u8
    pub priority_fee_commission_score: u8,  // CHANGED from f64 to u8
    pub priority_fee_merkle_root_upload_authority_score: u8,  // CHANGED from f64 to u8
    
    /// Vote credits ratio with full precision (multiply by u32::MAX)
    pub vote_credits_ratio: u32,  // CHANGED from f64
    
    /// Validator age - number of epochs with non-zero vote credits
    pub validator_age: u32,  // NEW FIELD
    
    /// MEV commission in basis points
    pub mev_commission_bps: u16,  // NEW FIELD for transparency
    
    /// Inflation commission in basis points
    pub commission_bps: u16,  // NEW FIELD for transparency
}
```

### Event Structure Benefits
- **Binary scores as u8**: More efficient for true/false values
- **Full u64 precision**: Matches the on-chain score storage
- **Transparent components**: Individual commission values exposed
- **Efficient serialization**: Smaller event size with appropriate types

## Validator History Account Extension

### New Persistent Fields
Two fields added to the ValidatorHistory account to track validator age beyond the 512-epoch circular buffer limit:

```rust
pub struct ValidatorHistory {
    // ... existing fields ...
    
    // Persistent validator age tracking
    pub validator_age: u32,                      // 4 bytes - Total epochs with non-zero vote credits
    pub validator_age_last_updated_epoch: u16,   // 2 bytes - Last epoch we updated the age counter
    
    pub _padding1: [u8; 226],  // Reduced from 232 bytes (was 232, now 226)
}
```

### Storage Optimization
- **Total new storage**: 6 bytes (4 + 2)
- **Source**: Repurposed from existing padding
- **Account size**: Remains exactly 65,848 bytes
- **Result**: Zero-copy struct maintains identical size

### Backward Compatibility

#### Read Compatibility (Both Directions)
1. **Old clients reading new accounts**: ✅
   - See the 6 bytes as part of padding (harmless)
   - All other fields at same offsets
   - No deserialization errors

2. **New clients reading old accounts**: ✅
   - Interpret padding bytes as `validator_age = 0` and `validator_age_last_updated_epoch = 0`
   - Triggers automatic backfill on first write
   - Idempotent design handles transition gracefully

#### Write Safety
- All writes go through the on-chain program
- Once deployed, all writes use the new structure
- No risk of data corruption

### Automatic Backfill Mechanism
The implementation includes idempotent backfill logic:
- When `validator_age_last_updated_epoch == 0`: Performs one-time historical backfill
- Counts all epochs in the circular buffer with non-zero vote credits
- Updates are idempotent - safe to call multiple times

## Implementation Benefits

1. **Deterministic Sorting**: Single u64 comparison for complex multi-tier ranking
2. **Full Precision**: All metrics encoded without loss of granularity
3. **Backward Compatibility**: Old clients continue functioning with deprecated fields
4. **Historical Preservation**: Validator age tracked beyond buffer limits
5. **Transparent Metrics**: All ranking components visible in events
6. **Future Proof**: 64 bits provides ample space for metric precision

## Summary

This implementation achieves a sophisticated 4-tier ranking system through:

1. **Migration to u64**: Provides necessary bit space for all four ranking metrics
2. **Account Reallocation**: One-time expansion to accommodate new arrays
3. **Dual-Write Strategy**: Maintains backward compatibility during transition
4. **Efficient Sorting**: Single u64 comparison determines ranking
5. **Validator History Extension**: Long-term age tracking using existing padding

The design acknowledges that 32 bits, even with aggressive bucketing, cannot provide adequate precision for four distinct ranking metrics. The migration to u64, while requiring account reallocation, provides a clean path forward with minimal risk and maximum compatibility.
