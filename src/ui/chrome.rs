//! Shared chrome: rounded panels, key-binding chips, glass dialogs.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
    Frame,
};

use crate::theme::Theme;

/// Rounded panel used by every chrome box.
pub fn panel_block<'a>(title: Line<'a>, active: bool, theme: &Theme) -> Block<'a> {
    let border = if active {
        theme.border_active
    } else {
        theme.border
    };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .title(title)
        .title_style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD))
        .style(Style::default().fg(theme.text))
}

/// `[key]` chip (bold `key_fg` on `key_bg`) plus a muted label.
///
/// Concatenated text stays `[key]{label}` so existing buffer-text tests
/// (`[y]es`, `[any key] dismiss`, …) keep matching.
pub fn key_hint<'a>(key: &'a str, label: &'a str, theme: &Theme) -> Vec<Span<'a>> {
    vec![
        Span::styled(
            format!("[{key}]"),
            Style::default()
                .fg(theme.key_fg)
                .bg(theme.key_bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(label.to_string(), Style::default().fg(theme.text_muted)),
    ]
}

/// Several key chips on one line, joined by three spaces so the hint
/// string `[x] cancel size walk   [r] recalculate   [Esc] close` survives
/// as contiguous buffer text.
pub fn hint_line<'a>(pairs: &[(&'a str, &'a str)], theme: &Theme) -> Line<'a> {
    let mut spans: Vec<Span<'a>> = Vec::new();
    for (i, (key, label)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
        }
        spans.extend(key_hint(key, label, theme));
    }
    Line::from(spans)
}

/// Builds the contextual hint bar as styled cells: accent-colored keys,
/// muted descriptions. When the joined line is wider than `width` it
/// scrolls marquee-style: `offset` chars into the line, cyclically
/// repeated, so narrow terminals still see every hint over time.
pub fn marquee_spans(
    items: &[(&str, &str)],
    width: usize,
    offset: usize,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let key_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(theme.text_muted);
    let mut unit: Vec<(char, Style)> = vec![(' ', desc_style)];
    for (key, desc) in items {
        if unit.len() > 1 {
            unit.extend(std::iter::repeat_n((' ', desc_style), 3));
        }
        unit.extend(key.chars().map(|c| (c, key_style)));
        unit.push((' ', key_style));
        unit.extend(desc.chars().map(|c| (c, desc_style)));
    }
    // Trailing separator: keeps a gap where the cycle wraps around.
    unit.extend(std::iter::repeat_n((' ', desc_style), 3));
    let n = unit.len();
    if n == 0 {
        return Vec::new();
    }
    let to_span = |(c, s): (char, Style)| Span::styled(c.to_string(), s);
    if n <= width {
        return unit.into_iter().map(to_span).collect();
    }
    (0..width)
        .map(|i| to_span(unit[(offset.wrapping_add(i)) % n]))
        .collect()
}

/// One keybindings-dialog row: two `[key] description` columns; the key
/// cells are bold accent.
pub fn bind_pair(k1: &str, d1: &str, k2: &str, d2: &str, theme: &Theme) -> Line<'static> {
    let key = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    Line::from(vec![
        Span::styled(format!(" {k1:<8}"), key),
        Span::styled(format!("{d1:<30}"), Style::default().fg(theme.text)),
        Span::styled(format!("{k2:<8}"), key),
        Span::raw(d2.to_string()),
    ])
}

/// Dialog accent: maps onto the semantic palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    Info,
    Confirm,
    Danger,
    Input,
    Progress,
    Error,
}

fn kind_color(kind: DialogKind, theme: &Theme) -> ratatui::style::Color {
    match kind {
        DialogKind::Info => theme.accent,
        DialogKind::Confirm => theme.warning,
        DialogKind::Danger => theme.error,
        DialogKind::Input => theme.border_active,
        DialogKind::Progress => theme.warning,
        DialogKind::Error => theme.error,
    }
}

/// Chrome around dialog content: 1-cell rounded border + 1-cell frost ring.
pub const DIALOG_CHROME: u16 = 4;

/// Centered dialog rect that fits `content_w` × `content_h` plus chrome
/// (border + frost), leaving one cell for the drop shadow.
pub fn dialog_area(content_w: u16, content_h: u16, frame: Rect) -> Rect {
    let w = content_w
        .saturating_add(DIALOG_CHROME)
        .min(frame.width.saturating_sub(2).max(1));
    let h = content_h
        .saturating_add(DIALOG_CHROME)
        .min(frame.height.saturating_sub(2).max(1));
    centered_rect(w, h, frame)
}

/// Glass dialog: dim the backdrop, drop a 1-cell shadow, keep a frost
/// ring of muted underlying cells, paint a tinted surface, rounded border.
pub fn render_dialog(
    frame: &mut Frame,
    title: &str,
    lines: Vec<Line<'_>>,
    kind: DialogKind,
    theme: &Theme,
    area: Rect,
) {
    dim_backdrop(frame, theme);
    paint_shadow(frame, area, theme);

    let accent = kind_color(kind, theme);
    let border_style = Style::default().fg(accent).bg(theme.surface);
    draw_rounded_border(frame, area, title, border_style, theme);

    let inner = inset(area, 1);
    restyle_frost_ring(frame, inner, theme);

    let content = inset(inner, 1);
    paint_bg(
        frame,
        content,
        Style::default().fg(theme.text).bg(theme.surface),
    );
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .style(Style::default().fg(theme.text).bg(theme.surface)),
        content,
    );
}

