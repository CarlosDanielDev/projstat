# Planned directory tree — projstat (v0.1)

Mirrors the `maestro` house layout. Created at build start (not yet on disk).

```
projstat/
├── Cargo.toml                 # edition 2024; ratatui 0.30, crossterm 0.28,
│                              #   tokio 1 (full), serde/serde_json, anyhow,
│                              #   thiserror, chrono, clap (derive)
├── Cargo.lock
├── CHANGELOG.md
├── README.md                  # what it is, install, keybinds, config
├── .gitignore                 # /target, cache artifacts
├── docs/
│   ├── decisions.md
│   ├── directory-tree.md
│   ├── fm-usage.md
│   └── superpowers/specs/2026-10-31-projstat-design.md
└── src/
    ├── main.rs                # clap args, load config, init gh check, run app
    ├── config.rs              # ~/.config/projstat/config.toml: load + defaults
    ├── cache.rs               # ~/.cache/projstat/cache.json: read/write, staleness
    ├── model.rs               # Repo, Milestone, Metrics, Pick — domain types
    ├── gh.rs                  # shell `gh`, parse JSON -> model (async, tokio)
    ├── metrics.rs             # momentum, closeness, pick score  [pure · fm]
    ├── format.rs              # duration humanizer, progress-bar string [pure · fm]
    ├── app.rs                 # App state + event loop (crossterm event-stream),
    │                          #   update/refresh orchestration
    └── ui.rs                  # ratatui render: hero, project table, footer
```

**`[pure · fm]`** = std-only, no external crates — eligible for `fm` per
`docs/fm-usage.md`, each gated behind a `cargo check` + unit test.

Deferred (not in v0.1 tree): `src/views/milestones.rs`, `src/views/releases.rs`,
streak computation, sparkline widget.

## Module dependency sketch

```
main → config, gh, app
app  → model, gh, cache, ui, metrics
ui   → model, format
gh   → model
metrics, format → (std only)
```

Keeps the pure (`metrics`, `format`) layer free of ratatui/tokio/gh so `fm` can
touch it safely and tests run fast.
