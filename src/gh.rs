use std::collections::HashSet;
use std::io;
use std::process::{Command, Output};

use anyhow::Context;
use chrono::{DateTime, TimeDelta, Utc};
use serde::Deserialize;

use crate::metrics::milestone_progress;
use crate::model::{Metrics, Milestone, RepoRef};

/// Returns true when `gh auth status` output reports a logged-in account.
pub fn parse_auth_ok(status_output: &str) -> bool {
    status_output.contains("Logged in to")
}

/// Verifies that the `gh` CLI is installed and authenticated.
pub fn check_available() -> anyhow::Result<()> {
    let version = run_gh(&["--version"]).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => anyhow::anyhow!(
            "GitHub CLI `gh` not found on PATH; install it from https://cli.github.com"
        ),
        _ => anyhow::anyhow!("failed to run `gh --version`: {e}"),
    })?;
    if !version.status.success() {
        anyhow::bail!("`gh --version` exited with {}", version.status);
    }

    let status = run_gh(&["auth", "status"])
        .map_err(|e| anyhow::anyhow!("failed to run `gh auth status`: {e}"))?;
    // `gh auth status` writes its report to stderr, so check both streams.
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );
    if !status.status.success() || !parse_auth_ok(&report) {
        anyhow::bail!("GitHub CLI is not authenticated; run `gh auth login`");
    }
    Ok(())
}

fn run_gh(args: &[&str]) -> io::Result<Output> {
    Command::new("gh").args(args).output()
}

/// Parses the `repos/{owner}/{name}/milestones` array.
pub fn parse_milestones(json: &str) -> anyhow::Result<Vec<Milestone>> {
    serde_json::from_str(json).context("parsing milestones JSON")
}

/// Picks the milestone with the nearest due date; if none is dated, the one
/// with the highest progress. Returns `None` for an empty slice.
pub fn pick_next_milestone(milestones: &[Milestone]) -> Option<Milestone> {
    let nearest = milestones
        .iter()
        .filter(|m| m.due_on.is_some())
        .min_by_key(|m| m.due_on);
    nearest
        .or_else(|| {
            milestones
                .iter()
                .max_by(|a, b| progress(a).total_cmp(&progress(b)))
        })
        .cloned()
}

fn progress(m: &Milestone) -> f64 {
    milestone_progress(m.open_issues, m.closed_issues)
}

/// Reads `total_count` from a `search/issues` response.
fn parse_total(json: &str) -> anyhow::Result<u32> {
    #[derive(Deserialize)]
    struct SearchTotal {
        total_count: u32,
    }
    let parsed: SearchTotal = serde_json::from_str(json).context("parsing search response")?;
    Ok(parsed.total_count)
}

/// Returns `(closed_7d, opened_7d)` from the two search responses.
pub fn parse_issue_counts(closed_json: &str, opened_json: &str) -> anyhow::Result<(u32, u32)> {
    Ok((parse_total(closed_json)?, parse_total(opened_json)?))
}

/// Parses `gh repo list --json nameWithOwner`. Entries that are not
/// `owner/name` are skipped.
pub fn parse_repo_list(json: &str) -> anyhow::Result<Vec<RepoRef>> {
    #[derive(Deserialize)]
    struct Entry {
        #[serde(rename = "nameWithOwner")]
        name_with_owner: String,
    }
    let entries: Vec<Entry> = serde_json::from_str(json).context("parsing repo list JSON")?;
    Ok(entries
        .iter()
        .filter_map(|e| RepoRef::parse(&e.name_with_owner))
        .collect())
}

