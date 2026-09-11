pub mod chrome;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::App;
use crate::services::file_info::{
    human, size_line_cancelled, size_line_partial, size_line_started, spinner_char,
};
use crate::theme::Theme;
use chrome::{dialog_area, hint_line, panel_block, render_dialog, text_input_line, DialogKind};

/// Renders the user interface widgets.
pub fn render(app: &mut App, frame: &mut Frame) {
    let Rect { width, height, .. } = frame.area();
    // Frame-scoped thumbnail-cache cap: the grid renderer raises it to
    // cover its working set (visible + prefetched cells).
    app.begin_thumb_cache_frame();

    let theme = app.theme;
    let area = frame.area();
    frame
        .buffer_mut()
        .set_style(area, Style::default().fg(theme.text).bg(theme.bg));

    let app_title_block = panel_block(
        Line::styled(
            "     IRA (Integrated Retro Archives)    ",
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ),
        true,
        &theme,
    );

    if app.should_increase_size(width, height) {
        frame.render_widget(
            Paragraph::new("Please increase the terminal's size")
                .block(app_title_block)
                .style(Style::default().fg(theme.accent).bg(theme.bg))
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
            Constraint::Length(1), // Status bar
        ])
        .split(frame.area());

    crate::components::drives_ui::render(frame, app, rows[0]);
    crate::components::common_folders_ui::render(frame, app, rows[1]);

    // Bottom status bar: non-error notices (errors use the modal dialog).
    if let Some(status) = &app.status {
        if !status.is_error {
            let bar = Rect {
                x: frame.area().x,
                y: frame.area().y + frame.area().height - 1,
                width: frame.area().width,
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(" ●  {} ", status.text),
                    Style::default().fg(theme.bg).bg(theme.info),
                ))
                .style(Style::default().fg(theme.bg).bg(theme.info)),
                bar,
            );
        }
    }

    // Bottom bar right side: the " * Keybindings " button, plus the
    // contextual hint marquee on the remaining width. Both hide while a
    // modal/text-input state owns the keyboard (app.hint_bar_blocked); the
    // marquee also yields to transient status notices (contextual_hints
    // returns nothing while one is up).
    // Rendered as one pill (`* Keybindings`) so it matches the chips; the
    // pill is always `label + 2` cells wide regardless of chip style.
    let btn_label = "* Keybindings";
    let btn_w = btn_label.len() as u16 + chrome::pill_extra(&theme);
    let bar_y = frame.area().y + frame.area().height - 1;
    if !app.hint_bar_blocked() {
        if frame.area().width > btn_w {
            let btn = Rect {
                x: frame.area().x + frame.area().width - btn_w,
                y: bar_y,
                width: btn_w,
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(Line::from(chrome::key_pill(btn_label, &theme))),
                btn,
            );
        }
        let hints = app.contextual_hints();
        let bar_w = frame.area().width.saturating_sub(btn_w);
        if !hints.is_empty() && bar_w > 2 {
            let spans = chrome::marquee_spans(&hints, bar_w as usize, app.hint_offset, &theme);
            frame.render_widget(
                Paragraph::new(Line::from(spans)),
                Rect {
                    x: frame.area().x,
                    y: bar_y,
                    width: bar_w,
                    height: 1,
                },
            );
        }
    }

    // Bookmarks (left) and Actions (right) share the third row. The four
    // action chips need 58 cols with square pills and 63 with rounded ones
    // (wider pills, narrower gaps); plus the border.
    let actions_w = if theme.chips == crate::theme::ChipStyle::Rounded {
        66
    } else {
        60
    };
    let bookmarks_row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(actions_w)])
        .split(rows[2]);
    crate::components::bookmarks_ui::render(frame, app, bookmarks_row[0]);
    crate::components::actions_ui::render(frame, app, bookmarks_row[1]);

    // Files area: optional Copy Board sidebar on the right, then the panes.
    let mut files_area = rows[3];
    if app.copy_board {
        let board = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(48)])
            .split(files_area);
        crate::components::copy_board_ui::render(frame, app, board[1]);
        files_area = board[0];
    }
    // Preview columns (per-pane `v` column mode) are drawn inside each
    // pane's own renderer; the files area itself is untouched here.
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

    // Keybindings help dialog (`*`): documents the keys that have no
    // on-screen hint. Any key closes it (handler.rs).
    if app.keybindings_visible {
        // One pair per row, no section headers: 13 rows + borders fit the
        // minimum supported terminal (90x15) without clipping.
        let lines: Vec<Line<'static>> = vec![
            chrome::bind_pair(
                "Arrows",
                "navigate; ←/→ open/leave",
                "z / x",
                "top / bottom",
                &theme,
            ),
            chrome::bind_pair(
                "Space",
                "multi-select entry",
                "c",
                "copy to other pane",
                &theme,
            ),
            chrome::bind_pair("m", "move to other pane", "Enter", "rename entry", &theme),
            chrome::bind_pair(
                "Del",
                "delete (with confirm)",
                "n",
                "new folder / file",
                &theme,
            ),
            chrome::bind_pair(".", "toggle hidden files", ",", "cycle sort mode", &theme),
            chrome::bind_pair("v", "cycle image preview", "Tab", "switch pane", &theme),
            chrome::bind_pair(
                "Ctrl+A",
                "select / clear all",
                "Alt+I",
                "invert selection",
                &theme,
            ),
            chrome::bind_pair("+", "split pane", "`", "copy board", &theme),
            chrome::bind_pair("b", "bookmark this folder", "/", "fuzzy search", &theme),
            chrome::bind_pair("Esc", "clear search filter", "?", "entry info", &theme),
            chrome::bind_pair("[", "go to path", "]", "copy folder path", &theme),
            chrome::bind_pair("-", "eject drive", "0", "terminal here", &theme),
            chrome::bind_pair("*", "this help", "q", "quit", &theme),
        ];
        let max_w = lines.iter().map(|l| l.width() as u16).max().unwrap_or(40);
        // Plain panel (no glass chrome): 13 rows + border must fit the
        // minimum supported terminal (90x15) exactly, so the quit and
        // reopen hints are never clipped.
        let w = (max_w + 2).min(frame.area().width.saturating_sub(2));
        let h = (lines.len() as u16 + 2).min(frame.area().height);
        let area = chrome::centered_rect(w, h, frame.area());
        chrome::paint_bg(
            frame,
            area,
            Style::default().fg(theme.text).bg(theme.surface),
        );
        frame.render_widget(
            Paragraph::new(lines)
                .block(chrome::panel_block(
                    Line::styled(
                        " Keybindings ",
                        Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
                    ),
                    true,
                    &theme,
                ))
                .style(Style::default().fg(theme.text).bg(theme.surface)),
            area,
        );
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
            let line = format!("Deleting {}/{} — {} ", del.done, del.total, current);
            let lines = vec![
                Line::raw(line),
                hint_line(&[("any key", " hide (deletion continues)")], &theme),
            ];
            let w = lines
                .iter()
                .map(|l| l.width() as u16)
                .max()
                .unwrap_or(40)
                .max(40);
            let area = dialog_area(w, 2, frame.area());
            render_dialog(frame, "Deleting", lines, DialogKind::Progress, &theme, area);
        }
    }

    // Confirmation prompt overlay (delete): glass dialog over the file list.
    if let Some(confirm) = &app.confirming {
        let verb = match confirm.action {
            crate::app::ConfirmAction::Delete => "Delete",
            crate::app::ConfirmAction::Copy => "Copy",
            crate::app::ConfirmAction::Move => "Move",
        };
        let kind = match confirm.action {
            crate::app::ConfirmAction::Delete => DialogKind::Danger,
            _ => DialogKind::Confirm,
        };
        let mut pairs: Vec<(&str, &str)> = vec![("y", "es"), ("n", "o")];
        let policy: Option<String> = match &confirm.action {
            crate::app::ConfirmAction::Delete => None,
            _ => Some(match confirm.policy {
                crate::services::transfer::OverwritePolicy::AutoRename => {
                    " if exists: auto-rename".to_string()
                }
                crate::services::transfer::OverwritePolicy::Overwrite => {
                    " if exists: overwrite".to_string()
                }
                crate::services::transfer::OverwritePolicy::SkipExisting => {
                    " if exists: skip".to_string()
                }
            }),
        };
        if policy.is_some() {
            pairs.push(("o", policy.as_deref().unwrap()));
        }
        let subject = match &confirm.dest_dir {
            Some(dest) => {
                let dest_name = std::path::Path::new(dest)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| dest.clone());
                format!(" {verb} {} → {dest_name}?  ", confirm.label)
            }
            None => format!(" {verb} {}?  ", confirm.label),
        };
        let mut spans = vec![Span::styled(
            subject,
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )];
        spans.extend(hint_line(&pairs, &theme).spans);
        let prompt = Line::from(spans);
        let w = (prompt.width() as u16).max(40);
        let area = dialog_area(w, 1, frame.area());
        render_dialog(frame, "Confirm", vec![prompt], kind, &theme, area);
    }

    // Rename dialog: in-place text editor with a visible cursor.
    if let Some(prompt) = &app.renaming {
        let text_line = text_input_line(&prompt.text, prompt.cursor, &theme);
        let hint = hint_line(&[("Enter", " rename"), ("Esc", " cancel")], &theme);
        let lines = vec![text_line, Line::raw(""), hint];
        let w = (prompt.text.len() as u16 + 12).max(34);
        let area = dialog_area(w, lines.len() as u16, frame.area());
        render_dialog(frame, "Rename", lines, DialogKind::Input, &theme, area);
    }

    // Go-to-path dialog: paste (Ctrl+V) or type a path; Enter navigates to
    // it or creates the missing chain.
    if let Some(p) = &app.goto_prompt {
        let chars: Vec<char> = p.chars().collect();
        let text_line = text_input_line(&chars, chars.len(), &theme);
        let hint = hint_line(&[("Enter", " go / create"), ("Esc", " cancel")], &theme);
        let lines = vec![text_line, hint];
        let w = (p.chars().count() as u16 + 20).max(52);
        let area = dialog_area(w, lines.len() as u16, frame.area());
        render_dialog(frame, "Go to path", lines, DialogKind::Input, &theme, area);
    }

    // Create-new dialog: live kind preview (folder vs file by extension).
    if let Some(p) = &app.new_entry {
        let name: String = p.text.iter().collect();
        let kind = match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() && !name.contains('/') => {
                format!("file (.{ext})")
            }
            _ if name.contains('/') => "nested folder(s) + file".to_string(),
            _ => "folder".to_string(),
        };
        let text_line = text_input_line(&p.text, p.cursor, &theme);
        let kind_line = Line::styled(
            format!("  new {kind}  "),
            Style::default().fg(theme.text_muted),
        );
        let hint = hint_line(&[("Enter", " create"), ("Esc", " cancel")], &theme);
        let lines = vec![text_line, kind_line, hint];
        let w = (name.len() as u16 + 16).max(34);
        let area = dialog_area(w, lines.len() as u16, frame.area());
        render_dialog(frame, "New", lines, DialogKind::Input, &theme, area);
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
            Line::styled(
                selection,
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
            styled_info_line(&size_line, &theme),
            Line::raw(""),
            hint_line(&[("any key", " close")], &theme),
        ];
        let max_w = lines
            .iter()
            .map(|l| l.width() as u16)
            .max()
            .unwrap_or(30)
            .max(34);
        let area = dialog_area(max_w, lines.len() as u16, frame.area());
        render_dialog(frame, "Info", lines, DialogKind::Info, &theme, area);
    }

    // Info dialog: read-only metadata for the selected entry. The Size line
    // is dynamic: animated partial size while the background walk runs, an
    // honest lower bound after `x`, and the final line once done (drain
    // inserts it into `lines`; the renderer only fills the gap).
    if let Some(info) = &app.info {
        let mut lines: Vec<Line<'static>> = info
            .lines
            .iter()
            .map(|l| styled_info_line(l, &theme))
            .collect();
        if !info.lines.iter().any(|l| l.starts_with("Size:")) {
            let line = match (app.size_walk_started(&info.path), app.size_info(&info.path)) {
                (Some(started), Some(si)) => size_line_partial(si, spinner_char(started)),
                (Some(started), None) => size_line_started(spinner_char(started)),
                (None, Some(si)) => size_line_cancelled(si),
                (None, None) => size_line_started(spinner_char(info.started)),
            };
            lines.insert(4.min(lines.len()), styled_info_line(&line, &theme));
        }
        let is_folder =
            app.size_walk_started(&info.path).is_some() || app.size_info(&info.path).is_some();
        if is_folder {
            lines.push(Line::raw(""));
            lines.push(hint_line(
                &[
                    ("x", " cancel size walk"),
                    ("r", " recalculate"),
                    ("Esc", " close"),
                ],
                &theme,
            ));
        }
        let mut max_w: u16 = 10;
        for l in &lines {
            max_w = max_w.max(l.width() as u16);
        }
        let area = dialog_area(max_w, lines.len() as u16, frame.area());
        render_dialog(frame, "Info", lines, DialogKind::Info, &theme, area);
    }

    // Error dialog: dismissable modal for action failures (eject busy,
    // rename collision, delete IO error...). Red theme; any key closes it
    // (handler.rs treats every key as dismiss while it is open).
    if let Some(status) = &app.status {
        if !status.is_error {
            return; // notices render in the bottom bar; only errors modal here
        }
        let text: Vec<Line<'static>> = vec![
            Line::styled(
                status.text.clone(),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
            Line::raw(""),
            hint_line(&[("any key", " dismiss")], &theme),
        ];
        let max_allowed = (frame.area().width * 4 / 5)
            .saturating_sub(chrome::DIALOG_CHROME)
            .max(30);
        let content_w = text
            .iter()
            .map(|l| l.width() as u16)
            .max()
            .unwrap_or(20)
            .max(30)
            .min(max_allowed);
        let inner_w = content_w.max(1) as usize;
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
        let area = dialog_area(content_w, wrapped as u16, frame.area());
        render_dialog(frame, "Error", text, DialogKind::Error, &theme, area);
    }
}

