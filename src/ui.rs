use chrono::{Local, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, TableState};

use crate::app::App;
use crate::format::{humanize_age, progress_bar};
use crate::metrics::{liveness, milestone_progress as milestone_pct};
use crate::model::{Liveness, Metrics};

/// Draws the whole dashboard into `f`. Diff-only rendering: no clears, no stdout.
pub fn render(f: &mut Frame, app: &App) {
    let [title_area, hero_area, table_area, summary_area, footer_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(5),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(f.area());

    f.render_widget(title_bar(app), title_area);
    f.render_widget(hero(app), hero_area);
    // Stateful so ratatui scrolls the selected row into view.
    let mut table_state =
        TableState::new().with_selected((!app.metrics.is_empty()).then_some(app.selected));
    f.render_stateful_widget(project_table(app), table_area, &mut table_state);
    f.render_widget(Paragraph::new(summary(app)), summary_area);
    f.render_widget(
        Paragraph::new("↑↓ move   ⏎ open repo   s sort   r refresh   q quit"),
        footer_area,
    );
}

/// One-line totals across all tracked repos.
fn summary(app: &App) -> String {
    let open: u32 = app.metrics.iter().map(|m| m.open_issues).sum();
    let closed: u32 = app.metrics.iter().map(|m| m.closed_7d).sum();
    let releases: u32 = app.metrics.iter().map(|m| m.releases_shipped).sum();
    format!(
        "{} repos · {} open issues · {} closed this week · {} releases shipped",
        app.metrics.len(),
        open,
        closed,
        releases
    )
}

fn title_bar(app: &App) -> Paragraph<'static> {
    let mut spans = vec![
        Span::styled("projstat", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!("  ·  {}", Local::now().format("%d %b"))),
    ];
    let age = match app.fetched_at {
        Some(t) => format!(
            "updated {} ago",
            humanize_age((Utc::now() - t).num_seconds())
        ),
        None => "never updated".to_string(),
    };
    spans.push(Span::raw(format!("  ·  {age}")));
    if !app.errors.is_empty() {
        spans.push(Span::styled(
            format!("  ·  ⚠ {}", app.errors.len()),
            Style::default().add_modifier(Modifier::BOLD),
        ));
    }
    if app.refreshing {
        spans.push(Span::raw("  ·  refreshing…"));
    }
    Paragraph::new(Line::from(spans))
}

fn hero(app: &App) -> Paragraph<'static> {
    let block = Block::bordered().title("today");
    let Some(pick) = &app.pick else {
        let msg = "no pinned repos — add one in ~/.config/projstat/config.toml";
        return Paragraph::new(msg).block(block);
    };

    let mut lines = vec![Line::from(vec![
        Span::styled(
            "TODAY'S PICK",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("  ▸  {pick}")),
    ])];

    let pick_metrics = app.metrics.iter().find(|m| &m.repo == pick);
    if let Some(ms) = pick_metrics.and_then(|m| m.next_milestone.as_ref()) {
        let p = milestone_pct(ms.open_issues, ms.closed_issues);
        lines.push(Line::from(format!(
            "Milestone \"{}\"  {}  {:>3.0}%",
            ms.title,
            progress_bar(p, 20),
            p * 100.0
        )));
    }
    Paragraph::new(lines).block(block)
}

fn project_table(app: &App) -> Table<'static> {
    let header = Row::new([
        "REPO",
        "NEXT MILESTONE",
        "PROGRESS",
        "OPEN",
        "Δ7d",
        "LAST",
        "",
    ])
    .style(Style::default().add_modifier(Modifier::BOLD));

    let now = Utc::now();
    // Rows follow sorted_indices(); the highlight is applied by the TableState
    // selection (an ordinal position), not by styling rows here.
    let rows: Vec<Row<'static>> = app
        .sorted_indices()
        .into_iter()
        .map(|idx| {
            let m = &app.metrics[idx];
            project_row(m, (now - m.last_activity).num_seconds())
        })
        .collect();

    let widths = [
        Constraint::Min(14),
        Constraint::Length(20),
        Constraint::Length(26),
        Constraint::Length(6),
        Constraint::Length(7),
        Constraint::Length(6),
        Constraint::Length(3),
    ];
    Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .block(Block::bordered().title("all projects"))
}

fn project_row(m: &Metrics, age_secs: i64) -> Row<'static> {
    let (milestone, progress) = match &m.next_milestone {
        Some(ms) => {
            let p = milestone_pct(ms.open_issues, ms.closed_issues);
            (
                ms.title.clone(),
                format!("{} {:>3.0}%", progress_bar(p, 10), p * 100.0),
            )
        }
        None => ("—".to_string(), "—".to_string()),
    };

    Row::new([
        Cell::from(m.repo.name.clone()),
        Cell::from(milestone),
        Cell::from(progress),
        Cell::from(m.open_issues.to_string()),
        Cell::from(momentum_label(m.closed_7d, m.opened_7d)),
        Cell::from(humanize_age(age_secs)),
        Cell::from(liveness_icon(liveness(age_secs))),
    ])
}

/// Net 7-day issue delta with a direction arrow.
fn momentum_label(closed_7d: u32, opened_7d: u32) -> String {
    let delta = closed_7d as i32 - opened_7d as i32;
    match delta {
        d if d > 0 => format!("+{d}↑"),
        d if d < 0 => format!("−{}↓", d.unsigned_abs()),
        _ => "0".to_string(),
    }
}

