use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::Block,
    Frame,
};

use crate::app::App;

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    if let Some(folders) = &app.folders {
        let folder_spans: Vec<Span> = folders
            .iter()
            .map(|folder| {
                Span::raw(format!(" [{}] {}", folder.shortcut, folder.label))
                    .style(Style::new().green())
            })
            .collect();

        let text = Line::from(folder_spans);
        let dlist = ratatui::widgets::Paragraph::new(text).block(
            Block::default()
                .title(" Common folders ")
                .borders(ratatui::widgets::Borders::ALL),
        );

        f.render_widget(dlist, area);
    } else {
        f.render_widget(
            ratatui::widgets::Paragraph::new(Line::from(vec![Span::raw(
                "No common folders found",
            )]))
            .block(
                Block::default()
                    .title(" Common folders ")
                    .borders(ratatui::widgets::Borders::ALL),
            ),
            area,
        )
    }
}
