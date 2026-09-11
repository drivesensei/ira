use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::App;
use crate::ui::chrome::{hint_line, panel_block};

/// Renders the "Actions" box, showing available pane actions and their keys.
/// The title carries the active theme preset so `\` has visible feedback
/// even after the banner expires.
pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let title = Line::from(vec![
        Span::raw(" Actions "),
        Span::styled(
            format!("· {} ", app.theme_preset.label()),
            Style::default().fg(theme.text_muted),
        ),
    ]);
    let widget = Paragraph::new(hint_line(
        &[
            ("+", " Split Pane"),
            ("`", " Copy Board"),
            ("0", " Terminal"),
            ("\\", " Theme"),
        ],
        &theme,
    ))
    .block(panel_block(title, true, &theme));
    f.render_widget(widget, area);
}
