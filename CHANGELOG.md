# Changelog

All notable changes to projstat are documented here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning: [SemVer](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Planned for **v0.1.0** (MVP — dashboard only):

### Added
- Terminal dashboard (Rust + ratatui) listing pinned GitHub repos.
- Today's-pick hero: ranks pinned repos by momentum × milestone-closeness.
- Per-repo: next-milestone progress bar, open-issue count, 7-day momentum
  (Δ7d), last-activity, liveness icon (🔥 active / 💤 stalling / 🪦 abandoned).
- Summary footer: repo / open-issue / closed-this-week / releases totals.
- Repo data via the `gh` CLI; pinned-only, with a discovery picker over the
  authed account + `hawk-oliver` org.
- On-disk cache for instant launch; async background refresh + `r` key.
- Config at `~/.config/projstat/config.toml` (pins, sort, discover_orgs).
- Keybinds: `↑↓` move, `⏎` open repo, `s` sort, `r` refresh, `q` quit.

### Deferred (post-v0.1)
- `m` milestones and `r` releases drill-in views.
- Daily streak tracking.
- In-TUI config / pin editing.
- Momentum sparklines.

[Unreleased]: https://github.com/CarlosDanielDev/projstat/commits/main
