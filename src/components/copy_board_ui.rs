use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, List, ListState, Paragraph},
    Frame,
};

use crate::app::App;
use crate::services::transfer::{Job, JobKind, JobStatus};

/// Renders the Copy Board sidebar: one row per transfer job with progress.
pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.board_has_focus();

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let rows: Vec<Line<'static>> = app.jobs.iter().map(job_line).collect();
    let border_style = if focused {
        Style::default()
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let list = List::new(rows)
        .block(
            Block::bordered()
                .title(" Copy Board ")
                .border_style(border_style),
        )
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▸");
    if focused {
        f.render_stateful_widget(list, vertical[0], &mut app.copy_board_state);
    } else {
        // Match inactive file panes: no highlighted row when unfocused.
        let mut unfocused_state = ListState::default();
        f.render_stateful_widget(list, vertical[0], &mut unfocused_state);
    }

    let hint = if focused {
        " p/Space pause  x cancel  Esc close "
    } else {
        " Tab to focus board "
    };
    f.render_widget(
        Paragraph::new(hint).style(Style::default().fg(Color::DarkGray)),
        vertical[1],
    );
}

fn job_line(job: &Job) -> Line<'static> {
    let icon = match job.status {
        JobStatus::Running => ">",
        JobStatus::Paused => "||",
        JobStatus::Cancelled => "x",
        JobStatus::Done => "d",
        JobStatus::Failed(_) => "!",
        JobStatus::Queued => "-",
    };
    let kind = match job.kind {
        JobKind::Copy => "C",
        JobKind::Move => "M",
    };
    let label: String = job.label.chars().take(10).collect();

    match &job.status {
        JobStatus::Done => Line::raw(format!("[{icon}] {kind} {label:10} done")),
        JobStatus::Cancelled => Line::raw(format!("[{icon}] {kind} {label:10} cancelled")),
        JobStatus::Failed(_) => Line::raw(format!("[{icon}] {kind} {label:10} error")),
        _ => {
            let pct = job
                .total_bytes
                .filter(|t| *t > 0)
                .map(|t| (job.copied_bytes as f64 / t as f64) * 100.0)
                .unwrap_or(0.0);
            let bytes = format!(
                "{}/{}",
                human(job.copied_bytes),
                job.total_bytes
                    .map(human)
                    .unwrap_or_else(|| "?".to_string())
            );
            // Keep the row within the fixed 36-column panel: no unicode bar,
            // tight padding, right-aligned byte counter.
            Line::raw(format!("[{icon}] {kind} {label:10} {pct:3.0}% {bytes:>12}"))
        }
    }
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}
