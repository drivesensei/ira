use std::time::Instant;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, List, ListState, Paragraph},
    Frame,
};

use crate::app::App;
use crate::services::file_info::{list_note, spinner_char, SizeInfo};

/// Renders one file-browser pane. When `active`, the pane shows its cursor and
/// any active search filter; otherwise it renders dimmed with no cursor.
pub fn render(f: &mut Frame, app: &mut App, area: Rect, pane_index: usize, active: bool) {
    // Rows visible inside the bordered block.
    let height = area.height.saturating_sub(2) as usize;

    let (title, file_spans, window_start, cursor) = {
        let pane = &app.panes[pane_index];
        let Some(folder) = &pane.folder else {
            let block = Block::bordered().title("  ");
            f.render_widget(Paragraph::new("").block(block), area);
            return;
        };

        let title = if active {
            match (&app.search_query, &pane.filter_query) {
                (Some(q), _) => {
                    format!("  {}  {}  /{}", folder.label, folder.path, q)
                }
                (None, Some(q)) => format!(
                    "  {}  {}  /{}  (Esc clears)",
                    folder.label, folder.path, q
                ),
                _ => format!("  {}  {}  ", folder.label, folder.path),
            }
        } else {
            format!("  {}  {}  ", folder.label, folder.path)
        };

        // Folders being measured show an animated spinner as their icon;
        // folders with a completed measurement carry a size annotation
        // after their name.
        let walk_started = |f: &crate::services::list_files::FEntry| {
            app.size_walk_started(&f.path).filter(|_| f.is_dir)
        };
        let size_note = |f: &crate::services::list_files::FEntry| {
            app.size_info(&f.path).filter(|s| s.complete && f.is_dir)
        };
        // Folders queued for background deletion show the spinner too.
        let deleting_started = |f: &crate::services::list_files::FEntry| {
            app.deleting_started(&f.path).filter(|_| f.is_dir)
        };

        // Build spans ONLY for the visible window: with 200k+ entries, one
        // Span allocation per row per frame is what makes the UI feel stuck.
        // `pane.render_scroll` (kept by the integrator on `Pane`) pins the
        // window across frames; the helper keeps the cursor inside it.
        // While a listing is still streaming in (first frames after
        // entering a folder), show a Loading hint instead of a partial
        // unsorted view.
        let selected = pane.state.selected();
        let loading = !pane.listing_settled && pane.files.is_empty();
        let filtered = pane.filter_query.is_some() && !pane.filter_indices.is_empty();
        let (start, file_spans) = if loading {
            (0, vec![Span::raw(" Loading…").style(Style::new().dim())])
        } else if filtered || (active && app.is_searching()) {
            let rows = app.pane_visible_rows(pane_index);
            let (start, end) = visible_window(rows.len(), selected, pane.render_scroll, height);
            let spans: Vec<Span> = rows[start..end]
                .iter()
                .map(|(i, f)| {
                    row_span(
                        pane.selected.get(*i).copied().unwrap_or(false),
                        f,
                        walk_started(f),
                        size_note(f),
                        deleting_started(f),
                    )
                })
                .collect();
            (start, spans)
        } else {
            let (start, end) =
                visible_window(pane.files.len(), selected, pane.render_scroll, height);
            let spans: Vec<Span> = pane.files[start..end]
                .iter()
                .enumerate()
                .map(|(rel, f)| {
                    row_span(
                        pane.selected.get(start + rel).copied().unwrap_or(false),
                        f,
                        walk_started(f),
                        size_note(f),
                        deleting_started(f),
                    )
                })
                .collect();
            (start, spans)
        };

        (title, file_spans, start, selected.map(|s| s - start))
    };

    let border_style = if active {
        Style::default()
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let list = List::new(file_spans)
        .block(Block::bordered().title(title).border_style(border_style))
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .highlight_symbol("→")
        .repeat_highlight_symbol(true);

    // Fresh ListState each frame: the window offset is ours (`render_scroll`),
    // and `cursor` is already relative to the rendered slice.
    let mut render_state = ListState::default();
    if active {
        render_state.select(cursor);
        let pane = &mut app.panes[pane_index];
        pane.render_scroll = window_start;
        f.render_stateful_widget(list, area, &mut render_state);
    } else {
        f.render_stateful_widget(list, area, &mut render_state);
    }
}

/// Computes the `(start, end)` row window to render for a list of `total`
/// rows, keeping the cursor row (`selected`) inside a viewport of `height`
/// rows, seeded from the previous window start (`scroll`). Clamps `end` to
/// `total` and a stale cursor to `total - 1`. Returns `(0, 0)` for an empty
/// list or zero height.
fn visible_window(
    total: usize,
    selected: Option<usize>,
    scroll: usize,
    height: usize,
) -> (usize, usize) {
    if total == 0 || height == 0 {
        return (0, 0);
    }
    // No cursor: park the window at `scroll`, clamped so a full viewport
    // fits inside the list.
    let selected = selected
        .map(|s| s.min(total - 1))
        .unwrap_or_else(|| scroll.min(total.saturating_sub(height)));
    let start = if selected < scroll {
        selected
    } else if selected >= scroll + height {
        selected + 1 - height
    } else {
        scroll
    };
    (start, (start + height).min(total))
}

fn row_span(
    selected: bool,
    file: &crate::services::list_files::FEntry,
    walking: Option<Instant>,
    note: Option<&SizeInfo>,
    deleting: Option<Instant>,
) -> Span<'static> {
    let mark = if selected { "[*]" } else { "[ ]" };
    let icon = match (deleting, walking) {
        (Some(started), _) => spinner_char(started).to_string(),
        (None, Some(started)) => spinner_char(started).to_string(),
        (None, None) => icon_for(file.is_dir).to_string(),
    };
    let label = match note {
        Some(si) => format!("{} ({})", file.label, list_note(si)),
        None => file.label.clone(),
    };
    Span::raw(format!(" {mark} {icon} {label}"))
}

