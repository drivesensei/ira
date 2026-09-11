use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListState, Paragraph},
    Frame,
};

use crate::app::App;
use crate::services::transfer::{Job, JobKind, JobStatus};
use crate::theme::Theme;
use crate::ui::chrome::{hint_line, panel_block};

/// Renders the Copy Board sidebar: one row per transfer job with progress.
pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let focused = app.board_has_focus();

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let rows: Vec<Line<'static>> = app.jobs.iter().map(|j| job_line(j, &theme)).collect();
    let list = List::new(rows)
        .block(panel_block(Line::raw(" Copy Board "), focused, &theme))
        .highlight_style(
            Style::new()
                .fg(theme.cursor_fg)
                .bg(theme.cursor_bg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▌");
    if focused {
        f.render_stateful_widget(list, vertical[0], &mut app.copy_board_state);
    } else {
        // Match inactive file panes: no highlighted row when unfocused.
        let mut unfocused_state = ListState::default();
        f.render_stateful_widget(list, vertical[0], &mut unfocused_state);
    }

    let hint = if focused {
        hint_line(
            &[("p/Space", " pause"), ("x", " cancel"), ("Esc", " close")],
            &theme,
        )
    } else {
        hint_line(&[("Tab", " to focus board")], &theme)
    };
    f.render_widget(Paragraph::new(hint), vertical[1]);
}

fn job_line(job: &Job, theme: &Theme) -> Line<'static> {
    let (icon, color) = match job.status {
        JobStatus::Running => ("▶", theme.accent),
        JobStatus::Paused => ("⏸", theme.warning),
        JobStatus::Cancelled => ("✕", theme.text_muted),
        JobStatus::Done => ("✓", theme.success),
        JobStatus::Failed(_) => ("!", theme.error),
        JobStatus::Queued => ("·", theme.text_muted),
    };
    let kind = match job.kind {
        JobKind::Copy => "C",
        JobKind::Move => "M",
    };
    let label: String = job.label.chars().take(10).collect();
    let icon_span = Span::styled(format!("[{icon}]"), Style::default().fg(color));
    let kind_span = Span::styled(format!(" {kind} "), Style::default().fg(theme.text_muted));
    let label_span = Span::styled(format!("{label:10}"), Style::default().fg(theme.text));

    match &job.status {
        JobStatus::Done => Line::from(vec![
            icon_span,
            kind_span,
            label_span,
            Span::styled(" done", Style::default().fg(theme.success)),
        ]),
        JobStatus::Cancelled => Line::from(vec![
            icon_span,
            kind_span,
            label_span,
            Span::styled(" cancelled", Style::default().fg(theme.text_muted)),
        ]),
        JobStatus::Failed(_) => Line::from(vec![
            icon_span,
            kind_span,
            label_span,
            Span::styled(" error", Style::default().fg(theme.error)),
        ]),
        _ => {
            let pct = job
                .total_bytes
                .filter(|t| *t > 0)
                .map(|t| (job.copied_bytes as f64 / t as f64) * 100.0)
                .unwrap_or(0.0);
            let filled = ((pct / 100.0) * 8.0).round() as usize;
            let filled = filled.min(8);
            let mut bar_spans = Vec::new();
            if filled > 0 {
                bar_spans.push(Span::styled(
                    "━".repeat(filled),
                    Style::default().fg(theme.accent),
                ));
            }
            if filled < 8 {
                bar_spans.push(Span::styled(
                    "─".repeat(8 - filled),
                    Style::default().fg(theme.border),
                ));
            }
            let bytes = format!(
                "{}/{}",
                human(job.copied_bytes),
                job.total_bytes
                    .map(human)
                    .unwrap_or_else(|| "?".to_string())
            );
            let mut spans = vec![icon_span, kind_span, label_span, Span::raw(" ")];
            spans.extend(bar_spans);
            spans.push(Span::styled(
                format!(" {pct:3.0}% {bytes:>12}"),
                Style::default().fg(theme.text_muted),
            ));
            Line::from(spans)
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
