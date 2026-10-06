use anchor_lang::Result;
use validator_history::EpochCreditsRatio;

use crate::{constants::EPOCH_DEFAULT, errors::StewardError};

/// Averages the participation ratios of the scorable epochs in the window, and flags the first one
/// that falls below the delinquency threshold. Unscorable epochs are neither averaged in nor checked
/// for delinquency, so a validator isn't judged on an epoch whose inputs are missing or ambiguous.
///
/// Returns `(average_ratio, delinquency_score, delinquency_ratio, delinquency_epoch)`.
pub fn calculate_scorable_epoch_credits(
    epoch_credits_ratio_window: &[EpochCreditsRatio],
    epoch_credits_start: u16,
    scoring_delinquency_threshold_ratio: f64,
) -> Result<(f64, u8, f64, u16)> {
    let mut scored = Vec::with_capacity(epoch_credits_ratio_window.len());
    for (i, participation) in epoch_credits_ratio_window.iter().enumerate() {
        if let EpochCreditsRatio::Scored(ratio) = *participation {
            let epoch = epoch_credits_start
                .checked_add(i as u16)
                .ok_or(StewardError::ArithmeticError)?;
            scored.push((epoch, ratio));
        }
    }

    if scored.is_empty() {
        return Ok((0., 1, 1., EPOCH_DEFAULT));
    }

    // Delinquency heuristic - not actual delinquency
    let mut delinquency_score = 1u8;
    let mut delinquency_ratio = 1.0;
    let mut delinquency_epoch = EPOCH_DEFAULT;
    for &(epoch, ratio) in scored.iter() {
        if ratio < scoring_delinquency_threshold_ratio {
            delinquency_score = 0;
            delinquency_ratio = ratio;
            delinquency_epoch = epoch;
            break;
        }
    }

    let average_ratio = scored.iter().map(|&(_, ratio)| ratio).sum::<f64>() / scored.len() as f64;

    Ok((
        average_ratio,
        delinquency_score,
        delinquency_ratio,
        delinquency_epoch,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scorable_epoch_credits_averages_scored_epochs() {
        let window = [
            EpochCreditsRatio::Scored(1.),
            EpochCreditsRatio::Scored(0.5),
            EpochCreditsRatio::Scored(0.9),
        ];
        let (average_ratio, delinquency_score, delinquency_ratio, delinquency_epoch) =
            calculate_scorable_epoch_credits(&window, 100, 0.4).unwrap();
        assert!((average_ratio - 0.8).abs() < 1e-9);
        assert_eq!(delinquency_score, 1);
        assert_eq!(delinquency_ratio, 1.);
        assert_eq!(delinquency_epoch, EPOCH_DEFAULT);
    }

    #[test]
    fn test_scorable_epoch_credits_flags_first_delinquent_epoch() {
        let window = [
            EpochCreditsRatio::Scored(1.),
            EpochCreditsRatio::Scored(0.5),
            EpochCreditsRatio::Scored(0.2),
        ];
        let (_, delinquency_score, delinquency_ratio, delinquency_epoch) =
            calculate_scorable_epoch_credits(&window, 100, 0.97).unwrap();
        assert_eq!(delinquency_score, 0);
        assert_eq!(delinquency_ratio, 0.5);
        assert_eq!(delinquency_epoch, 101);
    }

    #[test]
    fn test_scorable_epoch_credits_skips_unscorable_epochs() {
        // The unscorable epoch is neither averaged in nor treated as delinquent
        let window = [
            EpochCreditsRatio::Scored(1.),
            EpochCreditsRatio::Unscorable,
            EpochCreditsRatio::Scored(1.),
        ];
        let (average_ratio, delinquency_score, _, delinquency_epoch) =
            calculate_scorable_epoch_credits(&window, 100, 0.97).unwrap();
        assert_eq!(average_ratio, 1.);
        assert_eq!(delinquency_score, 1);
        assert_eq!(delinquency_epoch, EPOCH_DEFAULT);

        // Delinquency epochs stay aligned to the real epoch numbers despite the skip
        let window = [
            EpochCreditsRatio::Unscorable,
            EpochCreditsRatio::Scored(1.),
            EpochCreditsRatio::Scored(0.),
        ];
        let (_, delinquency_score, delinquency_ratio, delinquency_epoch) =
            calculate_scorable_epoch_credits(&window, 100, 0.97).unwrap();
        assert_eq!(delinquency_score, 0);
        assert_eq!(delinquency_ratio, 0.);
        assert_eq!(delinquency_epoch, 102);

        assert_eq!(
            calculate_scorable_epoch_credits(&[EpochCreditsRatio::Unscorable; 3], 100, 0.97).unwrap(),
            (0., 1, 1., EPOCH_DEFAULT)
        );
    }
}
