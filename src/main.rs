mod app;
mod cache;
mod config;
mod format;
mod gh;
mod metrics;
mod model;
mod ui;

use std::future::Future;
use std::io::{self, Stdout};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use tokio::sync::mpsc;

use app::App;
use cache::Cache;
use model::{Metrics, RepoRef};

/// Terminal dashboard for your pinned GitHub projects. Configure pins in
/// `~/.config/projstat/config.toml`.
#[derive(Parser)]
#[command(version, about)]
struct Args {}

type Term = Terminal<CrosstermBackend<Stdout>>;

/// Maximum number of concurrent `gh` fetches during a refresh.
const FETCH_CONCURRENCY: usize = 6;
/// How long the loop waits for input before redrawing and draining refreshes.
const TICK: Duration = Duration::from_millis(100);

/// Result of one background refresh, sent back to the event loop.
struct Refresh {
    metrics: Vec<Metrics>,
    skipped: Vec<String>,
    /// False when every pinned repo failed; the cache is then left untouched.
    any_ok: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Args::parse();

    // Checked before touching the terminal so the error lands on a normal stderr.
    if let Err(e) = gh::check_available() {
        eprintln!("projstat: {e:#}");
        std::process::exit(1);
    }
    let config = config::load();
    let mut app = App::new(cache::load(), config.sort);

    install_panic_restore();
    let mut terminal = enter_terminal()?;
    let result = run(&mut terminal, &mut app, &config.pinned).await;
    // Normal exit and error exit both land here; panics are covered by the hook.
    restore_terminal();
    result
}

/// Leaves raw mode and the alternate screen. Safe to call repeatedly and
/// ignores errors, since it runs while the program is already failing.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
}

/// Restores the terminal before the default panic message is printed, so the
/// message is readable and the shell is not left in raw mode. Only a panic on
/// the main thread (the event loop) restores: a panic in a fetch task is
/// reported as a skipped repo and must not tear down the live TUI.
fn install_panic_restore() {
    let main_thread = std::thread::current().id();
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == main_thread {
            restore_terminal();
            default_hook(info);
        }
    }));
}

fn enter_terminal() -> anyhow::Result<Term> {
    enable_raw_mode().context("enabling raw mode")?;
    if let Err(e) = execute!(io::stdout(), EnterAlternateScreen) {
        restore_terminal();
        return Err(anyhow::Error::from(e).context("entering alternate screen"));
    }
    match Terminal::new(CrosstermBackend::new(io::stdout())) {
        Ok(terminal) => Ok(terminal),
        Err(e) => {
            restore_terminal();
            Err(anyhow::Error::from(e).context("creating terminal"))
        }
    }
}

async fn run(terminal: &mut Term, app: &mut App, pinned: &[RepoRef]) -> anyhow::Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Refresh>();
    // Cached metrics render first; this refresh then updates them in place.
    start_refresh(app, pinned, &tx);

    loop {
        terminal.draw(|f| ui::render(f, app))?;

        while let Ok(done) = rx.try_recv() {
            finish_refresh(app, done);
        }

        if event::poll(TICK)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && handle_key(app, key, pinned, &tx)
        {
            return Ok(());
        }
    }
}

/// Applies one key press. Returns `true` when the app should quit.
fn handle_key(
    app: &mut App,
    key: KeyEvent,
    pinned: &[RepoRef],
    tx: &mpsc::UnboundedSender<Refresh>,
) -> bool {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Up | KeyCode::Char('k') => app.select_prev(),
        KeyCode::Down | KeyCode::Char('j') => app.select_next(),
        KeyCode::Char('s') => app.cycle_sort(),
        KeyCode::Char('r') => start_refresh(app, pinned, tx),
        KeyCode::Enter => open_selected(app),
        _ => {}
    }
    false
}

/// Spawns a bounded refresh of every pinned repo and always sends exactly one
/// `Refresh` back, even if fetches panic. Does nothing when a refresh is
/// already in flight or there are no pins.
fn start_refresh(app: &mut App, pinned: &[RepoRef], tx: &mpsc::UnboundedSender<Refresh>) {
    if app.refreshing || pinned.is_empty() {
        return;
    }
    app.refreshing = true;
    app.errors.clear();

    let pinned = pinned.to_vec();
    let previous = app.metrics.clone();
    let tx = tx.clone();
    tokio::spawn(async move {
        let refresh = collect_refresh(pinned, previous, |repo| async move {
            gh::fetch_metrics(&repo).await
        })
        .await;
        // The receiver is gone only when the app is quitting; nothing to do then.
        let _ = tx.send(refresh);
    });
}

