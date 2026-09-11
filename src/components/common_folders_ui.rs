use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::App;
use crate::theme::icons::{common_folder_icon, pad_icon};
use crate::ui::chrome::{key_hint, panel_block};

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let icons = app.icons;
    if let Some(folders) = &app.folders {
        let keys: Vec<String> = folders.iter().map(|f| f.shortcut.to_string()).collect();
        let mut folder_spans: Vec<Span> = Vec::new();
        for (i, folder) in folders.iter().enumerate() {
            if i > 0 {
                folder_spans.push(Span::raw("  "));
            }
            folder_spans.extend(key_hint(&keys[i], "", &theme));
            folder_spans.push(Span::raw(" "));
            folder_spans.push(Span::styled(
                pad_icon(common_folder_icon(&folder.label, icons), icons),
                Style::default().fg(theme.dir),
            ));
            folder_spans.push(Span::styled(
                folder.label.clone(),
                Style::default().fg(theme.text),
            ));
        }

        let dlist = Paragraph::new(Line::from(folder_spans)).block(panel_block(
            Line::raw(" Common folders "),
            true,
            &theme,
        ));

        f.render_widget(dlist, area);
    } else {
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                "No common folders found",
                Style::default().fg(theme.text_muted),
            )]))
            .block(panel_block(Line::raw(" Common folders "), true, &theme)),
            area,
        )
    }
}
