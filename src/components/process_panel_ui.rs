use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::app::App;
use crate::services::process_panel::RunState;

/// Renders the bottom command panel: the output ring buffer plus an input
/// line. Hidden entirely when closed; keeps rendering (without focus) when
/// a command runs in the background.
pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.panel_has_focus();
    let (state_label, _state_style) = match app.process_panel.as_ref().map(|p| p.state()) {
        Some(RunState::Running) => ("running", Style::default().fg(Color::Green)),
        Some(RunState::Exited(0)) => ("done", Style::default().fg(Color::Green)),
        Some(RunState::Exited(_)) => ("exited", Style::default().fg(Color::Red)),
        Some(RunState::Failed) => ("failed", Style::default().fg(Color::Red)),
        Some(RunState::Killed) => ("stopped", Style::default().fg(Color::DarkGray)),
        None => ("idle", Style::default().fg(Color::DarkGray)),
    };

    let mut lines: Vec<Line> = Vec::new();
    if let Some(panel) = &app.process_panel {
        let rows = area.height.saturating_sub(5) as usize; // borders+input+2
        for line in panel.tail(rows) {
            lines.push(Line::raw(line));
        }
    } else {
        lines.push(
            Line::raw(" No command yet — type one below and press Enter.")
                .style(Style::default().fg(Color::DarkGray)),
        );
    }

    lines.push(Line::raw(""));

    let title = format!(" Command  [{state_label}]  Ctrl+C stops  Esc unfocuses ");
    let border = if focused {
        Style::default()
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::bordered().title(title).border_style(border);

    f.render_widget(Paragraph::new(lines).block(block), area);

    // Input line: last row inside the block.
    let input_area = Rect {
        x: area.x + 1,
        y: area.y + area.height - 2,
        width: area.width.saturating_sub(2),
        height: 1,
    };
    f.render_widget(Paragraph::new(input_line_span(app)), input_area);
}

fn input_line_span(app: &App) -> Line<'static> {
    Line::from(vec![
        Span::styled("$ ", Style::default().fg(Color::Green)),
        Span::raw(app.panel_input.clone()),
        Span::raw(" ").style(Style::default().add_modifier(Modifier::REVERSED)),
    ])
}