/// A dependency-free glyph: a folder marker for directories and a plain-file
/// marker otherwise. Both are single-width Unicode from the geometric block
/// (U+25A0..) that every terminal font — DejaVu, Menlo, Consolas, Cascadia —
/// covers identically, so rows stay aligned across platforms.
fn icon_for(is_dir: bool) -> &'static str {
    if is_dir {
        "□" // U+25A1 white square: box-like, clearly distinct from the dot
    } else {
        "·" // U+00B7 middle dot
    }
}

#[cfg(test)]
mod tests {
    use super::row_span;
    use super::visible_window;
    use crate::services::list_files::FEntry;

    #[test]
    fn folder_and_file_rows_have_distinct_icons() {
        let dir = FEntry {
            path: "/x/src".to_string(),
            label: "src".to_string(),
            is_dir: true,
        };
        let file = FEntry {
            path: "/x/r.txt".to_string(),
            label: "r.txt".to_string(),
            is_dir: false,
        };

        let d = row_span(false, &dir, None, None, None).content.to_string();
        let f = row_span(false, &file, None, None, None).content.to_string();
        assert!(
            d.contains('□'),
            "folder row should carry the folder square: {d:?}"
        );
        assert!(f.contains('·'), "file row should carry the file dot: {f:?}");
        assert_ne!(d, f, "folder and file rows must render differently");
    }

    #[test]
    fn completed_size_annotates_the_folder_name() {
        use crate::services::file_info::SizeInfo;
        use std::time::SystemTime;
        let dir = FEntry {
            path: "/x/cybertouch".to_string(),
            label: "cybertouch".to_string(),
            is_dir: true,
        };
        let si = SizeInfo {
            bytes: 322_000_000_000,
            items: 248_662,
            on_disk: 400_000_000_000,
            complete: true,
            updated: SystemTime::now(),
        };
        let row = row_span(false, &dir, None, Some(&si), None)
            .content
            .to_string();
        assert!(
            row.starts_with(" [ ] □ cybertouch ("),
            "folder row keeps mark and icon: {row:?}"
        );
        assert!(row.contains("cybertouch ("), "{row:?}");
        assert!(
            row.contains("data / 372.5 GiB on disk - last updated: "),
            "{row:?}"
        );
        assert!(!row.contains("before 1970"), "{row:?}");
    }

    #[test]
    fn walking_folder_shows_spinner_icon() {
        use std::time::Instant;
        let dir = FEntry {
            path: "/x/src".to_string(),
            label: "src".to_string(),
            is_dir: true,
        };
        let plain = row_span(false, &dir, None, None, None).content.to_string();
        let walking = row_span(false, &dir, Some(Instant::now()), None, None)
            .content
            .to_string();
        assert_ne!(plain, walking, "walking folder swaps its icon");
        assert!(walking.contains("src"));
    }

    #[test]
    fn window_follows_cursor_down() {
        // Cursor walks past the bottom edge; the window slides just enough
        // to keep the cursor on the last visible row.
        assert_eq!(visible_window(1000, Some(0), 0, 20), (0, 20));
        assert_eq!(visible_window(1000, Some(19), 0, 20), (0, 20));
        assert_eq!(visible_window(1000, Some(20), 0, 20), (1, 21));
        // Stepping further, seeded from the previous window start.
        assert_eq!(visible_window(1000, Some(45), 1, 20), (26, 46));
        assert_eq!(visible_window(1000, Some(45), 26, 20), (26, 46));
    }

    #[test]
    fn window_jumps_up_when_cursor_moves_above_scroll() {
        // Cursor jumps to the top (e.g. Home): the window snaps back.
        assert_eq!(visible_window(1000, Some(0), 500, 20), (0, 20));
        assert_eq!(visible_window(1000, Some(5), 10, 20), (5, 25));
    }

    #[test]
    fn window_clamps_to_list_bounds() {
        // Fewer rows than the viewport: window is the whole list.
        assert_eq!(visible_window(10, Some(8), 0, 20), (0, 10));
        // Cursor near the end: end clamps to total, window still full-height
        // where possible.
        assert_eq!(visible_window(1000, Some(999), 0, 20), (980, 1000));
        // Stale cursor beyond the list (list shrank underneath it).
        assert_eq!(visible_window(100, Some(150), 0, 20), (80, 100));
    }

    #[test]
    fn window_handles_empty_list_and_zero_height() {
        assert_eq!(visible_window(0, Some(0), 0, 20), (0, 0));
        assert_eq!(visible_window(0, None, 0, 20), (0, 0));
        assert_eq!(visible_window(100, Some(3), 0, 0), (0, 0));
        // No selection keeps the window parked at the clamped scroll.
        assert_eq!(visible_window(1000, None, 30, 20), (30, 50));
        assert_eq!(visible_window(10, None, 30, 20), (0, 10));
    }
}
