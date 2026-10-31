use std::path::{Path, PathBuf};
use std::{env, fs};

use anyhow::Context;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::model::Metrics;

/// The last fetched dashboard snapshot, persisted so the TUI can render
/// immediately on launch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cache {
    pub fetched_at: DateTime<Utc>,
    pub metrics: Vec<Metrics>,
}

/// `$XDG_CACHE_HOME/projstat/cache.json`, falling back to
/// `$HOME/.cache/projstat/cache.json`.
pub fn cache_path() -> PathBuf {
    let base = env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"));
    base.join("projstat").join("cache.json")
}

/// Writes the cache to the default path, creating parent directories.
pub fn save(c: &Cache) -> anyhow::Result<()> {
    save_to(&cache_path(), c)
}

/// Reads the cache from the default path. Returns `None` if the file is
/// missing or not a valid cache.
pub fn load() -> Option<Cache> {
    load_from(&cache_path())
}

/// True when the snapshot is older than `max_age_secs`.
pub fn is_stale(c: &Cache, max_age_secs: i64) -> bool {
    is_stale_at(c, max_age_secs, Utc::now())
}

fn save_to(path: &Path, c: &Cache) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(c).context("serializing cache")?;
    fs::write(path, json).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn load_from(path: &Path) -> Option<Cache> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn is_stale_at(c: &Cache, max_age_secs: i64, now: DateTime<Utc>) -> bool {
    now - c.fetched_at > Duration::seconds(max_age_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Milestone, RepoRef};
    use chrono::TimeZone;

    fn sample() -> Cache {
        let fetched_at = Utc.with_ymd_and_hms(2026, 10, 30, 12, 0, 0).unwrap();
        Cache {
            fetched_at,
            metrics: vec![
                Metrics {
                    repo: RepoRef::parse("acme/widgets").unwrap(),
                    next_milestone: Some(Milestone {
                        title: "v1.0".to_string(),
                        open_issues: 3,
                        closed_issues: 7,
                        due_on: Some(Utc.with_ymd_and_hms(2026, 11, 15, 0, 0, 0).unwrap()),
                    }),
                    open_issues: 4,
                    closed_7d: 2,
                    opened_7d: 1,
                    last_activity: fetched_at,
                    releases_shipped: 1,
                },
                Metrics {
                    repo: RepoRef::parse("acme/gadgets").unwrap(),
                    next_milestone: None,
                    open_issues: 0,
                    closed_7d: 0,
                    opened_7d: 0,
                    last_activity: fetched_at - Duration::days(40),
                    releases_shipped: 0,
                },
            ],
        }
    }

    fn scratch_dir() -> PathBuf {
        env::temp_dir().join(format!("projstat-cache-test-{}", std::process::id()))
    }

    #[test]
    fn round_trip_through_temp_path_equals_original() {
        let path = scratch_dir().join("nested").join("cache.json");
        let original = sample();
        save_to(&path, &original).expect("save succeeds and creates parent dirs");
        let loaded = load_from(&path).expect("load returns the saved cache");
        assert_eq!(loaded, original);
        let _ = std::fs::remove_dir_all(scratch_dir());
    }

    #[test]
    fn load_missing_or_corrupt_file_returns_none() {
        let missing = scratch_dir().join("does-not-exist.json");
        assert!(load_from(&missing).is_none());

        let corrupt = scratch_dir().join("corrupt").join("cache.json");
        std::fs::create_dir_all(corrupt.parent().unwrap()).unwrap();
        std::fs::write(&corrupt, "{ not json").unwrap();
        assert!(load_from(&corrupt).is_none());
        let _ = std::fs::remove_dir_all(scratch_dir());
    }

    #[test]
    fn is_stale_true_when_older_than_max_age() {
        let now = Utc.with_ymd_and_hms(2026, 10, 31, 12, 0, 0).unwrap();
        let c = Cache {
            fetched_at: now - Duration::seconds(601),
            metrics: vec![],
        };
        assert!(is_stale_at(&c, 600, now));
    }

    #[test]
    fn is_stale_false_when_fresh() {
        let now = Utc.with_ymd_and_hms(2026, 10, 31, 12, 0, 0).unwrap();
        let c = Cache {
            fetched_at: now - Duration::seconds(30),
            metrics: vec![],
        };
        assert!(!is_stale_at(&c, 600, now));
    }

    #[test]
    fn cache_path_ends_with_projstat_cache_json() {
        let p = cache_path();
        assert!(p.ends_with("projstat/cache.json"));
    }
}
