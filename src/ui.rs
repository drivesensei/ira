use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph},
    Frame,
};

use crate::app::App;

/// Renders the user interface widgets.
pub fn render(app: &mut App, frame: &mut Frame) {
    let Rect { width, height, .. } = frame.size();

    let app_title_block = Block::bordered()
        .title("     IRA (Integrated Retro Archives)    ")
        .title_alignment(Alignment::Center)
        .title_style(Style::new().add_modifier(Modifier::BOLD))
        .border_type(BorderType::Rounded);

    if app.should_increase_size(width, height) {
        frame.render_widget(
            Paragraph::new("Please increase the terminal's size")
                .block(app_title_block)
                .style(Style::default().fg(Color::Cyan).bg(Color::Black))
                .centered(),
            frame.size(),
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Drives
            Constraint::Length(3), // Common folders
            Constraint::Length(3), // Bookmarks + Actions
            Constraint::Min(2),    // Files
        ])
        .split(frame.size());

    crate::components::drives_ui::render(frame, app, chunks[0]);
    crate::components::common_folders_ui::render(frame, app, chunks[1]);

    // Bookmarks (left) and Actions (right) share the third row.
    let bookmarks_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(36)])
        .split(chunks[2]);
    crate::components::bookmarks_ui::render(frame, app, bookmarks_row[0]);
    crate::components::actions_ui::render(frame, app, bookmarks_row[1]);

    // Files area: optional Copy Board sidebar on the right, then the panes.
    let mut files_area = chunks[3];
    if app.copy_board {
        let board = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(36)])
            .split(chunks[3]);
        crate::components::copy_board_ui::render(frame, app, board[1]);
        files_area = board[0];
    }

    if app.split {
        let files = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(files_area);
        crate::components::tab1_files_ui::render(frame, app, files[0], 0, app.active_pane == 0);
        crate::components::tab1_files_ui::render(frame, app, files[1], 1, app.active_pane == 1);
    } else {
        crate::components::tab1_files_ui::render(frame, app, files_area, 0, true);
    }

    // Confirmation prompt overlay (delete): solid light background so it is
    // readable over the file list.
    if let Some(confirm) = &app.confirming {
        let prompt = format!(" Delete {}?  [y]es  [n]o ", confirm.label);
        let style = Style::default().fg(Color::Black).bg(Color::White);
        let block = Block::bordered().title(" Confirm ").style(style);
        frame.render_widget(
            Paragraph::new(prompt).block(block).alignment(Alignment::Center).style(style),
            centered_rect(60, 3, frame.size()),
        );
    }

    // Rename dialog: in-place text editor with a visible cursor.
    if let Some(prompt) = &app.renaming {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let chars = prompt.text.clone();
        for (i, c) in chars.iter().enumerate() {
            let s = Span::raw(format!("{c}"));
            spans.push(if i == prompt.cursor {
                s.style(Style::default().add_modifier(Modifier::REVERSED))
            } else {
                s
            });
        }
        if prompt.cursor >= chars.len() {
            spans.push(Span::raw(" ").style(Style::default().add_modifier(Modifier::REVERSED)));
        }
        let text_line = Line::from(spans);
        let hint = Line::raw("  [Enter] rename  [Esc] cancel  ");
        let lines: Vec<Line<'static>> = vec![text_line, Line::raw(""), hint];
        let w = (prompt.text.len() as u16 + 12).max(34).min(frame.size().width.saturating_sub(4));
        let h = lines.len() as u16 + 2;
        let style = Style::default().fg(Color::Black).bg(Color::White);
        frame.render_widget(
            Paragraph::new(lines).block(Block::bordered().title(" Rename ").style(style)).style(style),
            centered_rect(w, h, frame.size()),
        );
    }

    // Info dialog: read-only metadata for the selected entry.
    if let Some(info) = &app.info {
        let lines: Vec<Line<'static>> = info.lines.iter().map(|l| Line::raw(l.clone())).collect();
        let mut max_w: u16 = 10;
        for l in &info.lines {
            max_w = max_w.max(l.chars().count() as u16);
        }
        let w = (max_w + 4).min(frame.size().width.saturating_sub(4));
        let h = lines.len() as u16 + 2;
        let style = Style::default().fg(Color::Black).bg(Color::White);
        frame.render_widget(
            Paragraph::new(lines).block(Block::bordered().title(" Info ").style(style)).style(style),
            centered_rect(w, h, frame.size()),
        );
    }
}

/// Returns a `width`x`height` rect centered within `area`.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}