/// Fetches every repo with bounded concurrency. Each fetch runs in its own
/// task, so a panic becomes a `JoinError` for that repo only. A failed repo
/// keeps its previous metrics so it does not vanish from the dashboard.
async fn collect_refresh<F, Fut>(pinned: Vec<RepoRef>, previous: Vec<Metrics>, fetch: F) -> Refresh
where
    F: Fn(RepoRef) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = anyhow::Result<Metrics>> + Send + 'static,
{
    let fetched: Vec<(RepoRef, anyhow::Result<Metrics>)> = futures::stream::iter(pinned)
        .map(|repo| {
            let fetch = fetch.clone();
            async move {
                let worker = tokio::spawn(fetch(repo.clone()));
                let result = match worker.await {
                    Ok(result) => result,
                    Err(e) => Err(anyhow::anyhow!("fetch task failed: {e}")),
                };
                (repo, result)
            }
        })
        .buffer_unordered(FETCH_CONCURRENCY)
        .collect()
        .await;

    let any_ok = fetched.iter().any(|(_, result)| result.is_ok());
    let mut metrics = Vec::new();
    let mut skipped = Vec::new();
    for (repo, result) in fetched {
        match result {
            Ok(m) => metrics.push(m),
            Err(e) => {
                skipped.push(format!("{repo}: {e:#}"));
                if let Some(old) = previous.iter().find(|m| m.repo == repo) {
                    metrics.push(old.clone());
                }
            }
        }
    }
    Refresh {
        metrics,
        skipped,
        any_ok,
    }
}

/// Applies a finished refresh. A refresh where every fetch failed only clears
/// the in-flight flag and records the errors: the cached data keeps its
/// original `fetched_at` and is not rewritten.
fn finish_refresh(app: &mut App, done: Refresh) {
    if !done.any_ok {
        app.refreshing = false;
        app.errors.extend(done.skipped);
        return;
    }
    app.apply_refresh(done.metrics, done.skipped);
    if let Some(fetched_at) = app.fetched_at {
        let snapshot = Cache {
            fetched_at,
            metrics: app.metrics.clone(),
        };
        if let Err(e) = cache::save(&snapshot) {
            app.errors.push(format!("cache not saved: {e:#}"));
        }
    }
}

/// Opens the selected repo on GitHub in the default browser. Failure to
/// launch is ignored; the dashboard keeps running.
fn open_selected(app: &App) {
    let order = app.sorted_indices();
    let Some(&index) = order.get(app.selected) else {
        return;
    };
    let url = format!("https://github.com/{}", app.metrics[index].repo);
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = Command::new(opener)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn repo(name: &str) -> RepoRef {
        RepoRef::parse(&format!("octo/{name}")).unwrap()
    }

    fn metric(repo: RepoRef, closed: u32) -> Metrics {
        Metrics {
            repo,
            next_milestone: None,
            open_issues: 0,
            closed_7d: closed,
            opened_7d: 0,
            last_activity: Utc.with_ymd_and_hms(2026, 10, 30, 0, 0, 0).unwrap(),
            releases_shipped: 0,
        }
    }

    /// Good repo succeeds, `bad` returns an error, `boom` panics.
    async fn fake_fetch(r: RepoRef) -> anyhow::Result<Metrics> {
        match r.name.as_str() {
            "good" => Ok(metric(r, 3)),
            "bad" => anyhow::bail!("404 not found"),
            _ => panic!("simulated fetch panic"),
        }
    }

    #[tokio::test]
    async fn panicking_fetch_still_delivers_a_refresh() {
        let pinned = vec![repo("good"), repo("bad"), repo("boom")];
        let previous = vec![metric(repo("boom"), 9)];
        let refresh = collect_refresh(pinned, previous, fake_fetch).await;

        assert!(refresh.any_ok);
        assert_eq!(refresh.metrics.len(), 2);
        assert!(refresh.metrics.iter().any(|m| m.repo == repo("good")));
        // The panicking repo falls back to its previous snapshot.
        assert!(refresh.metrics.iter().any(|m| m.closed_7d == 9));
        assert_eq!(refresh.skipped.len(), 2);
        assert!(
            refresh
                .skipped
                .iter()
                .any(|s| s.contains("fetch task failed"))
        );
        assert!(refresh.skipped.iter().any(|s| s.contains("404 not found")));
    }

    #[tokio::test]
    async fn refreshing_clears_after_panicking_refresh() {
        let mut app = App::new(None, crate::config::Sort::Momentum);
        app.refreshing = true;
        let refresh = collect_refresh(vec![repo("boom")], vec![], fake_fetch).await;
        finish_refresh(&mut app, refresh);

        assert!(!app.refreshing);
        assert!(app.fetched_at.is_none());
        assert!(app.errors.iter().any(|e| e.contains("fetch task failed")));
    }

    #[tokio::test]
    async fn all_failed_refresh_keeps_cached_timestamp() {
        let cached = Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap();
        let mut app = App::new(
            Some(crate::cache::Cache {
                fetched_at: cached,
                metrics: vec![metric(repo("bad"), 1)],
            }),
            crate::config::Sort::Momentum,
        );
        app.refreshing = true;
        let refresh = collect_refresh(vec![repo("bad")], app.metrics.clone(), fake_fetch).await;
        assert!(!refresh.any_ok);
        finish_refresh(&mut app, refresh);

        assert!(!app.refreshing);
        assert_eq!(app.fetched_at, Some(cached));
        assert_eq!(app.metrics.len(), 1);
    }
}