/// Runs `gh` with `args` and returns stdout. Non-zero exit (including a 404
/// from `gh api`) becomes an error so callers can skip the repo.
async fn gh_stdout(args: &[String]) -> anyhow::Result<String> {
    let out = tokio::process::Command::new("gh")
        .args(args)
        .output()
        .await
        .with_context(|| format!("failed to run `gh {}`", args.join(" ")))?;
    if !out.status.success() {
        anyhow::bail!(
            "`gh {}` exited with {}: {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

async fn gh_api(path: &str, jq: Option<&str>) -> anyhow::Result<String> {
    let mut args = vec!["api".to_string(), path.to_string()];
    if let Some(expr) = jq {
        args.push("--jq".to_string());
        args.push(expr.to_string());
    }
    gh_stdout(&args).await
}

/// `YYYY-MM-DD` for seven days before `now`, used in search qualifiers.
fn since_date(now: DateTime<Utc>) -> String {
    (now - TimeDelta::days(7)).format("%Y-%m-%d").to_string()
}

fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// Most recent of `pushed_at` and the latest issue/PR update. Either may be
/// absent (`null`/empty); both absent is an error.
fn latest_activity(pushed: &str, issue_updated: &str) -> anyhow::Result<DateTime<Utc>> {
    parse_ts(pushed)
        .into_iter()
        .chain(parse_ts(issue_updated))
        .max()
        .context("repo has neither pushed_at nor issue/PR activity")
}

/// Fetches the activity metrics for one repository. Any failing `gh` call
/// fails the whole fetch.
pub async fn fetch_metrics(repo: &RepoRef) -> anyhow::Result<Metrics> {
    let slug = repo.to_string();
    let since = since_date(Utc::now());
    let milestones_path = format!("repos/{slug}/milestones?state=open");
    let open_path = format!("search/issues?q=repo:{slug}+type:issue+state:open");
    let closed_path = format!("search/issues?q=repo:{slug}+type:issue+closed:>={since}");
    let opened_path = format!("search/issues?q=repo:{slug}+type:issue+created:>={since}");
    let releases_path = format!("repos/{slug}/releases?per_page=100");
    let repo_path = format!("repos/{slug}");
    let issues_path =
        format!("repos/{slug}/issues?state=all&sort=updated&direction=desc&per_page=1");
    let (milestones_json, open_json, closed_json, opened_json, releases, pushed, issue_updated) = tokio::try_join!(
        gh_api(&milestones_path, None),
        gh_api(&open_path, None),
        gh_api(&closed_path, None),
        gh_api(&opened_path, None),
        gh_api(&releases_path, Some("length")),
        gh_api(&repo_path, Some(".pushed_at")),
        // Issues disabled (HTTP 410) or any failure: no issue activity, not a repo failure.
        async {
            Ok::<_, anyhow::Error>(
                gh_api(&issues_path, Some(".[0].updated_at // empty"))
                    .await
                    .unwrap_or_default(),
            )
        },
    )?;
    let last_activity = latest_activity(&pushed, &issue_updated)?;

    let milestones = parse_milestones(&milestones_json)?;
    let (closed_7d, opened_7d) = parse_issue_counts(&closed_json, &opened_json)?;
    Ok(Metrics {
        repo: repo.clone(),
        next_milestone: pick_next_milestone(&milestones),
        open_issues: parse_total(&open_json)?,
        closed_7d,
        opened_7d,
        last_activity,
        releases_shipped: releases
            .trim()
            .parse::<u32>()
            .context("parsing release count")?,
    })
}

fn repo_list_args(org: Option<&str>) -> Vec<String> {
    let mut args = vec!["repo".to_string(), "list".to_string()];
    if let Some(org) = org {
        args.push(org.to_string());
    }
    args.extend(["--json", "nameWithOwner", "--limit", "500"].map(String::from));
    args
}

/// Lists repositories for each org plus the authed user, de-duplicated in
/// first-seen order.
// Not wired into the MVP event loop yet (discovery picker is post-v0.1); kept and tested.
#[allow(dead_code)]
pub async fn discover_repos(orgs: &[String]) -> anyhow::Result<Vec<RepoRef>> {
    let mut found = Vec::new();
    for org in orgs {
        let json = gh_stdout(&repo_list_args(Some(org)))
            .await
            .with_context(|| format!("listing repos for org {org}"))?;
        found.extend(parse_repo_list(&json)?);
    }
    let json = gh_stdout(&repo_list_args(None))
        .await
        .context("listing repos for the authed user")?;
    found.extend(parse_repo_list(&json)?);

    let mut seen = HashSet::new();
    found.retain(|r| seen.insert(r.to_string()));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    const MILESTONES: &str = include_str!("../tests/fixtures/milestones.json");
    const SEARCH_CLOSED: &str = include_str!("../tests/fixtures/search_closed_7d.json");
    const SEARCH_OPENED: &str = include_str!("../tests/fixtures/search_opened_7d.json");

    fn ms(title: &str, open: u32, closed: u32, due: Option<i64>) -> Milestone {
        Milestone {
            title: title.to_string(),
            open_issues: open,
            closed_issues: closed,
            due_on: due.map(|s| Utc.timestamp_opt(s, 0).unwrap()),
        }
    }

    #[test]
    fn parse_milestones_reads_fixture() {
        let got = parse_milestones(MILESTONES).expect("fixture parses");
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].title, "v0.2");
        assert_eq!(got[0].open_issues, 4);
        assert_eq!(got[0].closed_issues, 6);
        assert_eq!(
            got[0].due_on,
            Some(Utc.with_ymd_and_hms(2026, 11, 15, 8, 0, 0).unwrap())
        );
        assert_eq!(got[2].title, "v1.0");
        assert_eq!(got[2].due_on, None);
    }

    #[test]
    fn parse_milestones_empty_input_is_err() {
        assert!(parse_milestones("").is_err());
    }

    #[test]
    fn parse_milestones_truncated_input_is_err() {
        assert!(parse_milestones("{").is_err());
    }

    #[test]
    fn pick_next_prefers_nearest_due_date() {
        let all = parse_milestones(MILESTONES).unwrap();
        let next = pick_next_milestone(&all).expect("some milestone");
        assert_eq!(next.title, "v0.2");
    }

    #[test]
    fn pick_next_without_due_dates_uses_highest_progress() {
        let all = vec![
            ms("a", 5, 5, None),
            ms("b", 2, 8, None),
            ms("c", 9, 1, None),
        ];
        let next = pick_next_milestone(&all).expect("some milestone");
        assert_eq!(next.title, "b");
    }

    #[test]
    fn pick_next_empty_is_none() {
        assert_eq!(pick_next_milestone(&[]), None);
    }

    #[test]
    fn parse_issue_counts_reads_total_count() {
        assert_eq!(
            parse_issue_counts(SEARCH_CLOSED, SEARCH_OPENED).unwrap(),
            (5, 9)
        );
    }

    #[test]
    fn parse_issue_counts_bad_input_is_err() {
        assert!(parse_issue_counts("", "{").is_err());
    }

    #[test]
    fn latest_activity_takes_max_and_tolerates_missing() {
        let push = "2026-10-01T00:00:00Z";
        let iss = "2026-10-20T00:00:00Z";
        assert_eq!(latest_activity(push, iss).unwrap(), parse_ts(iss).unwrap());
        assert_eq!(latest_activity(iss, push).unwrap(), parse_ts(iss).unwrap());
        assert_eq!(latest_activity(push, "").unwrap(), parse_ts(push).unwrap());
        assert_eq!(
            latest_activity("null", iss).unwrap(),
            parse_ts(iss).unwrap()
        );
        assert!(latest_activity("null", "").is_err());
    }

    #[test]
    fn parse_repo_list_keeps_valid_names() {
        let got = parse_repo_list(r#"[{"nameWithOwner":"a/b"},{"nameWithOwner":"bad"}]"#)
            .expect("parses");
        assert_eq!(got, vec![RepoRef::parse("a/b").unwrap()]);
    }
}

#[cfg(test)]
#[test]
fn auth_parsed() {
    assert!(parse_auth_ok("Logged in to github.com"));
    assert!(!parse_auth_ok("not logged"));
}
