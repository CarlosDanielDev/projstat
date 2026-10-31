use crate::model::Liveness;

pub fn momentum(closed_7d: u32, opened_7d: u32) -> i32 {
    closed_7d as i32 - opened_7d as i32
}

pub fn milestone_progress(open: u32, closed: u32) -> f64 {
    if open + closed == 0 {
        0.0
    } else {
        (closed as f64) / ((open as f64) + (closed as f64))
    }
}

pub fn closeness(progress: f64) -> f64 {
    progress.clamp(0.0, 1.0)
}

pub fn pick_score(momentum: i32, closeness: f64) -> f64 {
    (momentum.max(0) as f64) * closeness
}

pub fn liveness(age_secs: i64) -> Liveness {
    if age_secs <= 172800 {
        Liveness::Active
    } else if age_secs <= 1209600 {
        Liveness::Stalling
    } else {
        Liveness::Abandoned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn momentum_is_closed_minus_opened() {
        assert_eq!(momentum(6, 0), 6);
        assert_eq!(momentum(2, 3), -1);
    }

    #[test]
    fn milestone_progress_is_closed_fraction() {
        assert_eq!(milestone_progress(4, 0), 0.0);
        assert_eq!(milestone_progress(0, 0), 0.0);
        assert_eq!(milestone_progress(3, 7), 0.7);
    }

    #[test]
    fn closeness_clamps_to_unit_range() {
        assert_eq!(closeness(-0.5), 0.0);
        assert_eq!(closeness(0.4), 0.4);
        assert_eq!(closeness(1.7), 1.0);
    }

    #[test]
    fn pick_score_ignores_negative_momentum() {
        assert_eq!(pick_score(-1, 0.8), 0.0);
        assert!((pick_score(6, 0.78) - 4.68).abs() < 1e-9);
    }

    #[test]
    fn liveness_thresholds() {
        assert_eq!(liveness(3600), Liveness::Active);
        assert_eq!(liveness(172_800), Liveness::Active);
        assert_eq!(liveness(600_000), Liveness::Stalling);
        assert_eq!(liveness(1_209_600), Liveness::Stalling);
        assert_eq!(liveness(9_999_999), Liveness::Abandoned);
    }
}
