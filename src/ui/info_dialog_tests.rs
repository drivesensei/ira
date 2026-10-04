use super::*;
use crate::app::InfoDialog;
use ratatui::{backend::TestBackend, buffer::Buffer, widgets::Wrap, Terminal};
use std::time::Instant;

fn footer(theme: &Theme) -> Line<'static> {
    hint_line(
        &[
            ("x", " cancel size walk"),
            ("r", " recalculate"),
            ("Esc", " close"),
        ],
        theme,
    )
}

fn plain(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn compact(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn content_text(buffer: &Buffer, area: Rect) -> String {
    let mut text = String::new();
    for y in area.y + 2..area.y + area.height.saturating_sub(2) {
        for x in area.x + 2..area.x + area.width.saturating_sub(2) {
            text.push_str(buffer[(x, y)].symbol());
        }
    }
    text
}

fn metadata(path: &str) -> Vec<String> {
    vec![
        "Name: owned-fixture".into(),
        format!("Path: {path}"),
        "Type: Directory".into(),
        "Modified: 2026-10-04 12:00".into(),
        "Size: 12 B data / 4 KiB on disk (1 items)".into(),
    ]
}

// Equivalent already-composed folder content is injected into InfoDialog.
// This exercises the real Info layout branch without starting metadata or
// size workers. It does not test App's private folder/cache predicate.
#[test]
fn synthetic_long_info_paths_preserve_metadata_and_equivalent_footer() {
    let paths = [
        format!(
            "/owned/regression-evidence/{}/TAIL-folder",
            "normal-component/".repeat(10)
        ),
        format!(
            "/owned/regression evidence/{}/TAIL folder",
            "long directory with spaces/".repeat(7)
        ),
        format!(
            "/owned/regression/{}/TAIL-界-e\u{301}",
            "資料界面-e\u{301}/".repeat(12)
        ),
    ];
    for path in paths {
        assert!((140..=260).contains(&Line::raw(path.clone()).width()));
        for (width, height) in [(90, 30), (100, 30), (160, 40)] {
            let mut app = App::default();
            let mut lines = metadata(&path);
            lines.push(String::new());
            let hint = plain(&footer(&app.theme));
            lines.push(hint.clone());
            let expected = lines.concat();
            app.info = Some(InfoDialog {
                path: path.clone(),
                lines,
                pending: false,
                started: Instant::now(),
            });
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| render(&mut app, frame)).unwrap();
            let buffer = terminal.backend().buffer();
            let styled: Vec<_> = app
                .info
                .as_ref()
                .unwrap()
                .lines
                .iter()
                .map(|line| styled_info_line(line, &app.theme))
                .collect();
            let area = chrome::wrapped_info_area(&styled, buffer.area);
            let text = content_text(buffer, area);
            assert!(
                compact(&text).contains(&compact(&expected)),
                "{width}x{height}: {text}"
            );
            assert!(text.contains(&hint), "footer must fit contiguously: {text}");
        }
    }
}

#[test]
fn short_file_info_has_no_folder_footer() {
    let mut app = App::default();
    app.info = Some(InfoDialog {
        path: "/owned/file.txt".into(),
        lines: vec![
            "Name: file.txt".into(),
            "Path: /owned/file.txt".into(),
            "Size: 3 B".into(),
        ],
        pending: false,
        started: Instant::now(),
    });
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| render(&mut app, frame)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Path: /owned/file.txt"));
    assert!(!text.contains("recalculate"));
    assert!(!text.contains("cancel size walk"));
}

#[test]
fn narrow_info_uses_word_reflow_and_preserves_styled_unicode_and_footer() {
    let theme = Theme::default();
    let lines = vec![
        styled_info_line("Path: abcdefgh ijklmnop qrstuvwx", &theme),
        styled_info_line("Name: 資料界面 e\u{301} e\u{301} 資料", &theme),
        Line::raw(""),
        footer(&theme),
    ];
    let frame = Rect::new(0, 0, 22, 40);
    let area = chrome::wrapped_info_area(&lines, frame);
    let content_w = area.width - chrome::DIALOG_CHROME;
    assert_eq!(content_w, 16);
    let exact = Paragraph::new(lines.clone())
        .wrap(Wrap { trim: false })
        .line_count(content_w);
    let naive: usize = lines
        .iter()
        .map(|line| line.width().div_ceil(content_w as usize).max(1))
        .sum();
    assert!(
        exact > naive,
        "fixture must distinguish word reflow from ceil: {exact}/{naive}"
    );
    assert_eq!(area.height as usize, exact + chrome::DIALOG_CHROME as usize);
    let mut terminal = Terminal::new(TestBackend::new(frame.width, frame.height)).unwrap();
    terminal
        .draw(|frame| render_dialog(frame, "Info", lines.clone(), DialogKind::Info, &theme, area))
        .unwrap();
    let text = content_text(terminal.backend().buffer(), area);
    let expected: String = lines.iter().map(plain).collect();
    assert!(compact(&text).contains(&compact(&expected)), "{text}");
}

#[test]
fn info_measurement_saturates_and_clamps_tiny_or_overfull_viewports() {
    let lines = vec![
        Line::raw("界".repeat(40_000)),
        Line::raw(""),
        Line::raw("footer"),
    ];
    for (width, height) in [(0, 0), (1, 1), (4, 4), (8, 8), (100, 15)] {
        let frame = Rect::new(0, 0, width, height);
        let area = chrome::wrapped_info_area(&lines, frame);
        assert!(area.right() <= frame.right());
        assert!(area.bottom() <= frame.bottom());
        assert!(area.width <= width.saturating_sub(2).max(1).min(width));
        assert!(area.height <= height.saturating_sub(2).max(1).min(height));
        // Complete content cannot fit these viewports; only bounded geometry
        // is asserted, not an impossible guarantee of footer visibility.
    }
}
