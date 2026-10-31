use chrono::{Local, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table};

use crate::app::App;
use crate::format::{humanize_age, progress_bar};
use crate::metrics::{liveness, milestone_progress as milestone_pct};
use crate::model::{Liveness, Metrics};

/// Draws the whole dashboard into `f`. Diff-only rendering: no clears, no stdout.
pub fn render(f: &mut Frame, app: &App) {
    let [title_area, hero_area, table_area, footer_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(5),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(f.area());

    f.render_widget(title_bar(app), title_area);
    f.render_widget(hero(app), hero_area);
    f.render_widget(project_table(app), table_area);
    f.render_widget(
        Paragraph::new("↑↓ move   ⏎ open repo   s sort   r refresh   q quit"),
        footer_area,
    );
}

fn title_bar(app: &App) -> Paragraph<'static> {
    let mut spans = vec![
        Span::styled("projstat", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!("  ·  {}", Local::now().format("%d %b"))),
    ];
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
    let rows: Vec<Row<'static>> = app
        .sorted_indices()
        .into_iter()
        .enumerate()
        .map(|(pos, idx)| {
            let m = &app.metrics[idx];
            let row = project_row(m, (now - m.last_activity).num_seconds());
            if pos == app.selected {
                row.style(Style::default().add_modifier(Modifier::REVERSED))
            } else {
                row
            }
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
        d if d < 0 => format!("{d}↓"),
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