fn liveness_icon(l: Liveness) -> &'static str {
    match l {
        Liveness::Active => "🔥",
        Liveness::Stalling => "💤",
        Liveness::Abandoned => "🪦",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Sort;
    use crate::model::{Metrics, Milestone, RepoRef};
    use chrono::{Duration, Utc};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn fixture_app() -> App {
        let repo = RepoRef::parse("octo/my-project").unwrap();
        let metric = Metrics {
            repo,
            next_milestone: Some(Milestone {
                title: "v0.4 - auth".into(),
                open_issues: 4,
                closed_issues: 14,
                due_on: None,
            }),
            open_issues: 4,
            closed_7d: 6,
            opened_7d: 0,
            last_activity: Utc::now() - Duration::hours(2),
            releases_shipped: 0,
        };
        let mut app = App::new(None, Sort::Momentum);
        app.metrics = vec![metric];
        app.pick = app.compute_pick();
        app
    }

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buf = terminal.backend().buffer();
        let width = buf.area.width as usize;
        buf.content()
            .chunks(width)
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_hero_and_header() {
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        let app = fixture_app();
        terminal.draw(|f| render(f, &app)).unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("TODAY'S PICK"), "missing hero label:\n{text}");
        assert!(text.contains("my-project"), "missing repo name:\n{text}");
        assert!(text.contains("q quit"), "missing keybind footer:\n{text}");
    }

    #[test]
    fn title_shows_age_and_error_count() {
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        let mut app = fixture_app();
        app.fetched_at = Some(Utc::now() - Duration::hours(3));
        terminal.draw(|f| render(f, &app)).unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("updated 3h ago"), "missing age:\n{text}");
        assert!(!text.contains('⚠'), "unexpected error indicator:\n{text}");

        app.errors = vec!["a failed".into(), "b failed".into()];
        terminal.draw(|f| render(f, &app)).unwrap();
        let text = buffer_text(&terminal);
        assert!(text.contains("⚠ 2"), "missing error count:\n{text}");

        app.fetched_at = None;
        terminal.draw(|f| render(f, &app)).unwrap();
        assert!(buffer_text(&terminal).contains("never updated"));
    }

    fn named_metric(name: &str, open: u32, closed_7d: u32) -> Metrics {
        Metrics {
            repo: RepoRef::parse(&format!("octo/{name}")).unwrap(),
            next_milestone: None,
            open_issues: open,
            closed_7d,
            opened_7d: 0,
            last_activity: Utc::now() - Duration::hours(2),
            releases_shipped: 1,
        }
    }

    fn app_with(metrics: Vec<Metrics>, selected: usize) -> App {
        let mut app = App::new(None, Sort::Name);
        app.metrics = metrics;
        app.selected = selected;
        app
    }

    /// Index of the first buffer row whose text contains `needle`.
    fn row_of(terminal: &Terminal<TestBackend>, needle: &str) -> Option<usize> {
        buffer_text(terminal)
            .lines()
            .position(|line| line.contains(needle))
    }

    /// True when any cell of buffer row `y` is reversed (the highlight).
    /// Uses `any`, not `all`: the row's border glyphs are never highlighted.
    fn row_is_reversed(terminal: &Terminal<TestBackend>, y: usize) -> bool {
        let buf = terminal.backend().buffer();
        let width = buf.area.width as usize;
        buf.content()[y * width..(y + 1) * width]
            .iter()
            .any(|c| c.modifier.contains(Modifier::REVERSED))
    }

    #[test]
    fn renders_summary_totals() {
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        let app = app_with(vec![named_metric("a", 4, 6), named_metric("b", 3, 2)], 0);
        terminal.draw(|f| render(f, &app)).unwrap();
        let text = buffer_text(&terminal);
        assert!(
            text.contains("2 repos · 7 open issues · 8 closed this week · 2 releases shipped"),
            "missing summary line:\n{text}"
        );
    }

    #[test]
    fn highlights_second_row_when_selected_is_one() {
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        let app = app_with(
            vec![named_metric("first", 1, 0), named_metric("second", 1, 0)],
            1,
        );
        terminal.draw(|f| render(f, &app)).unwrap();
        let first = row_of(&terminal, "first").expect("first row rendered");
        let second = row_of(&terminal, "second").expect("second row rendered");
        assert!(
            row_is_reversed(&terminal, second),
            "second row not highlighted"
        );
        assert!(
            !row_is_reversed(&terminal, first),
            "first row wrongly highlighted"
        );
    }

    #[test]
    fn scrolls_selected_row_into_view_when_below_fold() {
        // Short terminal: only a few table rows fit; select the last of ten repos.
        let mut terminal = Terminal::new(TestBackend::new(100, 14)).unwrap();
        let metrics: Vec<Metrics> = (0..10)
            .map(|i| named_metric(&format!("repo{i:02}"), 1, 0))
            .collect();
        let app = app_with(metrics, 9);
        terminal.draw(|f| render(f, &app)).unwrap();
        let y = row_of(&terminal, "repo09").expect("selected row scrolled into view");
        assert!(
            row_is_reversed(&terminal, y),
            "scrolled-in row not highlighted"
        );
    }

    #[test]
    fn renders_negative_momentum_with_minus_sign() {
        assert_eq!(momentum_label(0, 3), "−3↓");
    }

    #[test]
    fn renders_empty_state_message_without_pins() {
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        let app = App::new(None, Sort::Momentum);
        terminal.draw(|f| render(f, &app)).unwrap();
        let text = buffer_text(&terminal);
        assert!(
            text.contains("no pinned repos"),
            "missing empty-state message:\n{text}"
        );
    }
}
