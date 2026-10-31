# Decision log — projstat

Lightweight ADR log. Newest at top. Each: context → decision → consequences.

---

## D7 · MVP scope = dashboard only
**Date:** 2026-10-31 · **Status:** accepted
**Context:** Mockup shows `m` milestones and `r` releases views; streak and
sparklines tempting. Risk of scope creep before anything ships.
**Decision:** v0.1 ships the dashboard list + today's-pick hero only. Deferred:
`m`/`r` drill-ins, streak (if costly to compute), in-TUI config editing,
momentum sparklines.
**Consequences:** Fast first useful screen. Footer advertises `m`/`r` as
stretch; wire post-MVP.

## D6 · fm limited to pure std-only functions
**Date:** 2026-10-31 · **Status:** accepted
**Context:** Plan was to offload code to `fm` to save main-agent tokens. `fm`
in-session only exposes `system` (AFM-3 Core, on-device, ~3B); `pcc` needs
Terminal.app. Model is weak at Rust and unaware of ratatui 0.30 API.
**Decision:** `fm` writes only pure, `std`-only functions (momentum, pick-score
normalization, duration/progress-bar formatting), one at a time, verified by
`cargo check` + a main-agent-written unit test. Rules in `docs/fm-usage.md`.
Everything else = main agent + subagents.
**Consequences:** Token-saving smaller than the Go path would've given; Rust
choice (D1) accepted that tradeoff.

## D5 · Cache + async refresh, flicker-free rendering
**Date:** 2026-10-31 · **Status:** accepted
**Context:** Many `gh` calls are slow; Carlos hit flicker in a prior Go TUI.
**Decision:** On-disk JSON cache shown instantly on launch; background async
refresh + `r` key. Rendering through ratatui alt-screen, diff-only repaint,
**no** manual clears or raw prints.
**Consequences:** Instant open, no rate-limit stalls, no flicker.

## D4 · Today's pick ranks over pinned repos only
**Date:** 2026-10-31 · **Status:** accepted
**Context:** Pick formula `momentum × closeness-to-milestone` was undefined for
negative momentum, no-milestone, and zero-issue milestones — all visible in the
mockup.
**Decision:** Rank pinned repos only. No milestone ⇒ closeness 0; zero-issue
milestone ⇒ guard divide, closeness 0; negative momentum clamped at 0 for the
product. Tie-break by most recent activity.
**Consequences:** Hero always points at one of Carlos's chosen projects; no dead
repo surfaces; matches streak scoping.

## D3 · Pinned-only scope; discovery = picker over account + hawk-oliver
**Date:** 2026-10-31 · **Status:** accepted (Carlos chose option A)
**Context:** 211 personal repos + orgs `Kyte-A-I`, `hawk-oliver`. Full metrics
for all = instant rate-limit death. Anti-goal: rate-limit stalls.
**Decision:** Only pinned repos get metrics / appear on dashboard. Discovery
(`gh repo list` over the authed account + `hawk-oliver` org; **not** Kyte-A-I)
only populates a pin picker. A pin may be any `owner/repo`.
**Consequences:** Bounded fetches. No folded "…N more" list. Empty dashboard
until first pin.

## D2 · Data via the `gh` CLI, not a raw HTTP client
**Date:** 2026-10-31 · **Status:** accepted
**Context:** Need GitHub issues/milestones/releases; auth already present
(`CarlosDanielDev`, `gh` 2.101.0, ssh + repo/read:org scopes).
**Decision:** Shell out to `gh` (`gh repo list`, `gh api`, `gh release list`),
parse JSON with `serde_json`. No token management or raw HTTP in v0.1.
**Consequences:** Reuses auth, pagination, rate-limit handling. Hard dependency
on `gh` being installed + authed (check at startup).

## D1 · Rust + ratatui (over Go, Python)
**Date:** 2026-10-31 · **Status:** accepted (Carlos chose Rust)
**Context:** TUI dashboard, network-bound. Candidates: Rust+ratatui,
Go+BubbleTea, Python+Textual. Runtime perf is irrelevant here (bottleneck is
`gh`/network). Go wins build-speed and `fm` support; Rust wins runtime
lightness and single-binary polish and matches Carlos's Rust practice.
**Decision:** Rust + ratatui + crossterm. Match house style from existing
crates (`maestro` reference): edition 2024, ratatui 0.30, crossterm 0.28,
tokio 1, serde, anyhow/thiserror, chrono, clap.
**Consequences:** Lightest runtime, single static binary. Accepts slower build
and that `fm` contributes little (see D6).
