# projstat — design

Date: 2026-10-31
Status: awaiting approval
Owner: Carlos (@carlosdanieldev)

## Intent / why

Carlos runs many GitHub projects, several AI-led, and loses the thread of
what's alive, what's stalling, and what deserves attention today. He lives in
the terminal. Existing GitHub UI doesn't answer the one question that keeps a
solo maintainer going: *"which project do I push on today, and am I close to a
win?"*

`projstat` is a terminal dashboard that answers that in the first three seconds
and turns scattered repo state into visible momentum — so finishing feels close
enough to keep going.

The point is **motivation**, not reporting. Numbers serve the push, not the
other way around.

## Goals

- One screen, opened each morning, that names **today's pick** and shows why.
- Per repo: progress toward the next milestone, open issue count, 7-day
  momentum, last activity, and a liveness state (active / stalling /
  abandoned).
- Track only the repos the user **pins** (any `owner/repo` across the account
  or the hawk-oliver org); a picker helps choose them from discovery.
- Instant open (show cached data immediately), refresh in the background.
- Single static binary, drops into `PATH`, no runtime. Flicker-free.

## Non-goals (v0.1)

- Not an issue editor. Read-only view of GitHub state.
- Not a git client. Doesn't touch local working trees.
- No write operations to GitHub (no closing issues, no releasing).
- No multi-account / GitHub Enterprise host switching in v0.1 (single `gh`
  auth context). Structure for it, don't build it.

## Anti-goals (would make this a failure even if it "works")

- A wall of numbers with no opinion — if it doesn't tell him *where to go*, it
  failed.
- Flicker or lag on refresh. A janky TUI kills the morning ritual.
- Config ceremony before first useful screen. It must be useful with zero
  config.
- Hitting GitHub on every keystroke / rate-limit stalls.

## The screen (approved mockup)

```
┌─ projstat ──────────────────────────────────── 31 Oct  ·  🔥 7-day streak ─┐
│                                                                            │
│  TODAY'S PICK  ▸  kyte-brain                                               │
│  Milestone "v0.4 – auth"  ███████████████░░░░░  78%   4 issues left        │
│  Last win: #412 closed 2h ago · one push from the v0.4 tag                 │
│                                                                            │
├─ ALL PROJECTS ───────────────────── sort: momentum ▼ ──────────────────────┤
│  REPO            NEXT MILESTONE        PROGRESS        OPEN  Δ7d   LAST     │
│  ▸ kyte-brain    v0.4 – auth           ██████████░ 78%   4   +6↑   2h  🔥   │
│    projstat      v0.1 – mvp            ███░░░░░░░░ 22%  11   +2↑   1d  ·    │
│    lexml-ai      v2 – parser rewrite   ███████░░░░ 61%   7   +0    5d  💤   │
│    hydra-growth  —  (no milestone)     ░░░░░░░░░░  0%   19   −1↓  23d  🪦   │
│  5 repos · 43 open issues · 11 closed this week · 2 releases shipped        │
├────────────────────────────────────────────────────────────────────────────┤
│  ↑↓ move   ⏎ open repo   m milestones   r releases   s sort   q quit        │
└────────────────────────────────────────────────────────────────────────────┘
```

## Decided (the "how", as decided with Carlos)

- **Language / TUI:** Rust + `ratatui` + `crossterm`. Chosen for lightest,
  fastest runtime and single static binary. (Build speed cost accepted.)
- **Match the house style** (from Carlos's existing crates in `../`, closest
  reference is `maestro`): edition **2024**, `ratatui = "0.30"`,
  `crossterm = "0.28"` (features `event-stream`), `tokio = "1"` (full),
  `serde`/`serde_json`, `anyhow` + `thiserror`, `chrono`, `clap` (derive) for
  args. Mirror `maestro`'s async-ratatui + tokio event loop structure rather
  than inventing a new one.
- **Data source:** shell out to the **`gh` CLI** (`gh repo list`, `gh api`,
  `gh release list`), parse JSON with `serde_json`. Reuses existing auth,
  pagination, rate-limit handling. No raw HTTP client, no token management in
  v0.1. — my call, accepted.
