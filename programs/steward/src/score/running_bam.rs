/// Returns 1 if the validator has been connected to BAM for at least
/// `minimum_epochs` out of the epochs in the window, otherwise returns 0.
pub fn calculate_running_bam_score(
    is_bam_connected_window: &[Option<u8>],
    minimum_epochs: u8,
) -> u8 {
    let bam_connected_count = is_bam_connected_window
        .iter()
        .filter(|entry| matches!(entry, Some(1)))
        .count();

    if bam_connected_count >= minimum_epochs as usize {
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
        // None entries (missing data) should not count as connected
        let window: Vec<Option<u8>> = vec![Some(1), None, Some(1), None, Some(1)];
        // 3 connected, 2 missing — minimum is 4
        assert_eq!(calculate_running_bam_score(&window, 4), 0);
        // minimum is 3
        assert_eq!(calculate_running_bam_score(&window, 3), 1);
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