/// In-place text editor line with a block cursor.
pub fn text_input_line(chars: &[char], cursor: usize, theme: &Theme) -> Line<'static> {
    let cursor_style = Style::default().fg(theme.cursor_fg).bg(theme.cursor_bg);
    let text_style = Style::default().fg(theme.text);
    let mut spans: Vec<Span<'static>> = Vec::new();
    if chars.is_empty() {
        spans.push(Span::raw(" ").style(cursor_style));
        return Line::from(spans);
    }
    for (i, c) in chars.iter().enumerate() {
        let s = Span::styled(c.to_string(), text_style);
        spans.push(if i == cursor {
            s.style(cursor_style)
        } else {
            s
        });
    }
    if cursor >= chars.len() {
        spans.push(Span::raw(" ").style(cursor_style));
    }
    Line::from(spans)
}

/// Paints a solid background (blank cells, `style`) across `area`.
pub fn paint_bg(frame: &mut Frame, area: Rect, style: Style) {
    frame.render_widget(Clear, area);
    frame.buffer_mut().set_style(area, style);
}

/// Returns a `width`×`height` rect centered within `area`.
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

fn inset(area: Rect, by: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(by),
        y: area.y.saturating_add(by),
        width: area.width.saturating_sub(by.saturating_mul(2)),
        height: area.height.saturating_sub(by.saturating_mul(2)),
    }
}

fn dim_backdrop(frame: &mut Frame, theme: &Theme) {
    let area = frame.area();
    let buf = frame.buffer_mut();
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            let cell = &mut buf[(x, y)];
            cell.set_style(
                cell.style()
                    .add_modifier(Modifier::DIM)
                    .fg(theme.text_muted),
            );
        }
    }
}

fn paint_shadow(frame: &mut Frame, dialog: Rect, theme: &Theme) {
    let area = frame.area();
    let buf = frame.buffer_mut();
    let bottom = dialog.y.saturating_add(dialog.height);
    let right = dialog.x.saturating_add(dialog.width);
    if bottom < area.y.saturating_add(area.height) {
        let start = dialog.x.saturating_add(1);
        let end = start.saturating_add(dialog.width).min(area.x + area.width);
        for x in start..end {
            buf[(x, bottom)].set_bg(theme.shadow);
        }
    }
    if right < area.x.saturating_add(area.width) {
        let start = dialog.y.saturating_add(1);
        let end = start
            .saturating_add(dialog.height)
            .min(area.y + area.height);
        for y in start..end {
            buf[(right, y)].set_bg(theme.shadow);
        }
    }
}

fn restyle_frost_ring(frame: &mut Frame, inner: Rect, theme: &Theme) {
    if inner.width < 2 || inner.height < 2 {
        return;
    }
    let buf = frame.buffer_mut();
    let frost = Style::default().fg(theme.text_muted).bg(theme.surface);
    for y in inner.y..inner.y.saturating_add(inner.height) {
        for x in inner.x..inner.x.saturating_add(inner.width) {
            let on_ring = x == inner.x
                || x == inner.x + inner.width - 1
                || y == inner.y
                || y == inner.y + inner.height - 1;
            if on_ring {
                buf[(x, y)].set_style(frost);
            }
        }
    }
}

fn draw_rounded_border(frame: &mut Frame, area: Rect, title: &str, style: Style, theme: &Theme) {
    if area.width < 2 || area.height < 2 {
        return;
    }
    let buf = frame.buffer_mut();
    let x0 = area.x;
    let y0 = area.y;
    let x1 = area.x + area.width - 1;
    let y1 = area.y + area.height - 1;
    buf[(x0, y0)].set_symbol("╭").set_style(style);
    buf[(x1, y0)].set_symbol("╮").set_style(style);
    buf[(x0, y1)].set_symbol("╰").set_style(style);
    buf[(x1, y1)].set_symbol("╯").set_style(style);
    for x in (x0 + 1)..x1 {
        buf[(x, y0)].set_symbol("─").set_style(style);
        buf[(x, y1)].set_symbol("─").set_style(style);
    }
    for y in (y0 + 1)..y1 {
        buf[(x0, y)].set_symbol("│").set_style(style);
        buf[(x1, y)].set_symbol("│").set_style(style);
    }
    // Title sits on the top border, padded, clipped to the inner width.
    let label = format!(" {title} ");
    let max = area.width.saturating_sub(2) as usize;
    let shown: String = label.chars().take(max).collect();
    let start = x0 + 1;
    for (i, ch) in shown.chars().enumerate() {
        let x = start + i as u16;
        if x >= x1 {
            break;
        }
        buf[(x, y0)].set_symbol(&ch.to_string()).set_style(
            style
                .add_modifier(Modifier::BOLD)
                .fg(style.fg.unwrap_or(theme.accent)),
        );
    }
}
