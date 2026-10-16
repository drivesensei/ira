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
        let keys: Vec<String> = drives
            .iter()
            .enumerate()
            .map(|(i, d)| {
                if d.shortcut.is_ascii_alphanumeric() {
                    d.shortcut.to_string()
                } else {
                    (i + 1).to_string()
                }
            })
            .collect();
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
        let icloud = Folder::new("iCloud".to_string(), "/Users/test/iCloud".to_string(), '9');
        app.drives = Some(vec![root, unmounted, icloud]);

        let mut terminal = Terminal::new(TestBackend::new(120, 3)).unwrap();
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
        assert!(text.contains("9") && text.contains("iCloud"), "{text}");
    }

    #[test]
    fn pressing_9_selects_icloud_drive_without_matching_unused_digits() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        use crate::handler::handle_key_events;

        let mut app = App::default();
        let hd = Folder::new(
            "Macintosh HD".to_string(),
            "/Volumes/Macintosh HD".to_string(),
            '1',
        );
        let icloud = Folder::new(
            "iCloud".to_string(),
            "/Users/test/Library/Mobile Documents/com~apple~CloudDocs".to_string(),
            '9',
        );
        app.drives = Some(vec![hd, icloud]);

        // Pressing '2' (not assigned) must not jump to iCloud (index 1).
        handle_key_events(
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE),
            &mut app,
        )
        .unwrap();
        assert_ne!(
            app.panes[0].folder.as_ref().map(|f| f.label.as_str()),
            Some("iCloud")
        );

        // Pressing '9' selects iCloud even though it is at index 1.
        handle_key_events(
            KeyEvent::new(KeyCode::Char('9'), KeyModifiers::NONE),
            &mut app,
        )
        .unwrap();
        assert_eq!(
            app.panes[0].folder.as_ref().map(|f| f.label.as_str()),
            Some("iCloud")
        );
        assert_eq!(
            app.panes[0].folder.as_ref().map(|f| f.path.as_str()),
            Some("/Users/test/Library/Mobile Documents/com~apple~CloudDocs")
        );
    }
}
