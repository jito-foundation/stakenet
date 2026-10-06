use anchor_lang::Result;
use validator_history::EpochCreditsRatio;

use crate::{constants::EPOCH_DEFAULT, errors::StewardError, Parameters};

/// Averages the participation ratios of the scorable epochs in the window, and flags the first one
/// that falls below the delinquency threshold.
///
/// Epochs are left out when they can't be measured: their inputs are missing, or they fall in the
/// alpenglow transition where credits mix the two eras. A validator isn't judged on those.
pub fn calculate_scorable_epoch_credits(
    epoch_credits_ratio_window: &[EpochCreditsRatio],
    params: &Parameters,
    epoch_credits_start: u16,
) -> Result<(f64, u8, f64, u16)> {
    let scoring_delinquency_threshold_ratio = params.scoring_delinquency_threshold_ratio;

    let mut scored = Vec::with_capacity(epoch_credits_ratio_window.len());
    for (i, participation) in epoch_credits_ratio_window.iter().enumerate() {
        if let EpochCreditsRatio::Scored(ratio) = *participation {
            let epoch = epoch_credits_start
                .checked_add(i as u16)
                .ok_or(StewardError::ArithmeticError)?;
            if params.is_alpenglow_transition_epoch(epoch) {
                continue;
            }
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
    use bytemuck::Zeroable;

    use super::*;

    /// Parameters with no alpenglow migration declared, so no epoch is a transition epoch
    fn params(scoring_delinquency_threshold_ratio: f64) -> Parameters {
        let mut params = Parameters::zeroed();
        params.scoring_delinquency_threshold_ratio = scoring_delinquency_threshold_ratio;
        params.alpenglow_migration_epoch = u16::MAX;
        params
    }

    #[test]
    fn test_scorable_epoch_credits_skips_transition_epochs() {
        // Epochs 100..=102, with the migration declared at 101, so 101 and 102 are skipped
        let window = [
            EpochCreditsRatio::Scored(1.),
            EpochCreditsRatio::Scored(0.),
            EpochCreditsRatio::Scored(0.),
        ];
        let mut params = params(0.97);
        params.alpenglow_migration_epoch = 101;

        let (average_ratio, delinquency_score, _, delinquency_epoch) =
            calculate_scorable_epoch_credits(&window, &params, 100).unwrap();
        assert_eq!(average_ratio, 1.);
        assert_eq!(delinquency_score, 1);
        assert_eq!(delinquency_epoch, EPOCH_DEFAULT);
    }

    #[test]
    fn test_scorable_epoch_credits_averages_scored_epochs() {
        let window = [
            EpochCreditsRatio::Scored(1.),
            EpochCreditsRatio::Scored(0.5),
            EpochCreditsRatio::Scored(0.9),
        ];
        let (average_ratio, delinquency_score, delinquency_ratio, delinquency_epoch) =
            calculate_scorable_epoch_credits(&window, &params(0.4), 100).unwrap();
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
            calculate_scorable_epoch_credits(&window, &params(0.97), 100).unwrap();
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
            calculate_scorable_epoch_credits(&window, &params(0.97), 100).unwrap();
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
            calculate_scorable_epoch_credits(&window, &params(0.97), 100).unwrap();
        assert_eq!(delinquency_score, 0);
        assert_eq!(delinquency_ratio, 0.);
        assert_eq!(delinquency_epoch, 102);

        assert_eq!(
            calculate_scorable_epoch_credits(
                &[EpochCreditsRatio::Unscorable; 3],
                &params(0.97),
                100
            )
            .unwrap(),
            (0., 1, 1., EPOCH_DEFAULT)
        );
    }
}
