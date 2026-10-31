use std::io;
use std::process::{Command, Output};

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

#[cfg(test)]
#[test]
fn auth_parsed() {
    assert!(parse_auth_ok("Logged in to github.com"));
    assert!(!parse_auth_ok("not logged"));
}
