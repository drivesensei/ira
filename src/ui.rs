use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
    Frame,
};

use crate::app::App;
use crate::services::file_info::{
    human, size_line_cancelled, size_line_partial, size_line_started, spinner_char,
};

/// Renders the user interface widgets.
pub fn render(app: &mut App, frame: &mut Frame) {
    let Rect { width, height, .. } = frame.area();

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
            frame.area(),
        );
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Drives
            Constraint::Length(3), // Common folders
            Constraint::Length(3), // Bookmarks + Actions
            Constraint::Min(2),    // Files
        ])
        .split(frame.area());

    crate::components::drives_ui::render(frame, app, rows[0]);
    crate::components::common_folders_ui::render(frame, app, rows[1]);

    // Bookmarks (left) and Actions (right) share the third row.
    let bookmarks_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(48)])
        .split(rows[2]);
    crate::components::bookmarks_ui::render(frame, app, bookmarks_row[0]);
    crate::components::actions_ui::render(frame, app, bookmarks_row[1]);

    // Files area: optional Copy Board sidebar on the right, optional
    // command panel at the bottom, then the panes.
    let mut files_area = rows[3];
    if app.panel_open() {
        let vert = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(4), Constraint::Length(10)])
            .split(files_area);
        files_area = vert[0];
        crate::components::process_panel_ui::render(frame, app, vert[1]);
    }
    if app.copy_board {
        let board = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(48)])
            .split(files_area);
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

    // Deletion progress dialog: shown while the background delete worker
    // runs (unless dismissed with a key). Dismissing never cancels the job.
    if let Some(del) = &app.deletion {
        if !app.deletion_box_hidden {
            let current = del
                .current
                .as_deref()
                .and_then(|p| p.rsplit('/').next())
                .unwrap_or("");
            let line = format!(" Deleting {}/{} — {} ", del.done, del.total, current);
            let style = Style::default().fg(Color::Black).bg(Color::LightYellow);
            let area = centered_rect(60, 3, frame.area());
            paint_bg(frame, area, style);
            let block = Block::bordered().title(" Deleting ").style(style);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::raw(line),
                    Line::raw("  [any key] hide (deletion continues)  "),
                ])
                .block(block)
                .style(style),
                area,
            );
        }
    }

    // Confirmation prompt overlay (delete): solid light background so it is
    // readable over the file list.
    if let Some(confirm) = &app.confirming {
        let verb = match confirm.action {
            crate::app::ConfirmAction::Delete => "Delete",
            crate::app::ConfirmAction::Copy => "Copy",
            crate::app::ConfirmAction::Move => "Move",
        };
        let prompt = match &confirm.dest_dir {
            Some(dest) => {
                let dest_name = std::path::Path::new(dest)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| dest.clone());
                format!(" {verb} {} → {dest_name}?  [y]es  [n]o ", confirm.label)
            }
            None => format!(" {verb} {}?  [y]es  [n]o ", confirm.label),
        };
        let style = Style::default().fg(Color::Black).bg(Color::White);
        let area = centered_rect(60, 3, frame.area());
        paint_bg(frame, area, style);
        let block = Block::bordered().title(" Confirm ").style(style);
        frame.render_widget(
            Paragraph::new(prompt)
                .block(block)
                .alignment(Alignment::Center)
                .style(style),
            area,
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
        let w = (prompt.text.len() as u16 + 12)
            .max(34)
            .min(frame.area().width.saturating_sub(4));
        let h = lines.len() as u16 + 2;
        let style = Style::default().fg(Color::Black).bg(Color::White);
        let area = centered_rect(w, h, frame.area());
        paint_bg(frame, area, style);
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().title(" Rename ").style(style))
                .style(style),
            area,
        );
    }

    // Create-new dialog: live kind preview (folder vs file by extension).
    if let Some(p) = &app.new_entry {
        let name: String = p.text.iter().collect();
        let kind = match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => {
                format!("file (.{ext})")
            }
            _ => "folder".to_string(),
        };
        let mut spans: Vec<Span<'static>> = Vec::new();
        let chars = p.text.clone();
        if chars.is_empty() {
            spans.push(Span::raw(" ").style(Style::default().add_modifier(Modifier::REVERSED)));
        }
        for (i, c) in chars.iter().enumerate() {
            let s = Span::raw(format!("{c}"));
            spans.push(if i == p.cursor {
                s.style(Style::default().add_modifier(Modifier::REVERSED))
            } else {
                s
            });
        }
        if p.cursor >= chars.len() {
            spans.push(Span::raw(" ").style(Style::default().add_modifier(Modifier::REVERSED)));
        }
        let text_line = Line::from(spans);
        let kind_line = Line::raw(format!("  new {kind}  "));
        let hint = Line::raw("  [Enter] create  [Esc] cancel  ");
        let lines: Vec<Line<'static>> = vec![text_line, kind_line, hint];
        let w = (name.len() as u16 + 16)
            .max(34)
            .min(frame.area().width.saturating_sub(4));
        let h = lines.len() as u16 + 2;
        let style = Style::default().fg(Color::Black).bg(Color::White);
        let area = centered_rect(w, h, frame.area());
        paint_bg(frame, area, style);
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().title(" New ").style(style))
                .style(style),
            area,
        );
    }

    // Multi-selection info dialog: aggregate sizes summed live from the
    // size cache while the per-folder walks run.
    if let Some(m) = &app.multi_info {
        let (complete, data, items, on_disk) = app.multi_info_aggregate();
        let selection = match (m.folders, m.files) {
            (f, 0) => format!("{f} folders selected"),
            (0, fl) => format!("{fl} files selected"),
            (f, fl) => format!("{f} folders / {fl} files selected"),
        };
        let size_line = if complete {
            format!("Size: {data} data / {on_disk} on disk ({items} items)")
        } else {
            format!(
                "Size: {} {} data / {} on disk — calculating…",
                spinner_char(m.started),
                human(data),
                human(on_disk)
            )
        };
        let lines: Vec<Line<'static>> = vec![
            Line::styled(selection, Style::default().add_modifier(Modifier::BOLD)),
            Line::raw(size_line),
            Line::raw(""),
            Line::raw("  [any key] close  "),
        ];
        let max_w = lines
            .iter()
            .map(|l| l.width() as u16)
            .max()
            .unwrap_or(30)
            .max(34);
        let w = (max_w + 4).min(frame.area().width.saturating_sub(4));
        let h = lines.len() as u16 + 2;
        let style = Style::default().fg(Color::Black).bg(Color::White);
        let area = centered_rect(w, h, frame.area());
        paint_bg(frame, area, style);
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(ratatui::widgets::Wrap { trim: false })
                .block(Block::bordered().title(" Info ").style(style))
                .style(style),
            area,
        );
    }

    // Info dialog: read-only metadata for the selected entry. The Size line
    // is dynamic: animated partial size while the background walk runs, an
    // honest lower bound after `x`, and the final line once done (drain
    // inserts it into `lines`; the renderer only fills the gap).
    if let Some(info) = &app.info {
        let mut lines: Vec<Line<'static>> =
            info.lines.iter().map(|l| Line::raw(l.clone())).collect();
        if !lines.iter().any(|l| {
            l.spans
                .first()
                .is_some_and(|s| s.content.starts_with("Size:"))
        }) {
            let line = match (app.size_walk_started(&info.path), app.size_info(&info.path)) {
                (Some(started), Some(si)) => {
                    Line::raw(size_line_partial(si, spinner_char(started)))
                }
                (Some(started), None) => Line::raw(size_line_started(spinner_char(started))),
                // Walk cancelled with `x`: show the partial lower bound.
                (None, Some(si)) => Line::raw(size_line_cancelled(si)),
                // Brief window before the walk's first progress tick (or a
                // file's pending metadata stat).
                (None, None) => Line::raw(size_line_started(spinner_char(info.started))),
            };
            lines.insert(4.min(lines.len()), line);
        }
        // Folder queries get a hint line (walk running or a cached
        // measurement exists); plain files need none of these keys.
        let is_folder =
            app.size_walk_started(&info.path).is_some() || app.size_info(&info.path).is_some();
        if is_folder {
            lines.push(Line::raw(""));
            lines.push(Line::raw(
                "  [x] cancel size walk   [r] recalculate   [Esc] close  ",
            ));
        }
        let mut max_w: u16 = 10;
        for l in &lines {
            max_w = max_w.max(l.width() as u16);
        }
        let w = (max_w + 4).min(frame.area().width.saturating_sub(4));
        let h = lines.len() as u16 + 2;
        let style = Style::default().fg(Color::Black).bg(Color::White);
        let area = centered_rect(w, h, frame.area());
        paint_bg(frame, area, style);
        frame.render_widget(
            Paragraph::new(lines)
                .block(Block::bordered().title(" Info ").style(style))
                .style(style),
            area,
        );
    }

    // Error dialog: dismissable modal for action failures (eject busy,
    // rename collision, delete IO error...). Red theme; any key closes it
    // (handler.rs treats every key as dismiss while it is open).
    if let Some(status) = &app.status {
        let text: Vec<Line<'static>> = vec![
            Line::styled(
                status.text.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Line::raw(""),
            Line::raw("  [any key] dismiss  "),
        ];
        // Cap the width at 80% of the screen and word-wrap long messages.
        // `Paragraph` wraps on word boundaries when the text exceeds the
        // widget width; the height must count every wrapped line.
        let max_allowed = (frame.area().width * 4 / 5).max(30);
        let content_w = text
            .iter()
            .map(|l| l.width() as u16)
            .max()
            .unwrap_or(20)
            .max(30)
            .min(max_allowed.saturating_sub(4));
        let w = (content_w + 4).min(frame.area().width.saturating_sub(4));
        let inner_w = w.saturating_sub(2) as usize; // minus borders
        let wrapped: usize = text
            .iter()
            .map(|l| {
                let lw = l.width();
                if lw == 0 {
                    1
                } else {
                    lw.div_ceil(inner_w.max(1))
                }
            })
            .sum();
        let h = (wrapped as u16 + 2).min(frame.area().height.saturating_sub(2));
        let style = Style::default().fg(Color::White).bg(Color::Red);
        let area = centered_rect(w, h, frame.area());
        paint_bg(frame, area, style);
        frame.render_widget(
            Paragraph::new(text)
                .wrap(ratatui::widgets::Wrap { trim: false })
                .block(Block::bordered().title(" Error ").style(style))
                .style(style),
            area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::list_files::FEntry;
    use ratatui::{backend::TestBackend, Terminal};

    /// Renders on a 100x30 TestBackend and returns the whole screen text.
    fn rendered(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| render(app, f)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Opens the info dialog for a single entry via the real `show_info`
    /// path (the only public way to start a walk / create a dialog).
    fn dialog_app(name: &str, is_dir: bool, dir: &std::path::Path) -> App {
        let mut app = App::default();
        app.panes[0].files = vec![FEntry {
            path: dir.join(name).to_string_lossy().into_owned(),
            label: name.to_string(),
            is_dir,
        }];
        app.panes[0].state.select(Some(0));
        app.show_info();
        app
    }

    #[test]
    fn long_error_messages_wrap_instead_of_cutting() {
        // 120 chars on a 100-wide backend: wider than the dialog, so the
        // Paragraph must word-wrap; every character must survive in the
        // buffer (nothing truncated).
        let long = format!("Failed to eject /dev/sdd2: {}", "x".repeat(100));
        let mut app = App::default();
        app.set_status(long, true);

        let text = rendered(&mut app);
        let tail = "x".repeat(10);
        assert!(
            text.contains(&tail),
            "the message tail must survive wrapping; buffer tail: {:?}",
            text.chars().rev().take(300).collect::<String>()
        );
        assert!(text.contains("Failed to eject"), "{text}");
    }
    #[test]
    fn eject_error_renders_as_dismissable_dialog_not_inline() {
        let mut app = App::default();
        app.set_status("Failed to eject /dev/sdd2: target is busy", true);

        let text = rendered(&mut app);
        // The dialog frame is present and the full human-readable line fits
        // on one line (nothing truncated into the files pane).
        assert!(text.contains(" Error "), "{text}");
        assert!(
            text.contains("Failed to eject /dev/sdd2: target is busy"),
            "{text}"
        );
        assert!(text.contains("[any key] dismiss"), "{text}");

        // A non-error notice renders with the same dialog shape.
        let mut app = App::default();
        app.set_status("Copied 2 items", false);
        let text = rendered(&mut app);
        assert!(text.contains("Copied 2 items"), "{text}");

        // No status -> nothing rendered.
        let mut app = App::default();
        let text = rendered(&mut app);
        assert!(!text.contains(" Error "), "{text}");
    }
    #[test]
    fn folder_dialog_shows_hint_and_file_dialog_does_not() {
        let dir = std::env::temp_dir().join(format!("ira-ui-hint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // A folder query starts a walk (or shows a cached size): hint shows.
        let mut app = dialog_app("folder", true, &dir);
        let text = rendered(&mut app);
        assert!(text.contains("[r] recalculate"), "{text}");
        assert!(text.contains("[x] cancel size walk"));
        assert!(text.contains("[Esc] close"));

        // A plain file gets no walk and no cache entry: no hint keys.
        let mut app = dialog_app("file.txt", false, &dir);
        let text = rendered(&mut app);
        assert!(!text.contains("recalculate"), "{text}");
        assert!(!text.contains("cancel size walk"), "{text}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hint_line_fits_inside_the_dialog() {
        let dir = std::env::temp_dir().join(format!("ira-ui-width-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = dialog_app("f", true, &dir);

        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| render(&mut app, f)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        // If the dialog were too narrow the Paragraph would truncate/wrap the
        // hint and the full string could not appear contiguously.
        assert!(
            text.contains("[x] cancel size walk   [r] recalculate   [Esc] close"),
            "{text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Paints a solid background (blank cells, `style`) across `area`, so modal
/// dialogs are opaque instead of letting the underlying file list show
/// through in the unfilled padding around their text.
fn paint_bg(frame: &mut Frame, area: Rect, style: Style) {
    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, style);
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
