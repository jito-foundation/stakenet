# Four-Tier Validator Scoring System Specification

## Overview
Implementation of a new 4-tier tiebreaker scoring system for validator ranking that enables deterministic sorting while maintaining backward compatibility and requiring zero on-chain migrations or account reallocations.

## The 4-Tier Ranking System

The system ranks validators by four metrics in priority order:
1. **Inflation Commission** (lower is better) - Existing metric
2. **MEV Commission** (lower is better) - Existing metric  
3. **Validator Age** (higher is better) - **NEW metric**
4. **Vote Credits Ratio** (higher is better) - Existing metric

## Bit Encoding Strategy

### Binary Score Encoding into u32
All four metrics are encoded into a single u32 value using bit manipulation, enabling efficient numeric comparison for sorting. This aligns with the StewardState's `scores: [u32; MAX_VALIDATORS]` array.

```
32-bit layout:
┌────────────┬──────────────┬─────────────────┬──────────────────────┐
│ Bits 28-31 │ Bits 18-27   │ Bits 10-17      │ Bits 0-9             │
│ (4 bits)   │ (10 bits)    │ (8 bits)        │ (10 bits)            │
├────────────┼──────────────┼─────────────────┼──────────────────────┤
│ Inflation  │ MEV          │ Validator Age   │ Vote Credits         │
│ Commission │ Commission   │ (÷16 bucketed)  │ Ratio (0.90-1.00)    │
└────────────┴──────────────┴─────────────────┴──────────────────────┘
```

### Field Specifications

1. **Inflation Commission (4 bits)**
   - Range: 0-15 (covers 0-10% commission range)
   - Inverted: `15 - min(commission, 10)` (lower commission = higher value)
   - Justification: Validators with >10% commission are disqualified by binary scoring filters

2. **MEV Commission (10 bits)**  
   - Range: 0-1023 (covers 0-1000 basis points = 0-10%)
   - Inverted: `1023 - min(mev_commission, 1000)` (lower commission = higher value)
   - Justification: Optimized for the 10% filter threshold

3. **Validator Age (8 bits)**
   - Range: 0-255 buckets (each bucket = 16 epochs)
   - Encoding: `min(validator_age / 16, 255)`
   - Effective range: 0-4080 epochs (~34 years at 3 days/epoch)
   - Direct encoding (older = higher value)
   - NEW METRIC: Counts epochs with non-zero vote credits
   - Bucketing rationale: 16-epoch granularity (~48 days) is sufficient for age differentiation

4. **Vote Credits Ratio (10 bits)**
   - Range: 0-1023 levels
   - Special encoding for high-performance focus:
     - Ratios < 0.90: Encoded as 0 (bucketed together as poor performers)
     - Ratios 0.90-1.00: Linear encoding across full 10-bit range
     - Formula: `if ratio < 0.90 { 0 } else { ((ratio - 0.90) * 10230) as u32 }`
   - Precision in 0.90-1.00 range: ~0.01% (can distinguish 0.9500 vs 0.9501)
   - Justification: Validators below 90% vote credits are significantly underperforming

### Why Bit Shifting Works
The bit shifting creates a hierarchy where:
- Higher-order bits always dominate in numeric comparison
- A validator with 5% inflation commission will ALWAYS rank higher than one with 6%, regardless of other metrics
- Within the same inflation commission tier, MEV commission determines ranking
- And so on through validator age and vote credits

This deterministic ordering is achieved through simple u32 comparison: `validator_a.score > validator_b.score`

## No Migration Required - Score Field Reuse

### Existing Infrastructure
- The StewardState has a `scores: [u32; MAX_VALIDATORS]` array
- Currently stores `(score.score * 1_000_000_000.) as u32`
- **We directly store the encoded u32 value** without float conversion
- The ScoreComponentsV4 event struct will also use u32 fields
- No account reallocation or migration needed on-chain

## Event Structure Update (ScoreComponentsV3 → ScoreComponentsV4)

### Updated Event Struct with u32 Fields
```rust
pub struct ScoreComponentsV4 {
    /// Encoded 4-tier tiebreaker score (u32)
    pub score: u32,  // CHANGED from f64
    
    /// yield_score encoded with appropriate precision
    pub yield_score: u32,  // CHANGED from f64
    
    /// Binary scores (0 or 1)
    pub mev_commission_score: u32,  // CHANGED from f64
    pub blacklisted_score: u32,  // CHANGED from f64
    pub superminority_score: u32,  // CHANGED from f64
    pub delinquency_score: u32,  // CHANGED from f64
    pub running_jito_score: u32,  // CHANGED from f64
    pub commission_score: u32,  // CHANGED from f64
    pub historical_commission_score: u32,  // CHANGED from f64
    pub merkle_root_upload_authority_score: u32,  // CHANGED from f64
    pub priority_fee_commission_score: u32,  // CHANGED from f64
    pub priority_fee_merkle_root_upload_authority_score: u32,  // CHANGED from f64
    
    /// Vote credits ratio with 5 decimal places (multiply by 100_000)
    pub vote_credits_ratio: u32,  // CHANGED from f64
    
    /// Validator age - number of epochs with non-zero vote credits
    pub validator_age: u32,  // NEW FIELD
    
    // ... other fields remain unchanged ...
}
```

### Why No Migration Needed
- ScoreComponents is an **event struct**, not an on-chain account
- Events are emitted for off-chain consumption
- Renaming to V4 clearly indicates the new schema version
- Off-chain systems can adapt to the new event format

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

1. **Zero Migration Cost**: No account reallocations, no migration scripts
2. **Full Backward Compatibility**: Old clients continue to function
3. **Efficient Sorting**: Single u32 comparison for complex multi-tier ranking
4. **Deterministic Results**: Eliminates randomness in validator selection
5. **Historical Preservation**: Validator age tracked beyond buffer limits
6. **Transparent Metrics**: All ranking components visible in events
7. **Optimized Storage**: Uses native u32 throughout, avoiding float conversions
8. **Precision Focus**: High precision (0.01%) for top performers (90-100% vote credits)

## Summary

This implementation achieves a sophisticated 4-tier ranking system with:
- **No on-chain migrations required**
- **No account reallocations needed**
- **Perfect backward compatibility**
- **Efficient single u32 comparison for sorting**
- **Long-term validator age tracking beyond circular buffer constraints**
- **Optimized bit allocation** focusing precision where it matters most

The design leverages existing infrastructure (u32 scores array) and unused padding bytes to add new functionality without disrupting the existing system. The move to u32 throughout eliminates unnecessary float conversions and aligns perfectly with the StewardState's storage format, making it an ideal upgrade path for the validator scoring mechanism.