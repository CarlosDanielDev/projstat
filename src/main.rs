mod app;
mod cache;
mod config;
mod format;
mod gh;
mod metrics;
mod model;
mod ui;

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
/// message is readable and the shell is not left in raw mode.
fn install_panic_restore() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
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

/// Spawns a bounded concurrent fetch of every pinned repo. Does nothing when
/// a refresh is already in flight or there are no pins.
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
        let fetched: Vec<(RepoRef, anyhow::Result<Metrics>)> = futures::stream::iter(pinned)
            .map(|repo| async move {
                let result = gh::fetch_metrics(&repo).await;
                (repo, result)
            })
            .buffer_unordered(FETCH_CONCURRENCY)
            .collect()
            .await;

        let mut metrics = Vec::new();
        let mut skipped = Vec::new();
        for (repo, result) in fetched {
            match result {
                Ok(m) => metrics.push(m),
                Err(e) => {
                    skipped.push(format!("{repo}: {e:#}"));
                    // Keep the last good snapshot so a failing repo does not vanish.
                    if let Some(old) = previous.iter().find(|m| m.repo == repo) {
                        metrics.push(old.clone());
                    }
                }
            }
        }
        // The receiver is gone only when the app is quitting; nothing to do then.
        let _ = tx.send(Refresh { metrics, skipped });
    });
}

/// Applies a finished refresh and persists the new snapshot.
fn finish_refresh(app: &mut App, done: Refresh) {
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
