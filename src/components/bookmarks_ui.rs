use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Block,
    Frame,
};

use crate::{app::App, domain::data::Folder};

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let folders = vec![
        Folder::new(String::from("Projects"), String::from("~/projects"), 'y'),
        Folder::new(String::from(".ssh"), String::from("~/.ssh"), 'u'),
    ];

    let folder_spans: Vec<Span> = folders
        .iter()
        .enumerate()
        .map(|(i, folder)| {
            if i + 1 == folders.len() {
                Span::raw(format!(" [{}] {}", folder.shortcut, folder.label))
            } else {
                Span::raw(format!(" [{}] {} |", folder.shortcut, folder.label))
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