/// Styles `Label: value` info rows: muted label, accent value while a
/// size walk is still running.
fn styled_info_line(raw: &str, theme: &Theme) -> Line<'static> {
    if let Some((label, value)) = raw.split_once(": ") {
        let value_style = if label == "Size" && is_partial_size(value) {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.text)
        };
        Line::from(vec![
            Span::styled(format!("{label}: "), Style::default().fg(theme.text_muted)),
            Span::styled(value.to_string(), value_style),
        ])
    } else {
        Line::styled(raw.to_string(), Style::default().fg(theme.text))
    }
}

fn is_partial_size(value: &str) -> bool {
    const SPINNER: &str = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";
    value.chars().any(|c| SPINNER.contains(c)) || value.contains("calculating")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::list_files::FEntry;
    use crate::theme::icons::IconSet;
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
        // Size walks persist state on completion; keep it off the real file.
        app.state_path =
            Some(std::env::temp_dir().join(format!("ira-ui-state-{}", std::process::id())));
        app.panes[0].files = vec![FEntry {
            path: dir.join(name).to_string_lossy().into_owned(),
            label: name.to_string(),
            is_dir,
            size: 0,
            modified: None,
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
        assert!(text.contains(" any key  dismiss"), "{text}");

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
        assert!(text.contains(" r  recalculate"), "{text}");
        assert!(text.contains(" x  cancel size walk"));
        assert!(text.contains(" Esc  close"));

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
            text.contains(" x  cancel size walk    r  recalculate    Esc  close"),
            "{text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dimmed_backdrop_keeps_file_list_text() {
        let mut app = App::default();
        app.theme = Theme::default();
        app.icons = IconSet::Unicode;
        app.panes[0].folder = Some(crate::domain::data::Folder::new(
            "tmp".into(),
            "/tmp".into(),
            '#',
        ));
        app.panes[0].files = vec![FEntry {
            path: "/tmp/backdrop-marker.txt".into(),
            label: "backdrop-marker.txt".into(),
            is_dir: false,
            size: 0,
            modified: None,
        }];
        app.panes[0].listing_settled = true;
        app.set_status("Failed to eject /dev/sdd2: target is busy", true);

        let text = rendered(&mut app);
        assert!(text.contains("backdrop-marker.txt"), "{text}");
        assert!(text.contains(" Error "), "{text}");
    }

    #[test]
    fn status_bar_shows_keybindings_button_always() {
        // No status: the button is still visible.
        let mut app = App::default();
        let text = rendered(&mut app);
        assert!(text.contains("* Keybindings"), "{text}");

        // A non-error notice shares the bar without hiding the button.
        let mut app = App::default();
        app.set_status("Copied 2 items", false);
        let text = rendered(&mut app);
        assert!(text.contains("Copied 2 items"), "{text}");
        assert!(text.contains("* Keybindings"), "{text}");
    }

    #[test]
    fn keybindings_dialog_lists_hidden_keys_and_closes_on_any_key() {
        use crate::handler::handle_key_events;
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let key_event = KeyEvent::new(KeyCode::Char('*'), KeyModifiers::empty());

        // `*` opens the dialog; it documents every normal-mode binding that
        // has no on-screen hint. Excluded: drive digits, common-folder and
        // bookmark shortcuts (visible in their boxes) and dialog-internal
        // keys (shown inside their own dialogs).
        let mut app = App::default();
        handle_key_events(key_event, &mut app).unwrap();
        assert!(app.keybindings_visible, "`*` must open the dialog");
        let text = rendered(&mut app);
        assert!(text.contains("this help"), "dialog must render: {text}");
        for binding in [
            "multi-select entry",
            "move to other pane",
            "rename entry",
            "toggle hidden files",
            "cycle sort mode",
            "go to path",
            "copy folder path",
            "fuzzy search",
            "bookmark this folder",
            "eject drive",
            "select / clear all",
            "invert selection",
            "cycle image preview",
            "switch pane",
        ] {
            assert!(text.contains(binding), "missing {binding:?}: {text}");
        }
        assert!(text.contains("terminal here"), "{text}");

        // Any key closes it.
        handle_key_events(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()), &mut app).unwrap();
        assert!(!app.keybindings_visible);
        let text = rendered(&mut app);
        assert!(!text.contains("this help"), "{text}");
    }

    #[test]
    fn backslash_switches_theme_shows_banner_and_actions_chip() {
        use crate::handler::handle_key_events;
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let file = std::env::temp_dir().join(format!("ira-ui-theme-{}", std::process::id()));
        let mut app = App::default();
        app.state_path = Some(file.clone());
        app.icons = IconSet::Unicode;

        let text = rendered(&mut app);
        assert!(
            text.contains(" \\  Theme"),
            "Actions must advertise `\\`: {text}"
        );
        assert!(
            text.contains("Catppuccin Mocha"),
            "title shows preset: {text}"
        );

        handle_key_events(
            KeyEvent::new(KeyCode::Char('\\'), KeyModifiers::empty()),
            &mut app,
        )
        .unwrap();
        let text = rendered(&mut app);
        assert!(
            text.contains("Switched to Cyberpunk 2077"),
            "banner must announce the new theme: {text}"
        );
        assert!(text.contains("Cyberpunk 2077"), "{text}");
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn outline_chips_use_thin_caps_or_parentheses_at_square_width() {
        use crate::theme::ChipStyle;
        let (l, r) = (chrome::OUTLINE_LEFT, chrome::OUTLINE_RIGHT);
        let mut app = App::default();
        app.icons = IconSet::Unicode;
        let square = rendered(&mut app);

        // Nerd glyphs available: thin rounded caps, bold key, no padding.
        app.theme.chips = ChipStyle::Outline;
        app.theme.nerd_glyphs = true;
        let outline = rendered(&mut app);
        let row =
            format!("{l}+{r} Split Pane   {l}`{r} Copy Board   {l}0{r} Terminal   {l}\\{r} Theme");
        assert!(outline.contains(&row), "{outline}");
        assert!(
            outline.contains(&format!("{l}* Keybindings{r}")),
            "{outline}"
        );
        // Same width as square: the label column does not move.
        let col = |s: &str| s.find(" Split Pane").map(|b| s[..b].chars().count());
        assert_eq!(col(&square), col(&outline));

        // Without a Nerd Font the caps degrade to parentheses.
        app.theme.nerd_glyphs = false;
        let ascii = rendered(&mut app);
        assert!(ascii.contains("(+) Split Pane   (`) Copy Board"), "{ascii}");
        assert!(ascii.contains("(* Keybindings)"), "{ascii}");
    }

    #[test]
    fn rounded_chips_keep_a_flat_body_between_the_caps() {
        let (l, r) = (chrome::PILL_LEFT, chrome::PILL_RIGHT);
        let mut app = App::default();
        app.icons = IconSet::Unicode;
        let square = rendered(&mut app);
        assert!(square.contains(" +  Split Pane"), "{square}");

        app.theme.chips = crate::theme::ChipStyle::Rounded;
        let round = rendered(&mut app);
        // Padding survives inside the caps (stadium, not a ball), and the
        // whole Actions row still fits on one line at 100 cols.
        let row = format!(
            "{l} + {r} Split Pane  {l} ` {r} Copy Board  {l} 0 {r} Terminal  {l} \\ {r} Theme"
        );
        assert!(round.contains(&row), "{round}");
        let btn = format!("{l} * Keybindings {r}");
        assert!(round.contains(&btn), "{round}");
        // Same cell count: the grid did not change size.
        assert_eq!(square.chars().count(), round.chars().count());
    }

    #[test]
    fn keybindings_dialog_fits_minimum_supported_terminal() {
        // 90x15 is the smallest terminal the app accepts
        // (should_increase_size): 13 rows + borders must fit, so the quit
        // and reopen hints are never clipped.
        let mut app = App::default();
        app.keybindings_visible = true;
        let backend = TestBackend::new(90, 15);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(&mut app, f)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("this help"), "{text}");
        assert!(text.contains("quit"), "{text}");
        assert!(text.contains("switch pane"), "{text}");
    }

    #[test]
    fn contextual_hint_bar_hides_for_modals_and_notices() {
        let mut app = App::default();
        assert!(!app.hint_bar_blocked());
        assert_eq!(app.contextual_hints().len(), 10);

        // Any modal/text-input state owns the keyboard: bar (and button) hide.
        app.start_goto();
        assert!(app.hint_bar_blocked());
        assert!(app.contextual_hints().is_empty());
        app.cancel_goto();
        assert!(!app.hint_bar_blocked());

        // The focused preview text editor captures every key: bar hides.
        app.edit_focus = true;
        assert!(app.hint_bar_blocked());
        assert!(app.contextual_hints().is_empty());
        app.edit_focus = false;
        assert!(!app.hint_bar_blocked());

        // A transient notice owns the bar instead of the hints.
        app.set_status("Copied 2 items", false);
        assert!(!app.hint_bar_blocked(), "the button stays with a notice");
        assert!(app.contextual_hints().is_empty());
    }

    #[test]
    fn marquee_is_static_when_it_fits_and_scrolls_when_not() {
        use crate::theme::Theme;
        let text = |items: &[(&str, &str)], w: usize, off: usize| -> String {
            chrome::marquee_spans(items, w, off, &Theme::default())
                .iter()
                .map(|s| s.content.as_ref())
                .collect()
        };

        // Fits: the same line regardless of offset (no scrolling).
        let short: Vec<(&str, &str)> = vec![("c", "copy")];
        assert_eq!(text(&short, 100, 0).trim_end(), " c copy");

        // Too long: a window of `width` chars, advancing with the offset,
        // cyclically wrapping so every hint eventually shows.
        let long: Vec<(&str, &str)> = vec![("a", "one"), ("b", "two")];
        assert_eq!(text(&long, 4, 0).chars().count(), 4);
        assert_ne!(text(&long, 4, 0), text(&long, 4, 2));
        assert_eq!(text(&long, 4, 17), text(&long, 4, 0), "wraps at unit len");
    }

    #[test]
    fn render_paints_no_hint_bar_while_a_modal_is_open() {
        // Goto dialog open: neither the marquee hints nor the [*] button may
        // occupy the bottom row.
        let mut app = App::default();
        app.start_goto();
        let bottom_row: String = {
            let buf = {
                let backend = TestBackend::new(100, 30);
                let mut terminal = Terminal::new(backend).unwrap();
                terminal.draw(|f| render(&mut app, f)).unwrap();
                terminal.backend().buffer().clone()
            };
            let y = 29;
            (0..100)
                .map(|x| buf[(x, y)].symbol())
                .collect::<Vec<_>>()
                .join("")
        };
        assert!(
            !bottom_row.contains("copy") && !bottom_row.contains("Keybindings"),
            "bottom row must be empty under a modal: {bottom_row:?}"
        );

        // Back to normal mode: the bar returns.
        app.cancel_goto();
        let text = rendered(&mut app);
        assert!(text.contains("copy"), "{text}");
        assert!(text.contains("* Keybindings"), "{text}");
    }
}
