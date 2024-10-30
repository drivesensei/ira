use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, List},
    Frame,
};

use crate::app::App;

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    if let Some(maybe_label) = &app.tab1_folder {
        let label = &maybe_label.label;
        let file_spans: Vec<Span> = app
            .tab1_files
            .iter()
            .map(|file| Span::raw(format!(" {}", &file.label)))
            .collect();

        let list = List::new(file_spans)
            .block(Block::bordered().title(format!("  Files  ({})", label)))
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
            .highlight_symbol("→")
            .repeat_highlight_symbol(true);

        f.render_stateful_widget(list, area, &mut app.tab1_state);
    }
}
