/// Calculate running bam score
///
/// Returns 1 if the validator has been connected to BAM for at least
/// `minimum_epochs` out of the observed epochs in the window, otherwise returns 0.
/// Epochs where BAM status was not uploaded (`None`) are excluded from the threshold,
/// effectively reducing `minimum_epochs` by the number of missed uploads.
pub fn calculate_running_bam_score(
    is_bam_connected_window: &[Option<u8>],
    minimum_epochs: u8,
) -> u8 {
    let num_missed_epochs = is_bam_connected_window
        .iter()
        .filter(|entry| entry.is_none())
        .count();
    let bam_connected_count = is_bam_connected_window
        .iter()
        .filter(|entry| matches!(entry, Some(1)))
        .count();

    if bam_connected_count >= (minimum_epochs as usize).saturating_sub(num_missed_epochs) {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_running_bam_score_all_connected() {
        let window: Vec<Option<u8>> = vec![Some(1), Some(1), Some(1), Some(1), Some(1)];
        assert_eq!(calculate_running_bam_score(&window, 3), 1);
    }

    #[test]
    fn test_calculate_running_bam_score_exact_threshold() {
        let window: Vec<Option<u8>> = vec![Some(1), Some(0), Some(1), Some(1), Some(0)];
        // 3 connected out of 5, minimum is 3
        assert_eq!(calculate_running_bam_score(&window, 3), 1);
    }

    #[test]
    fn test_calculate_running_bam_score_below_threshold() {
        let window: Vec<Option<u8>> = vec![Some(1), Some(0), Some(1), Some(0), Some(0)];
        // 2 connected out of 5, minimum is 3
        assert_eq!(calculate_running_bam_score(&window, 3), 0);
    }

    #[test]
    fn test_calculate_running_bam_score_none_entries() {
        // None entries (missing data) reduce the effective minimum threshold.
        // window: 3 connected, 2 missing → effective window size = 3
        let window: Vec<Option<u8>> = vec![Some(1), None, Some(1), None, Some(1)];

        // minimum=4: threshold = 4-2 = 2, 3 >= 2 → passes
        assert_eq!(calculate_running_bam_score(&window, 4), 1);
        // minimum=3: threshold = 3-2 = 1, 3 >= 1 → passes
        assert_eq!(calculate_running_bam_score(&window, 3), 1);
        // minimum=6: threshold = 6-2 = 4, 3 >= 4 → fails
        assert_eq!(calculate_running_bam_score(&window, 6), 0);

        // None epochs with some non-connected epochs: 2 connected, 2 missing, 1 not connected
        let window2: Vec<Option<u8>> = vec![Some(1), None, Some(0), None, Some(1)];
        // minimum=4: threshold = 4-2 = 2, 2 >= 2 → passes
        assert_eq!(calculate_running_bam_score(&window2, 4), 1);
        // minimum=5: threshold = 5-2 = 3, 2 >= 3 → fails
        assert_eq!(calculate_running_bam_score(&window2, 5), 0);
    }

    #[test]
    fn test_calculate_running_bam_score_zero_minimum() {
        // With minimum 0, any window should pass
        let window: Vec<Option<u8>> = vec![Some(0), Some(0), Some(0)];
        assert_eq!(calculate_running_bam_score(&window, 0), 1);
    }

    #[test]
    fn test_calculate_running_bam_score_empty_window() {
        let window: Vec<Option<u8>> = vec![];
        assert_eq!(calculate_running_bam_score(&window, 0), 1);
        assert_eq!(calculate_running_bam_score(&window, 1), 0);
    }
}
