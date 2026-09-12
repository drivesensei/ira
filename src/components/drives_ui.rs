use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::app::App;
use crate::theme::icons::{drive_icon, pad_icon};
use crate::ui::chrome::{key_hint, panel_block};

pub fn render(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let icons = app.icons;
    if let Some(ref drives) = app.drives {
        let keys: Vec<String> = (1..=drives.len()).map(|i| i.to_string()).collect();
        let mut drive_spans: Vec<Span> = Vec::new();
        for (i, drive) in drives.iter().enumerate() {
            if i > 0 {
                drive_spans.push(Span::raw("  "));
            }
            drive_spans.extend(key_hint(&keys[i], "", &theme));
            drive_spans.push(Span::raw(" "));
            drive_spans.push(Span::styled(
                pad_icon(drive_icon(icons), icons),
                Style::default().fg(theme.dir),
            ));
            // Same icon/name gap as the file list; pad_icon alone only adds
            // one for the Nerd set.
            drive_spans.push(Span::raw(" "));
            drive_spans.push(Span::styled(
                drive.label.clone(),
                Style::default().fg(theme.text),
            ));
        }

        let mut title = vec![Span::raw(" Drives  ")];
        title.extend(key_hint("-", " unmount the current drive", &theme));
        let dlist = Paragraph::new(Line::from(drive_spans)).block(panel_block(
            Line::from(title),
            true,
            &theme,
        ));

        f.render_widget(dlist, area);
    } else {
        f.render_widget(Span::raw("no drives"), area)
    }
}
