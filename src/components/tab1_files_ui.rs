use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, List, ListState, Paragraph},
    Frame,
};

use crate::app::App;

/// Renders one file-browser pane. When `active`, the pane shows its cursor and
/// any active search filter; otherwise it renders dimmed with no cursor.
pub fn render(f: &mut Frame, app: &mut App, area: Rect, pane_index: usize, active: bool) {
    let (title, file_spans) = {
        let pane = &app.panes[pane_index];
        let Some(folder) = &pane.folder else {
            let block = Block::bordered().title("  Files  ");
            f.render_widget(Paragraph::new("").block(block), area);
            return;
        };

        let title = match (active, app.search_query.as_deref()) {
            (true, Some(q)) => format!("  Files  {}  {}  /{}", folder.label, folder.path, q),
            _ => format!("  Files  {}  {}", folder.label, folder.path),
        };

        let spans: Vec<Span> = if active && app.is_searching() {
            app.visible_rows()
                .iter()
                .map(|(i, f)| row_span(pane.selected.get(*i).copied().unwrap_or(false), f))
                .collect()
        } else {
            pane.files
                .iter()
                .enumerate()
                .map(|(i, f)| row_span(pane.selected.get(i).copied().unwrap_or(false), f))
                .collect()
        };

        (title, spans)
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

    if active {
        f.render_stateful_widget(list, area, &mut app.panes[pane_index].state);
    } else {
        let mut inactive_state = ListState::default();
        f.render_stateful_widget(list, area, &mut inactive_state);
    }
}

fn row_span(selected: bool, file: &crate::services::list_files::FEntry) -> Span<'static> {
    let mark = if selected { "[*]" } else { "[ ]" };
    let icon = icon_for(file.is_dir);
    Span::raw(format!(" {mark} {icon} {}", file.label))
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

        let d = row_span(false, &dir).content.to_string();
        let f = row_span(false, &file).content.to_string();
        assert!(d.contains('□'), "folder row should carry the folder square: {d:?}");
        assert!(f.contains('·'), "file row should carry the file dot: {f:?}");
        assert_ne!(d, f, "folder and file rows must render differently");
    }
}
