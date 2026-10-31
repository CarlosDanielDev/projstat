pub fn humanize_age(secs: i64) -> String {
    if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}

pub fn progress_bar(pct: f64, width: usize) -> String {
    let p = pct.clamp(0.0, 1.0);
    let filled: usize = (p * width as f64).round() as usize;
    let mut result = String::new();
    for _ in 0..filled {
        result.push('█');
    }
    for _ in 0..width - filled {
        result.push('░');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_age_hours() {
        assert_eq!(humanize_age(7200), "2h");
    }

    #[test]
    fn humanize_age_days() {
        assert_eq!(humanize_age(86400), "1d");
        assert_eq!(humanize_age(1987200), "23d");
    }

    #[test]
    fn humanize_age_minutes_under_an_hour() {
        assert_eq!(humanize_age(59), "0m");
        assert_eq!(humanize_age(1800), "30m");
    }

    #[test]
    fn progress_bar_half() {
        let bar = progress_bar(0.5, 10);
        assert_eq!(bar.chars().filter(|&c| c == '█').count(), 5);
        assert_eq!(bar.chars().filter(|&c| c == '░').count(), 5);
    }

    #[test]
    fn progress_bar_clamps_above_one() {
        assert_eq!(progress_bar(1.5, 4), "████");
    }

    #[test]
    fn progress_bar_zero() {
        assert_eq!(progress_bar(0.0, 4), "░░░░");
    }
}
