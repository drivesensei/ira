use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Block,
    Frame,
};

use crate::app::App;

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    if let Some(bookmarks) = &app.bookmarks {
        let folder_spans: Vec<Span> = bookmarks
            .iter()
            .enumerate()
            .map(|(i, folder)| {
                if i + 1 == bookmarks.len() {
                    Span::raw(format!("[{}] {}", folder.shortcut, folder.label))
                } else {
                    Span::raw(format!("[{}] {} ", folder.shortcut, folder.label))
                }
            })
            .collect();

        let text = Line::from(folder_spans);
        let dlist = ratatui::widgets::Paragraph::new(text).block(
            Block::default()
                .title(" Bookmarks ")
                .borders(ratatui::widgets::Borders::BOTTOM),
        );

        f.render_widget(dlist, area);
    }
}
