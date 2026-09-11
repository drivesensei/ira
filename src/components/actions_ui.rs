use ratatui::{layout::Rect, text::Line, widgets::Paragraph, Frame};

use crate::app::App;
use crate::ui::chrome::{hint_line, panel_block};

/// Renders the "Actions" box, showing available pane actions and their keys.
pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let widget = Paragraph::new(hint_line(
        &[
            ("+", " Split Pane"),
            ("`", " Copy Board"),
            ("0", " Terminal"),
        ],
        &theme,
    ))
    .block(panel_block(Line::raw(" Actions "), true, &theme));
    f.render_widget(widget, area);
}
