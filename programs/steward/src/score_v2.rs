use anchor_lang::Result;
use validator_history::ValidatorHistory;

use crate::{
    constants::BASIS_POINTS_MAX,
    errors::StewardError,
};

/// Encode a 4-tier validator score into a u64 with the following bit layout:
/// Bits 56-63 (8 bits):  Inflation commission (inverted, 0-100%)
/// Bits 42-55 (14 bits): MEV commission (inverted, 0-10000 bps)
/// Bits 25-41 (17 bits): Validator age (direct, epochs)
/// Bits 0-24 (25 bits):  Epoch credits (direct value)
///
/// Higher scores are better in all cases.
pub fn encode_validator_score_v2(
    inflation_commission: u8,      // 0-100
    mev_commission_bps: u16,        // 0-10000
    validator_age: u32,             // epochs with non-zero vote credits
    epoch_credits: u32,             // direct epoch credits value
) -> Result<u64> {
    // Tier 1: Inflation commission (inverted so lower commission = higher score)
    let inflation_score = 100u64
        .saturating_sub(inflation_commission.min(100) as u64);
    
    // Tier 2: MEV commission (inverted so lower commission = higher score)
    let mev_score = (BASIS_POINTS_MAX as u64)
        .saturating_sub(mev_commission_bps.min(BASIS_POINTS_MAX) as u64);
    
    // Tier 3: Validator age (direct - older validators score higher)
    // Cap at 17 bits max value (131,071 epochs = ~716 years)
    let age_score = (validator_age as u64).min((1u64 << 17) - 1);
    
    // Tier 4: Epoch credits (direct value)
    // Cap at 25 bits max value (33,554,431)
    let credits_score = (epoch_credits as u64).min((1u64 << 25) - 1);
    
    // Combine into single u64
    let score = (inflation_score << 56)
        | (mev_score << 42)
        | (age_score << 25)
        | credits_score;
    
    Ok(score)
}

/// Extract the most recent MEV commission from validator history
pub fn get_mev_commission(validator: &ValidatorHistory, current_epoch: u16) -> u16 {
    // Look for MEV commission from current or recent epochs
    // Start with most recent and work backwards
    for entry in validator.history.arr.iter().rev() {
        if entry.epoch != u16::MAX 
            && entry.epoch <= current_epoch 
            && entry.mev_commission != u16::MAX {
            return entry.mev_commission;
        }
    }
    
    // Default to max if no data
    BASIS_POINTS_MAX
}

/// Extract the most recent commission from validator history
pub fn get_commission(validator: &ValidatorHistory, current_epoch: u16) -> u8 {
    // Look for commission from current or recent epochs
    // Start with most recent and work backwards
    for entry in validator.history.arr.iter().rev() {
        if entry.epoch != u16::MAX 
            && entry.epoch <= current_epoch 
            && entry.commission != u8::MAX {
            return entry.commission;
        }
    }
    
    // Default to max if no data
    100
}

/// Get epoch credits for the previous epoch
pub fn get_epoch_credits(validator: &ValidatorHistory, current_epoch: u16) -> Result<u32> {
    let target_epoch = current_epoch.checked_sub(1)
        .ok_or(StewardError::ArithmeticError)?;
    
    // Find the entry for the previous epoch
    for entry in validator.history.arr.iter().rev() {
        if entry.epoch == target_epoch && entry.epoch_credits != u32::MAX {
            return Ok(entry.epoch_credits);
        }
    }
    
    // No data for previous epoch
    Ok(0)
}

/// Calculate the new 4-tier validator score
pub fn calculate_validator_score_v2(
    validator: &ValidatorHistory,
    current_epoch: u16,
) -> Result<u64> {
    // Get components
    let commission = get_commission(validator, current_epoch);
    let mev_commission = get_mev_commission(validator, current_epoch);
    let validator_age = validator.validator_age();
    let epoch_credits = get_epoch_credits(validator, current_epoch)?;
    
    // Encode into u64
    encode_validator_score_v2(
        commission,
        mev_commission,
        validator_age,
        epoch_credits,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_encoding() {
        // Test perfect validator
        let score = encode_validator_score_v2(0, 0, 1000, 432_000).unwrap();
        
        // Extract components back
        let inflation = (score >> 56) & 0xFF;
        let mev = (score >> 42) & 0x3FFF;
        let age = (score >> 25) & 0x1FFFF;
        let credits = score & 0x1FFFFFF;
        
        assert_eq!(inflation, 100); // inverted: 0% commission = 100 score
        assert_eq!(mev, 10000);     // inverted: 0 bps = 10000 score
        assert_eq!(age, 1000);       // direct
        assert_eq!(credits, 432_000); // direct
    }
    
    #[test]
    fn test_score_ordering() {
        // Validator A: Lower inflation commission wins
        let score_a = encode_validator_score_v2(5, 100, 100, 400_000).unwrap();
        let score_b = encode_validator_score_v2(10, 100, 100, 400_000).unwrap();
        assert!(score_a > score_b);
        
        // Equal inflation, lower MEV commission wins
        let score_a = encode_validator_score_v2(5, 100, 100, 400_000).unwrap();
        let score_b = encode_validator_score_v2(5, 200, 100, 400_000).unwrap();
        assert!(score_a > score_b);
        
        // Equal inflation and MEV, higher age wins
        let score_a = encode_validator_score_v2(5, 100, 200, 400_000).unwrap();
        let score_b = encode_validator_score_v2(5, 100, 100, 400_000).unwrap();
        assert!(score_a > score_b);
        
        // All else equal, higher credits wins
        let score_a = encode_validator_score_v2(5, 100, 100, 432_000).unwrap();
        let score_b = encode_validator_score_v2(5, 100, 100, 400_000).unwrap();
        assert!(score_a > score_b);
    }
}