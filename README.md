# projstat

A terminal dashboard for your GitHub projects that answers one question each morning: **which project do I push on today — and am I close to a win?**

Built for solo maintainers juggling many repos. It turns scattered issue/milestone/release state into visible momentum, so finishing feels close enough to keep going.

> **Status:** v0.1 in active development. The dashboard below is the target; not all of it is wired yet. Not released on crates.io.

```
┌─ projstat ──────────────────────────────────── 31 Oct  ·  🔥 7-day streak ─┐
│                                                                            │
│  TODAY'S PICK  ▸  my-project                                               │
│  Milestone "v0.4 – auth"  ███████████████░░░░░  78%   4 issues left        │
│  Last win: #412 closed 2h ago · one push from the v0.4 tag                 │
│                                                                            │
├─ ALL PROJECTS ───────────────────── sort: momentum ▼ ──────────────────────┤
│  REPO            NEXT MILESTONE        PROGRESS        OPEN  Δ7d   LAST     │
│  ▸ my-project    v0.4 – auth           ██████████░ 78%   4   +6↑   2h  🔥   │
│    side-thing    v0.1 – mvp            ███░░░░░░░░ 22%  11   +2↑   1d  ·    │
│    old-parser    v2 – rewrite          ███████░░░░ 61%   7   +0    5d  💤   │
│    dead-repo     —  (no milestone)     ░░░░░░░░░░  0%   19   −1↓  23d  🪦   │
│  4 repos · 41 open issues · 11 closed this week · 2 releases shipped        │
├────────────────────────────────────────────────────────────────────────────┤
│  ↑↓ move   ⏎ open repo   m milestones   r releases   s sort   q quit        │
└────────────────────────────────────────────────────────────────────────────┘
```

## Why

GitHub's UI is built for one repo at a time. If you run a dozen side projects, it can't tell you where your attention is best spent today. `projstat` ranks the projects you pin by **momentum** (net issues closed this week) and **closeness to the next milestone**, and names a single "today's pick" — the project where a little effort lands a visible win.

## Features

- **Today's pick** — one project surfaced as the highest-leverage place to work right now.
- **Per-repo momentum** — 7-day net issue delta (`Δ7d`), next-milestone progress bar, open-issue count, last activity, and a liveness marker (🔥 active · 💤 stalling · 🪦 abandoned).
- **Pinned-only** — you choose the handful of repos that matter; 200 dead repos stay out of sight.
- **Fast** — reads from a local cache on launch (instant), refreshes in the background.
- **Flicker-free** single static binary. No runtime, no daemon.

## How it works

`projstat` shells out to the [GitHub CLI](https://cli.github.com/) (`gh`) for all data, so it reuses your existing auth — no tokens to manage. It only ever reads; it never writes to your repos.

## Requirements

- Rust (edition 2024) to build.
- [`gh`](https://cli.github.com/) installed and authenticated (`gh auth login`).

## Install

From source (while unreleased):

```sh
git clone https://github.com/CarlosDanielDev/projstat.git
cd projstat
cargo install --path .
```

Then run:

```sh
projstat
```

## Configuration

`~/.config/projstat/config.toml`:

```toml
# The only repos tracked. Any owner/repo you can access.
pinned = ["CarlosDanielDev/projstat", "some-org/some-repo"]

# momentum | progress | name | last
sort = "momentum"

# orgs scanned when picking repos to pin
discover_orgs = ["some-org"]
```

No config file yet? `projstat` opens to an empty dashboard and tells you how to add your first pin.

## Keybinds

| Key | Action |
|-----|--------|
| `↑` / `↓` (`k` / `j`) | Move selection |
| `⏎` | Open the selected repo on GitHub |
| `s` | Cycle sort |
| `r` | Refresh now |
| `q` | Quit |

## Roadmap

- `m` / `r` — milestones and releases drill-in views
- Daily streak tracking
- In-TUI pin editing / repo picker
- Momentum sparklines over time

## License

To be decided before first release.

---

Built in Rust with [ratatui](https://ratatui.rs/).
