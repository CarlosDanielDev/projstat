use chrono::{DateTime, Utc};

use crate::cache::Cache;
use crate::config::Sort;
use crate::metrics;
use crate::model::{Metrics, RepoRef};

/// In-memory dashboard state. Pure logic: no rendering and no `gh` calls.
#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub metrics: Vec<Metrics>,
    pub selected: usize,
    pub sort: Sort,
    pub pick: Option<RepoRef>,
    pub fetched_at: Option<DateTime<Utc>>,
    pub refreshing: bool,
    pub errors: Vec<String>,
}

impl App {
    pub fn new(cache: Option<Cache>, sort: Sort) -> App {
        let (metrics, fetched_at) = match cache {
            Some(c) => (c.metrics, Some(c.fetched_at)),
            None => (Vec::new(), None),
        };
        let mut app = App {
            metrics,
            selected: 0,
            sort,
            pick: None,
            fetched_at,
            refreshing: false,
            errors: Vec::new(),
        };
        app.pick = app.compute_pick();
        app
    }

    /// Indices into `self.metrics`, ordered according to `self.sort`.
    pub fn sorted_indices(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.metrics.len()).collect();
        let m = &self.metrics;
        match self.sort {
            Sort::Momentum => idx.sort_by_key(|&i| {
                std::cmp::Reverse(metrics::momentum(m[i].closed_7d, m[i].opened_7d))
            }),
            Sort::Progress => idx.sort_by(|&a, &b| {
                progress(&m[b])
                    .partial_cmp(&progress(&m[a]))
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            Sort::Name => idx.sort_by_key(|&i| m[i].repo.to_string()),
            Sort::Last => idx.sort_by_key(|&i| std::cmp::Reverse(m[i].last_activity)),
        }
        idx
    }

    /// The repo with the highest `pick_score`; ties go to the newest
    /// `last_activity`. `None` when there are no metrics.
    pub fn compute_pick(&self) -> Option<RepoRef> {
        let mut best: Option<(f64, &Metrics)> = None;
        for m in &self.metrics {
            let mom = metrics::momentum(m.closed_7d, m.opened_7d);
            let close = match &m.next_milestone {
                Some(ms) => metrics::closeness(metrics::milestone_progress(
                    ms.open_issues,
                    ms.closed_issues,
                )),
                None => 0.0,
            };
            let score = metrics::pick_score(mom, close);
            let better = match best {
                None => true,
                Some((bs, bm)) => score > bs || (score == bs && m.last_activity > bm.last_activity),
            };
            if better {
                best = Some((score, m));
            }
        }
        best.map(|(_, m)| m.repo.clone())
    }

    pub fn select_next(&mut self) {
        let len = self.metrics.len();
        if len == 0 {
            return;
        }
        self.selected = (self.selected + 1) % len;
    }

    pub fn select_prev(&mut self) {
        let len = self.metrics.len();
        if len == 0 {
            return;
        }
        self.selected = (self.selected + len - 1) % len;
    }

    pub fn cycle_sort(&mut self) {
        self.sort = match self.sort {
            Sort::Momentum => Sort::Progress,
            Sort::Progress => Sort::Name,
            Sort::Name => Sort::Last,
            Sort::Last => Sort::Momentum,
        };
    }

    pub fn apply_refresh(&mut self, metrics: Vec<Metrics>, skipped: Vec<String>) {
        self.metrics = metrics;
        self.fetched_at = Some(Utc::now());
        self.refreshing = false;
        self.errors.extend(skipped);
        self.pick = self.compute_pick();
        self.selected = self.selected.min(self.metrics.len().saturating_sub(1));
    }
}

/// Milestone completion for sorting; `None` milestone counts as 0.0.
fn progress(m: &Metrics) -> f64 {
    match &m.next_milestone {
        Some(ms) => metrics::milestone_progress(ms.open_issues, ms.closed_issues),
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Milestone;
    use chrono::TimeZone;

    fn ts(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap()
    }

    fn repo(name: &str) -> RepoRef {
        RepoRef::parse(&format!("octo/{name}")).unwrap()
    }

    fn ms(open: u32, closed: u32) -> Milestone {
        Milestone {
            title: "v1".into(),
            open_issues: open,
            closed_issues: closed,
            due_on: None,
        }
    }

    fn metric(
        name: &str,
        closed_7d: u32,
        opened_7d: u32,
        milestone: Option<Milestone>,
        last: DateTime<Utc>,
    ) -> Metrics {
        Metrics {
            repo: repo(name),
            next_milestone: milestone,
            open_issues: 0,
            closed_7d,
            opened_7d,
            last_activity: last,
            releases_shipped: 0,
        }
    }

    fn fixtures() -> Vec<Metrics> {
        vec![
            // momentum 2, progress 0.5 -> score 1.0
            metric("alpha", 4, 2, Some(ms(5, 5)), ts(2026, 10, 1)),
            // momentum 6, progress 0.9 -> score 5.4 (winner)
            metric("beta", 6, 0, Some(ms(1, 9)), ts(2026, 10, 2)),
            // no milestone -> score 0.0
            metric("gamma", 9, 0, None, ts(2026, 10, 30)),
            // negative momentum -> score 0.0
            metric("delta", 0, 3, Some(ms(0, 4)), ts(2026, 10, 29)),
        ]
    }

    fn cache_with(metrics: Vec<Metrics>) -> Cache {
        Cache {
            fetched_at: ts(2026, 10, 30),
            metrics,
        }
    }

    #[test]
    fn compute_pick_chooses_highest_score_and_ignores_zero_score_repos() {
        let mut app = App::new(None, Sort::Momentum);
        app.metrics = fixtures();
        assert_eq!(app.compute_pick(), Some(repo("beta")));
    }

    #[test]
    fn compute_pick_ties_go_to_newest_activity() {
        let a = metric("alpha", 2, 0, Some(ms(1, 1)), ts(2026, 10, 1));
        let b = metric("beta", 2, 0, Some(ms(1, 1)), ts(2026, 10, 5));
        let app = App::new(Some(cache_with(vec![a, b])), Sort::Momentum);
        assert_eq!(app.compute_pick(), Some(repo("beta")));
    }

    #[test]
    fn select_next_wraps_at_end() {
        let mut app = App::new(Some(cache_with(fixtures())), Sort::Momentum);
        app.selected = 3;
        app.select_next();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn select_prev_wraps_at_start() {
        let mut app = App::new(Some(cache_with(fixtures())), Sort::Momentum);
        app.select_prev();
        assert_eq!(app.selected, 3);
    }

    #[test]
    fn select_is_noop_when_empty() {
        let mut app = App::new(None, Sort::Momentum);
        app.select_next();
        app.select_prev();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn apply_refresh_replaces_metrics_sets_time_and_records_skipped() {
        let mut app = App::new(None, Sort::Momentum);
        app.refreshing = true;
        app.apply_refresh(fixtures(), vec!["octo/broken: 404".into()]);
        assert_eq!(app.metrics.len(), 4);
        assert!(app.fetched_at.is_some());
        assert!(!app.refreshing);
        assert_eq!(app.errors, vec!["octo/broken: 404".to_string()]);
        assert_eq!(app.pick, Some(repo("beta")));
    }

    #[test]
    fn apply_refresh_clamps_selected() {
        let mut app = App::new(Some(cache_with(fixtures())), Sort::Momentum);
        app.selected = 3;
        app.apply_refresh(vec![metric("alpha", 1, 0, None, ts(2026, 10, 1))], vec![]);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn new_without_cache_is_empty_dashboard() {
        let app = App::new(None, Sort::Name);
        assert!(app.metrics.is_empty());
        assert_eq!(app.fetched_at, None);
        assert_eq!(app.pick, None);
        assert_eq!(app.selected, 0);
        assert!(!app.refreshing);
        assert!(app.errors.is_empty());
        assert_eq!(app.sort, Sort::Name);
        assert!(app.sorted_indices().is_empty());
    }

    #[test]
    fn new_with_cache_seeds_state() {
        let app = App::new(Some(cache_with(fixtures())), Sort::Momentum);
        assert_eq!(app.metrics.len(), 4);
        assert_eq!(app.fetched_at, Some(ts(2026, 10, 30)));
        assert_eq!(app.pick, Some(repo("beta")));
    }

    #[test]
    fn sorted_indices_by_name_and_last_and_progress() {
        let mut app = App::new(Some(cache_with(fixtures())), Sort::Name);
        // alpha, beta, delta, gamma -> indices 0, 1, 3, 2
        assert_eq!(app.sorted_indices(), vec![0, 1, 3, 2]);

        app.sort = Sort::Last;
        // gamma (30), delta (29), beta (2), alpha (1)
        assert_eq!(app.sorted_indices(), vec![2, 3, 1, 0]);

        app.sort = Sort::Progress;
        // delta 1.0, beta 0.9, alpha 0.5, gamma 0.0 (no milestone)
        assert_eq!(app.sorted_indices(), vec![3, 1, 0, 2]);
    }

    #[test]
    fn sorted_indices_by_momentum() {
        let app = App::new(Some(cache_with(fixtures())), Sort::Momentum);
        // gamma 9, beta 6, alpha 2, delta -3
        assert_eq!(app.sorted_indices(), vec![2, 1, 0, 3]);
    }

    #[test]
    fn cycle_sort_order() {
        let mut app = App::new(None, Sort::Momentum);
        app.cycle_sort();
        assert_eq!(app.sort, Sort::Progress);
        app.cycle_sort();
        assert_eq!(app.sort, Sort::Name);
        app.cycle_sort();
        assert_eq!(app.sort, Sort::Last);
        app.cycle_sort();
        assert_eq!(app.sort, Sort::Momentum);
    }
}
