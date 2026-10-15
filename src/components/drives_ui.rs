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
use crate::utils::bytes::format_bytes;

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
            if let Some(space) = drive.space {
                let text = match space.available {
                    Some(avail) => format!(
                        " {} free / {}",
                        format_bytes(avail),
                        format_bytes(space.total)
                    ),
                    None => format!(" {}", format_bytes(space.total)),
                };
                drive_spans.push(Span::styled(text, Style::default().fg(theme.text_muted)));
            }
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

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use crate::app::App;
    use crate::domain::data::{DiskSpace, Folder};

    #[test]
    fn renders_free_and_total_space() {
        let mut app = App::default();
        let mut root = Folder::new("/".to_string(), "/".to_string(), '1');
        root.space = Some(DiskSpace {
            total: 500 * 1024u64.pow(3),
            available: Some(120 * 1024u64.pow(3)),
        });
        let mut unmounted = Folder::new("USB".to_string(), String::new(), '2');
        unmounted.space = Some(DiskSpace {
            total: 2 * 1024u64.pow(4),
            available: None,
        });
        app.drives = Some(vec![root, unmounted]);

        let mut terminal = Terminal::new(TestBackend::new(100, 3)).unwrap();
        terminal
            .draw(|f| super::render(f, &mut app, f.area()))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("free /"), "{text}");
        assert!(text.contains("120G free / 500G"), "{text}");
        assert!(text.contains("2.0T"), "{text}");
    }
}
