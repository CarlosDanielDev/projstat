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
use config::Config;
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

/// Everything a background task sends back to the event loop.
enum Msg {
    Refresh(Refresh),
    Discovered(anyhow::Result<Vec<RepoRef>>),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Args::parse();

    // Checked before touching the terminal so the error lands on a normal stderr.
    if let Err(e) = gh::check_available() {
        eprintln!("projstat: {e:#}");
        std::process::exit(1);
    }
    let mut config = config::load();
    let mut app = App::new(cache::load(), config.sort);

    install_panic_restore();
    let mut terminal = enter_terminal()?;
    let result = run(&mut terminal, &mut app, &mut config).await;
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

/// Loop-local state that is not part of the rendered `App`.
#[derive(Default)]
struct Flags {
    /// Repo discovery is in flight; a second `a` press is ignored.
    discovering: bool,
    /// Pins were saved while a refresh was running; refresh once it finishes.
    refresh_pending: bool,
}

async fn run(terminal: &mut Term, app: &mut App, config: &mut Config) -> anyhow::Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    let mut flags = Flags::default();
    // Cached metrics render first; this refresh then updates them in place.
    start_refresh(app, &config.pinned, &tx);

    loop {
        terminal.draw(|f| ui::render(f, app))?;

        while let Ok(msg) = rx.try_recv() {
            match msg {
                Msg::Refresh(done) => finish_refresh(app, done),
                Msg::Discovered(result) => {
                    flags.discovering = false;
                    match result {
                        Ok(candidates) => app.open_picker(candidates, &config.pinned),
                        Err(e) => app.errors.push(format!("discovery failed: {e:#}")),
                    }
                }
            }
        }
        if flags.refresh_pending && !app.refreshing {
            flags.refresh_pending = false;
            start_refresh(app, &config.pinned, &tx);
        }

        if event::poll(TICK)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && handle_key(app, key, config, &mut flags, &tx)
        {
            return Ok(());
        }
    }
}

/// Applies one key press. Returns `true` when the app should quit.
fn handle_key(
    app: &mut App,
    key: KeyEvent,
    config: &mut Config,
    flags: &mut Flags,
    tx: &mpsc::UnboundedSender<Msg>,
) -> bool {
    if app.mode == app::Mode::Picker {
        return handle_picker_key(app, key, config, flags, tx);
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Up | KeyCode::Char('k') => app.select_prev(),
        KeyCode::Down | KeyCode::Char('j') => app.select_next(),
        KeyCode::Char('s') => app.cycle_sort(),
        KeyCode::Char('r') => start_refresh(app, &config.pinned, tx),
        KeyCode::Char('a') if !flags.discovering => {
            flags.discovering = true;
            start_discovery(&config.discover_orgs, tx);
        }
        KeyCode::Enter => open_selected(app),
        _ => {}
    }
    false
}

/// Applies one key press while the pin picker is open. Only Esc leaves the
/// picker; `q` is an ordinary filter character here. Returns `true` only for
/// Ctrl-C, which quits from anywhere.
fn handle_picker_key(
    app: &mut App,
    key: KeyEvent,
    config: &mut Config,
    flags: &mut Flags,
    tx: &mpsc::UnboundedSender<Msg>,
) -> bool {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
        KeyCode::Esc => app.close_picker(),
        KeyCode::Enter => save_picker(app, config, flags, tx),
        KeyCode::Up => app.picker_move(-1),
        KeyCode::Down => app.picker_move(1),
        KeyCode::Backspace => app.picker_filter_pop(),
        KeyCode::Char(' ') => app.picker_toggle(),
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            app.picker_filter_push(c);
        }
        _ => {}
    }
    false
}

/// Persists the picker's pins, makes them the active set, closes the picker
/// and refreshes over the new pins. A save failure is reported in the errors
/// list but the new pins still apply for this session.
fn save_picker(
    app: &mut App,
    config: &mut Config,
    flags: &mut Flags,
    tx: &mpsc::UnboundedSender<Msg>,
) {
    let pins = app.picker_pins();
    let saved = Config {
        pinned: pins.clone(),
        sort: app.sort,
        discover_orgs: config.discover_orgs.clone(),
    };
    let save_result = config::save(&saved);
    config.pinned = pins;
    app.close_picker();

    if app.refreshing {
        // The in-flight refresh fetched the old pins; the loop starts a new one.
        flags.refresh_pending = true;
    } else {
        start_refresh(app, &config.pinned, tx);
    }
    // Pushed after the refresh starts, which clears the error list.
    // While a refresh is in flight it is cleared again when the queued one starts.
    if let Err(e) = save_result {
        app.errors.push(format!("config not saved: {e:#}"));
    }
}

/// Runs repo discovery in the background and sends the outcome back. A panic
/// inside discovery becomes an `Err`, so the event loop always gets one reply.
fn start_discovery(discover_orgs: &[String], tx: &mpsc::UnboundedSender<Msg>) {
    let orgs = discover_orgs.to_vec();
    let tx = tx.clone();
    tokio::spawn(async move {
        let worker = tokio::spawn(async move { gh::discover_repos(&orgs).await });
        let result = match worker.await {
            Ok(result) => result,
            Err(e) => Err(anyhow::anyhow!("discovery task failed: {e}")),
        };
        let _ = tx.send(Msg::Discovered(result));
    });
}

/// Spawns a bounded refresh of every pinned repo and always sends exactly one
/// `Refresh` back, even if fetches panic. Does nothing when a refresh is
/// already in flight or there are no pins.
fn start_refresh(app: &mut App, pinned: &[RepoRef], tx: &mpsc::UnboundedSender<Msg>) {
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
        let _ = tx.send(Msg::Refresh(refresh));
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

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn picker_keeps_q_as_filter_text_and_only_esc_leaves() {
        let (tx, _rx) = mpsc::unbounded_channel::<Msg>();
        let mut app = App::new(None, crate::config::Sort::Momentum);
        let mut config = Config::default();
        let mut flags = Flags::default();
        app.open_picker(vec![repo("alpha")], &[]);

        assert!(!handle_key(
            &mut app,
            key(KeyCode::Char('q')),
            &mut config,
            &mut flags,
            &tx
        ));
        assert_eq!(app.picker.as_ref().unwrap().filter, "q");
        assert_eq!(app.mode, app::Mode::Picker);

        assert!(!handle_key(
            &mut app,
            key(KeyCode::Esc),
            &mut config,
            &mut flags,
            &tx
        ));
        assert_eq!(app.mode, app::Mode::Dashboard);
        assert!(app.picker.is_none());
    }

    #[test]
    fn dashboard_q_quits() {
        let (tx, _rx) = mpsc::unbounded_channel::<Msg>();
        let mut app = App::new(None, crate::config::Sort::Momentum);
        let mut config = Config::default();
        let mut flags = Flags::default();
        assert!(handle_key(
            &mut app,
            key(KeyCode::Char('q')),
            &mut config,
            &mut flags,
            &tx
        ));
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
