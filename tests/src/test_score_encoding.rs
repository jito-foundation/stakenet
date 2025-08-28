#[cfg(test)]
mod tests {
    fn encode_tiebreaker_score(
        inflation_commission: u8,
        mev_commission: u16,
        validator_age: u32,
        vote_credits_ratio: f64,
    ) -> f64 {
        // Inflation: 4 bits (60-63)
        let inflation_inverted =
            (10_u64.saturating_sub(inflation_commission.min(10) as u64)).min(15);
        let inflation_score = inflation_inverted << 60;

        // MEV: 10 bits (50-59)
        let mev_inverted = (1000_u64.saturating_sub(mev_commission.min(1000) as u64)).min(1023);
        let mev_score = mev_inverted << 50;

        // Validator age: 20 bits (30-49)
        let age_score = (validator_age.min(1048575) as u64) << 30;

        // Vote credits: 30 bits (0-29)
        let max_vote_credits = (1u64 << 30) - 1;
        let vote_score = (vote_credits_ratio * max_vote_credits as f64) as u64;

        let combined = inflation_score | mev_score | age_score | vote_score;
        f64::from_bits(combined)
    }

    #[test]
    fn test_vote_credits_tiebreaker() {
        // Two validators with same commissions and age, different vote credits
        let v1 = encode_tiebreaker_score(5, 500, 100, 0.95);
        let v2 = encode_tiebreaker_score(5, 500, 100, 0.94);

        assert!(
            v1 > v2,
            "Higher vote credits should win when other factors are equal"
        );
    }

    #[test]
    fn test_validator_age_tiebreaker() {
        // Same commissions, different ages
        let older = encode_tiebreaker_score(5, 500, 200, 0.94);
        let younger = encode_tiebreaker_score(5, 500, 100, 0.95);

        assert!(
            older > younger,
            "Older validator should win despite lower vote credits"
        );
    }

    #[test]
    fn test_mev_commission_tiebreaker() {
        // Different MEV commissions
        let lower_mev = encode_tiebreaker_score(5, 400, 100, 0.94);
        let higher_mev = encode_tiebreaker_score(5, 500, 200, 0.95);

        assert!(
            lower_mev > higher_mev,
            "Lower MEV commission should win despite worse age and credits"
        );
    }

    #[test]
    fn test_inflation_commission_priority() {
        // Different inflation commissions - highest priority
        let lower_inflation = encode_tiebreaker_score(3, 500, 100, 0.94);
        let higher_inflation = encode_tiebreaker_score(5, 400, 200, 0.95);

        assert!(
            lower_inflation > higher_inflation,
            "Lower inflation commission should win (highest priority)"
        );
    }

    #[test]
    fn test_vote_credits_precision() {
        // Test precision in vote credits (30 bits gives ~9-10 decimal places)
        // Note: f64 to u64 conversion may lose some precision at the extremes
        let v1 = encode_tiebreaker_score(5, 500, 100, 0.950001);
        let v2 = encode_tiebreaker_score(5, 500, 100, 0.950000);

        assert!(
            v1 > v2,
            "Should detect small differences in vote credits ratio"
        );
    }

    #[test]
    fn test_commission_boundaries() {
        // Test commission at boundaries
        // Important: We're comparing the f64 bit patterns directly, not as floating point numbers
        // Higher bit patterns = higher scores
        let zero_zero = encode_tiebreaker_score(0, 0, 100, 0.95);
        let one_one = encode_tiebreaker_score(1, 100, 100, 0.95);
        let five_five = encode_tiebreaker_score(5, 500, 100, 0.95);
        let ten_ten = encode_tiebreaker_score(10, 1000, 100, 0.95);

        // Convert back to u64 for reliable comparison
        let zero_bits = zero_zero.to_bits();
        let one_bits = one_one.to_bits();
        let five_bits = five_five.to_bits();
        let ten_bits = ten_ten.to_bits();

        // These should be in descending order when compared as u64
        assert!(zero_bits > one_bits, "0%/0bps should beat 1%/100bps");
        assert!(one_bits > five_bits, "1%/100bps should beat 5%/500bps");
        assert!(five_bits > ten_bits, "5%/500bps should beat 10%/1000bps");
    }

    #[test]
    fn test_validator_age_range() {
        // Test with very old validator (near 20-bit limit)
        let ancient = encode_tiebreaker_score(5, 500, 1000000, 0.95);
        let new = encode_tiebreaker_score(5, 500, 1, 0.95);

        assert!(ancient > new, "Ancient validator should beat new validator");
    }

    #[test]
    fn test_sorting_order() {
        // Create multiple validators and ensure they sort correctly
        let mut validators = vec![
            (
                "V1",
                5,
                500,
                100,
                0.95,
                encode_tiebreaker_score(5, 500, 100, 0.95),
            ),
            (
                "V2",
                3,
                500,
                100,
                0.95,
                encode_tiebreaker_score(3, 500, 100, 0.95),
            ), // Better inflation
            (
                "V3",
                5,
                400,
                100,
                0.95,
                encode_tiebreaker_score(5, 400, 100, 0.95),
            ), // Better MEV
            (
                "V4",
                5,
                500,
                200,
                0.95,
                encode_tiebreaker_score(5, 500, 200, 0.95),
            ), // Older
            (
                "V5",
                5,
                500,
                100,
                0.99,
                encode_tiebreaker_score(5, 500, 100, 0.99),
            ), // Better credits
        ];

        validators.sort_by(|a, b| b.5.partial_cmp(&a.5).unwrap());

        // Expected order: V2 (best inflation), V3 (better MEV), V4 (older), V5 (better credits), V1
        assert_eq!(
            validators[0].0, "V2",
            "Best inflation commission should be first"
        );
        assert_eq!(validators[1].0, "V3", "Better MEV should be second");
        assert_eq!(validators[2].0, "V4", "Older validator should be third");
        assert_eq!(
            validators[3].0, "V5",
            "Better vote credits should be fourth"
        );
        assert_eq!(validators[4].0, "V1", "Base validator should be last");
    }
}
