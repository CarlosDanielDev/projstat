use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A GitHub repository identified by `owner/name`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoRef {
    pub owner: String,
    pub name: String,
}

impl RepoRef {
    /// Parses `owner/name`. Returns `None` unless there is exactly one `/`
    /// separating two non-empty parts.
    pub fn parse(s: &str) -> Option<RepoRef> {
        let (owner, name) = s.split_once('/')?;
        if owner.is_empty() || name.is_empty() || name.contains('/') {
            return None;
        }
        Some(RepoRef {
            owner: owner.to_string(),
            name: name.to_string(),
        })
    }
}

impl fmt::Display for RepoRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// A GitHub milestone and its issue progress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Milestone {
    pub title: String,
    pub open_issues: u32,
    pub closed_issues: u32,
    pub due_on: Option<DateTime<Utc>>,
}

/// Computed activity metrics for one repository.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub repo: RepoRef,
    pub next_milestone: Option<Milestone>,
    pub open_issues: u32,
    pub closed_7d: u32,
    pub opened_7d: u32,
    pub last_activity: DateTime<Utc>,
    pub releases_shipped: u32,
}

/// A repository selected for display, with its ranking score.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub repo: RepoRef,
    pub score: f64,
}

/// How recently a repository has been active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    Active,
    Stalling,
    Abandoned,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_owner_repo() {
        let r = RepoRef::parse("a/b").expect("valid owner/name");
        assert_eq!(r.owner, "a");
        assert_eq!(r.name, "b");
        assert_eq!(RepoRef::parse("bad"), None);
    }

    #[test]
    fn repo_display_roundtrips() {
        let r = RepoRef::parse("octo/cat").unwrap();
        assert_eq!(r.to_string(), "octo/cat");
        assert_eq!(RepoRef::parse(&r.to_string()), Some(r));
    }
}
