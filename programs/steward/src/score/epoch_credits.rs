use anchor_lang::Result;
use validator_history::EpochCredits;

use crate::{constants::EPOCH_DEFAULT, errors::StewardError, score::calculate_epoch_credits};

/// `calculate_epoch_credits` over only the scorable epochs in the window, so unscorable epochs are
/// neither averaged in nor checked for delinquency.
pub fn calculate_scorable_epoch_credits(
    epoch_credits_window: &[EpochCredits],
    total_blocks_window: &[Option<u32>],
    epoch_credits_start: u16,
    scoring_delinquency_threshold_ratio: f64,
) -> Result<(f64, u8, f64, u16)> {
    let mut epochs = Vec::with_capacity(epoch_credits_window.len());
    let mut credits_window = Vec::with_capacity(epoch_credits_window.len());
    let mut blocks_window = Vec::with_capacity(epoch_credits_window.len());
    for (i, (epoch_credits, total_blocks)) in epoch_credits_window
        .iter()
        .zip(total_blocks_window.iter())
        .enumerate()
    {
        if let EpochCredits::Scored(credits) = epoch_credits {
            epochs.push(
                epoch_credits_start
                    .checked_add(i as u16)
                    .ok_or(StewardError::ArithmeticError)?,
            );
            credits_window.push(*credits);
            blocks_window.push(*total_blocks);
        }
    }

    if epochs.is_empty() {
        // Nothing to judge the validator by, so it isn't treated as delinquent
        return Ok((0., 1, 1., EPOCH_DEFAULT));
    }

    let (vote_credits_ratio, delinquency_score, delinquency_ratio, delinquency_index) =
        calculate_epoch_credits(
            &credits_window,
            &blocks_window,
            0,
            scoring_delinquency_threshold_ratio,
        )?;
    let delinquency_epoch = if delinquency_score == 0 {
        *epochs
            .get(delinquency_index as usize)
            .ok_or(StewardError::ArithmeticError)?
    } else {
        EPOCH_DEFAULT
    };

    Ok((
        vote_credits_ratio,
        delinquency_score,
        delinquency_ratio,
        delinquency_epoch,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scorable_epoch_credits_matches_tower_scoring() {
        let credits = [Some(16_000), None, Some(8_000), Some(16_000)];
        let total_blocks = [Some(1_000), Some(1_000), None, Some(1_000)];
        assert_eq!(
            calculate_scorable_epoch_credits(
                &credits.map(EpochCredits::Scored),
                &total_blocks,
                100,
                0.9
            )
            .unwrap(),
            calculate_epoch_credits(&credits, &total_blocks, 100, 0.9).unwrap()
        );
    }

    #[test]
    fn test_scorable_epoch_credits_skips_unscorable_epochs() {
        let window = [
            EpochCredits::Scored(Some(16_000)),
            EpochCredits::Unscorable,
            EpochCredits::Scored(Some(16_000)),
        ];
        let (vote_credits_ratio, delinquency_score, _, delinquency_epoch) =
            calculate_scorable_epoch_credits(&window, &[Some(1_000); 3], 100, 0.97).unwrap();
        assert_eq!(vote_credits_ratio, 1.);
        assert_eq!(delinquency_score, 1);
        assert_eq!(delinquency_epoch, EPOCH_DEFAULT);

        let window = [
            EpochCredits::Unscorable,
            EpochCredits::Scored(Some(16_000)),
            EpochCredits::Scored(Some(0)),
        ];
        let (_, delinquency_score, delinquency_ratio, delinquency_epoch) =
            calculate_scorable_epoch_credits(&window, &[Some(1_000); 3], 100, 0.97).unwrap();
        assert_eq!(delinquency_score, 0);
        assert_eq!(delinquency_ratio, 0.);
        assert_eq!(delinquency_epoch, 102);

        assert_eq!(
            calculate_scorable_epoch_credits(
                &[EpochCredits::Unscorable; 3],
                &[Some(1_000); 3],
                100,
                0.97
            )
            .unwrap(),
            (0., 1, 1., EPOCH_DEFAULT)
        );
    }
}
