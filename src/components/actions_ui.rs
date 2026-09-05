use ratatui::{
    layout::Rect,
    widgets::{Block, Paragraph},
    Frame,
};

use crate::app::App;

/// Renders the "Actions" box, showing available pane actions and their keys.
pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let _ = app;
    let widget = Paragraph::new("[+] Split Pane   [`] Copy Board   [0] Terminal")
        .block(Block::bordered().title(" Actions "));
    f.render_widget(widget, area);
}
