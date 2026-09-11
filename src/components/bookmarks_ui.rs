use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::App;
use crate::theme::icons::{bookmark_icon, pad_icon};
use crate::ui::chrome::{key_hint, panel_block};

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let icons = app.icons;
    if let Some(bookmarks) = &app.bookmarks {
        let keys: Vec<String> = bookmarks.iter().map(|f| f.shortcut.to_string()).collect();
        let mut folder_spans: Vec<Span> = Vec::new();
        for (i, folder) in bookmarks.iter().enumerate() {
            if i > 0 {
                folder_spans.push(Span::raw("  "));
            }
            folder_spans.extend(key_hint(&keys[i], "", &theme));
            folder_spans.push(Span::raw(" "));
            folder_spans.push(Span::styled(
                pad_icon(bookmark_icon(icons), icons),
                Style::default().fg(theme.accent),
            ));
            folder_spans.push(Span::styled(
                folder.label.clone(),
                Style::default().fg(theme.text),
            ));
        }

        let dlist = Paragraph::new(Line::from(folder_spans)).block(panel_block(
            Line::raw(" Bookmarks "),
            true,
            &theme,
        ));

        f.render_widget(dlist, area);
    }
}
