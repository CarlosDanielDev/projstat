use chrono::{DateTime, Utc};

use crate::cache::Cache;
use crate::config::Sort;
use crate::metrics;
use crate::model::{Metrics, RepoRef};

/// Which screen is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Dashboard,
    Picker,
}

/// State of the in-TUI pin picker. `pinned` holds `owner/name` strings.
#[derive(Debug, Clone, PartialEq)]
pub struct PickerState {
    pub candidates: Vec<RepoRef>,
    pub pinned: std::collections::BTreeSet<String>,
    pub filter: String,
    pub cursor: usize,
}

impl PickerState {
    /// Indices into `candidates` whose display string contains `filter`
    /// (case-insensitive), in candidate order.
    pub fn filtered(&self) -> Vec<usize> {
        let needle = self.filter.to_lowercase();
        self.candidates
            .iter()
            .enumerate()
            .filter(|(_, r)| r.to_string().to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect()
    }
}

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
    pub mode: Mode,
    pub picker: Option<PickerState>,
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
            mode: Mode::Dashboard,
            picker: None,
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

    /// Switches to the picker over `candidates`, seeding the pinned set
    /// from `already_pinned`.
    pub fn open_picker(&mut self, candidates: Vec<RepoRef>, already_pinned: &[RepoRef]) {
        self.mode = Mode::Picker;
        self.picker = Some(PickerState {
            candidates,
            pinned: already_pinned.iter().map(|r| r.to_string()).collect(),
            filter: String::new(),
            cursor: 0,
        });
    }

    pub fn close_picker(&mut self) {
        self.mode = Mode::Dashboard;
        self.picker = None;
    }

    /// Moves the picker cursor by `delta`, clamped to the filtered list.
    pub fn picker_move(&mut self, delta: i32) {
        if let Some(p) = self.picker.as_mut() {
            let len = p.filtered().len();
            p.cursor = if len == 0 {
                0
            } else {
                (p.cursor as i64 + i64::from(delta)).clamp(0, len as i64 - 1) as usize
            };
        }
    }

    /// Toggles the highlighted filtered candidate in the pinned set.
    pub fn picker_toggle(&mut self) {
        if let Some(p) = self.picker.as_mut() {
            let filtered = p.filtered();
            if let Some(&idx) = filtered.get(p.cursor) {
                let key = p.candidates[idx].to_string();
                if !p.pinned.remove(&key) {
                    p.pinned.insert(key);
                }
            }
        }
    }

    pub fn picker_filter_push(&mut self, c: char) {
        if let Some(p) = self.picker.as_mut() {
            p.filter.push(c);
            p.cursor = 0;
        }
    }

    pub fn picker_filter_pop(&mut self) {
        if let Some(p) = self.picker.as_mut() {
            p.filter.pop();
            p.cursor = 0;
        }
    }