- **Repo selection:** **pinned-only** (reviewer option A, Carlos confirmed).
  Only pinned repos get metrics and appear on the dashboard. Discovery
  (`gh repo list` across the authed account **and the `hawk-oliver` org** —
  **not** Kyte-A-I) exists only to populate a picker for *choosing* pins. No
  folded "…N more" list on the dashboard. A pin may be any `owner/repo`
  (personal or hawk-oliver). This keeps fetches bounded and off the rate limit
  (211 personal repos make discover-all-metrics non-viable).
- **Cache:** on-disk JSON cache. Launch renders cached state instantly, then
  refreshes in the background; `r` forces refresh. — my call, accepted.
- **Rendering discipline (flicker fix):** alt-screen buffer, diff-only repaint
  through ratatui's frame, **never** manual screen clears or raw prints. Low
  refresh frequency. This directly addresses the flicker Carlos hit in a prior
  Go TUI.

### Config

`~/.config/projstat/config.toml`:
```toml
pinned = ["CarlosDanielDev/projstat", "hawk-oliver/<repo>"]  # the only repos tracked
sort   = "momentum"                                          # momentum | progress | name | last
discover_orgs = ["hawk-oliver"]                              # orgs scanned for the pin picker
```
Missing file / no pins ⇒ empty dashboard with a prompt to add pins via the
picker (discovery runs over the authed account + `discover_orgs`). Repo names
in the mockup above are placeholders; real repos are whatever Carlos pins.

### Cache

`~/.cache/projstat/cache.json` — last fetched per-repo snapshot + timestamp.
Shown immediately on launch; staleness indicated; refreshed async.

### Metrics (definitions — builder implements, these are the contract)

- **Next milestone & progress:** the repo's open milestone with the nearest due
  date (or, if none dated, the one with highest completion). Progress =
  `closed_issues / (open_issues + closed_issues)` for that milestone, from
  `gh api repos/{o}/{r}/milestones`.
- **Open issues:** open issue count for the repo (excluding PRs).
- **Δ7d (momentum):** `(issues closed in last 7d) − (issues opened in last 7d)`.
  Positive = net progress. Shown with ↑/↓.
- **Last activity:** most recent of pushedAt / last issue or PR update.
- **Liveness icon:** 🔥 active (activity ≤ ~2d) · 💤 stalling (≤ ~14d) ·
  🪦 abandoned (> ~14d). Thresholds configurable later; hardcode sane defaults
  now.
- **Today's pick:** ranks over **pinned repos only**. Score =
  momentum × closeness-to-next-milestone; highest wins, tie-break by most
  recent activity. Edge cases (fixed): no milestone ⇒ closeness = 0; milestone
  with zero issues ⇒ guard the divide, closeness = 0; negative momentum clamped
  at 0 for the product so a dead repo can't win the hero. (The scoring +
  normalization is a pure std-only function — an `fm` target per
  `docs/fm-usage.md`.)
- **Streak:** consecutive days with ≥1 issue closed or commit across pinned
  repos. (Best-effort from available `gh` data; if too costly, defer to v0.2 —
  builder's call, note it.)

### MVP scope (v0.1 — ship this first)

Dashboard list view only: discover + pin, next-milestone progress bar, open
count, Δ7d, last activity, liveness icon, today's-pick hero, summary footer
line, cache + async refresh, `↑↓ / s sort / r refresh / ⏎ open-in-browser /
q quit`.

### Deferred (v0.2+, explicitly out of MVP)

- `m` milestones drill-in view and `r` releases view (footer shows them; wire
  as stretch).
- Streak if it proves expensive to compute.
- Config editing from inside the TUI.
- Charts / sparklines of momentum over time.
- Multi-host / multi-account.

## Build approach (post-approval, for the plan)

- Repo is not yet a git repo — `git init` + Cargo scaffold happens at build
  start (gated behind approval).
- Issues created via `setwave:define` once the plan is written.
- `council` convened when a design/impl decision is genuinely contested.
- Subagents for parallel, well-scoped build chunks; `fm` attempted on isolated
  non-architectural pieces only (weak at Rust — verify hard, don't trust
  blind).

## Left to the builder

- Exact `gh` invocations and JSON shapes per metric (contract above fixes the
  *meaning*, not the command).
- Module layout, error/retry handling for `gh` failures and offline launch.
- ratatui widget composition and color theme (match mockup; light/dark safe).
- Whether streak ships in v0.1 or defers (per cost).
```