    /// The pinned set as parsed `RepoRef`s, sorted by `owner/name`.
    /// Unparsable entries are skipped.
    pub fn picker_pins(&self) -> Vec<RepoRef> {
        match &self.picker {
            Some(p) => p.pinned.iter().filter_map(|s| RepoRef::parse(s)).collect(),
            None => Vec::new(),
        }
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

    fn picker_app(candidates: &[&str], already: &[&str]) -> App {
        let mut app = App::new(None, Sort::Momentum);
        let cands = candidates
            .iter()
            .map(|s| RepoRef::parse(s).unwrap())
            .collect();
        let pins: Vec<RepoRef> = already.iter().map(|s| RepoRef::parse(s).unwrap()).collect();
        app.open_picker(cands, &pins);
        app
    }

    #[test]
    fn open_picker_switches_mode_and_seeds_pins() {
        let app = picker_app(&["octo/alpha", "octo/beta"], &["octo/beta"]);
        assert_eq!(app.mode, Mode::Picker);
        let p = app.picker.as_ref().unwrap();
        assert_eq!(p.cursor, 0);
        assert!(p.filter.is_empty());
        assert!(p.pinned.contains("octo/beta"));
        assert_eq!(p.pinned.len(), 1);
    }

    #[test]
    fn close_picker_returns_to_dashboard() {
        let mut app = picker_app(&["octo/alpha"], &[]);
        app.close_picker();
        assert_eq!(app.mode, Mode::Dashboard);
        assert!(app.picker.is_none());
    }

    #[test]
    fn new_app_starts_in_dashboard_without_picker() {
        let app = App::new(None, Sort::Momentum);
        assert_eq!(app.mode, Mode::Dashboard);
        assert!(app.picker.is_none());
    }

    #[test]
    fn filter_narrows_candidates_case_insensitively() {
        let mut app = picker_app(&["octo/Alpha", "octo/beta", "other/gamma"], &[]);
        for c in "ALP".chars() {
            app.picker_filter_push(c);
        }
        let p = app.picker.as_ref().unwrap();
        assert_eq!(p.filtered(), vec![0]);
        assert_eq!(p.cursor, 0);
        for _ in 0..3 {
            app.picker_filter_pop();
        }
        assert_eq!(app.picker.as_ref().unwrap().filtered(), vec![0, 1, 2]);
    }

    #[test]
    fn filter_edit_resets_cursor() {
        let mut app = picker_app(&["octo/a", "octo/b", "octo/c"], &[]);
        app.picker_move(2);
        assert_eq!(app.picker.as_ref().unwrap().cursor, 2);
        app.picker_filter_push('b');
        assert_eq!(app.picker.as_ref().unwrap().cursor, 0);
    }

    #[test]
    fn filter_pop_on_empty_filter_is_noop() {
        let mut app = picker_app(&["octo/a"], &[]);
        app.picker_filter_pop();
        assert!(app.picker.as_ref().unwrap().filter.is_empty());
    }

    #[test]
    fn toggle_adds_then_removes_highlighted_candidate() {
        let mut app = picker_app(&["octo/alpha", "octo/beta"], &[]);
        app.picker_move(1);
        app.picker_toggle();
        assert!(app.picker.as_ref().unwrap().pinned.contains("octo/beta"));
        app.picker_toggle();
        assert!(!app.picker.as_ref().unwrap().pinned.contains("octo/beta"));
    }

    #[test]
    fn toggle_respects_filtered_cursor() {
        let mut app = picker_app(&["octo/alpha", "octo/beta"], &[]);
        app.picker_filter_push('b');
        app.picker_toggle();
        let p = app.picker.as_ref().unwrap();
        assert!(p.pinned.contains("octo/beta"));
        assert!(!p.pinned.contains("octo/alpha"));
    }

    #[test]
    fn cursor_clamps_at_both_ends() {
        let mut app = picker_app(&["octo/a", "octo/b", "octo/c"], &[]);
        app.picker_move(10);
        assert_eq!(app.picker.as_ref().unwrap().cursor, 2);
        app.picker_move(-10);
        assert_eq!(app.picker.as_ref().unwrap().cursor, 0);
    }

    #[test]
    fn cursor_and_toggle_are_safe_on_empty_filtered_list() {
        let mut app = picker_app(&["octo/a"], &[]);
        app.picker_filter_push('z');
        app.picker_move(1);
        app.picker_move(-1);
        app.picker_toggle();
        let p = app.picker.as_ref().unwrap();
        assert_eq!(p.cursor, 0);
        assert!(p.pinned.is_empty());
    }

    #[test]
    fn picker_pins_reflects_toggles_sorted() {
        let mut app = picker_app(&["octo/zeta", "octo/alpha"], &["octo/zeta"]);
        app.picker_move(1);
        app.picker_toggle();
        let pins = app.picker_pins();
        assert_eq!(
            pins,
            vec![
                RepoRef::parse("octo/alpha").unwrap(),
                RepoRef::parse("octo/zeta").unwrap()
            ]
        );
        app.picker_move(-1);
        app.picker_toggle();
        assert_eq!(
            app.picker_pins(),
            vec![RepoRef::parse("octo/alpha").unwrap()]
        );
    }
}
