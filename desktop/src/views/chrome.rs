use gpui::{
    ClickEvent, Div, ElementId, InteractiveElement, IntoElement, ParentElement, SharedString,
    Stateful, StatefulInteractiveElement, Styled, Window, div, px,
};
use ira_core::theme::{ChipStyle, Theme};
use std::time::Instant;

pub fn to_gpui_color(color: ira_core::theme::Color) -> gpui::Rgba {
    crate::views::native_color(color)
}

/// Renders a key pill matching the TUI chip style (Square, Rounded, Outline).
pub fn key_pill(key: &str, theme: &Theme) -> Div {
    let key_fg = to_gpui_color(theme.key_fg);
    let key_bg = to_gpui_color(theme.key_bg);
    let accent = to_gpui_color(theme.accent);
    let border_active = to_gpui_color(theme.border_active);

    match theme.chips {
        ChipStyle::Square => div()
            .flex()
            .items_center()
            .justify_center()
            .px(px(5.))
            .py(px(1.))
            .rounded(px(3.))
            .bg(key_bg)
            .text_color(key_fg)
            .text_size(px(11.))
            .font_weight(gpui::FontWeight::BOLD)
            .child(key.to_string()),
        ChipStyle::Rounded => div()
            .flex()
            .items_center()
            .justify_center()
            .px(px(7.))
            .py(px(1.))
            .rounded(px(10.))
            .bg(key_bg)
            .text_color(key_fg)
            .text_size(px(11.))
            .font_weight(gpui::FontWeight::BOLD)
            .child(key.to_string()),
        ChipStyle::Outline => div()
            .flex()
            .items_center()
            .justify_center()
            .px(px(5.))
            .py(px(1.))
            .rounded(px(4.))
            .border_1()
            .border_color(border_active)
            .text_color(accent)
            .text_size(px(11.))
            .font_weight(gpui::FontWeight::BOLD)
            .child(key.to_string()),
    }
}

/// Interactive clickable key hint chip `[pill] label`.
pub fn key_hint(
    id: impl Into<ElementId>,
    key: &str,
    label: &str,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> Stateful<Div> {
    let text_color = to_gpui_color(theme.text);
    let hover_bg = to_gpui_color(theme.selection);
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(4.))
        .px(px(5.))
        .py(px(2.))
        .rounded(px(4.))
        .cursor_pointer()
        .hover(|s| s.bg(hover_bg))
        .child(key_pill(key, theme))
        .child(
            div()
                .text_size(px(12.))
                .text_color(text_color)
                .child(label.to_string()),
        )
        .on_click(on_click)
}

/// Renders a bordered panel block matching the TUI's rounded block styling.
pub fn panel_block(
    title: impl IntoElement,
    right_title: Option<impl IntoElement>,
    active: bool,
    theme: &Theme,
) -> Div {
    let border_color = if active {
        to_gpui_color(theme.border_active)
    } else {
        to_gpui_color(theme.border)
    };
    let bg_color = to_gpui_color(theme.surface);

    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .px(px(8.))
        .py(px(3.))
        .child(title)
        .children(right_title.map(|r| div().child(r)));

    div()
        .flex()
        .flex_col()
        .rounded(px(4.))
        .border_1()
        .border_color(border_color)
        .bg(bg_color)
        .child(header)
}

/// Braille spinner character for background operations (size walk, deletion).
pub fn spinner_char(started: Instant) -> &'static str {
    const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let idx = (started.elapsed().as_millis() / 80) as usize % FRAMES.len();
    FRAMES[idx]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    Info,
    Confirm,
    Danger,
    Progress,
    Input,
    Error,
}

pub fn dialog_kind_color(kind: DialogKind, theme: &Theme) -> gpui::Rgba {
    match kind {
        DialogKind::Info | DialogKind::Progress => to_gpui_color(theme.info),
        DialogKind::Confirm | DialogKind::Input => to_gpui_color(theme.accent),
        DialogKind::Danger | DialogKind::Error => to_gpui_color(theme.error),
    }
}

pub fn dialog_header(
    title: impl Into<SharedString>,
    kind: DialogKind,
    theme: &Theme,
) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .pb(px(6.))
        .border_b_1()
        .border_color(to_gpui_color(theme.border))
        .child(
            div()
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(dialog_kind_color(kind, theme))
                .child(title.into()),
        )
}

pub fn hint_pill(key: &str, label: &str, theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .child(key_pill(key, theme))
        .child(
            div()
                .text_size(px(11.))
                .text_color(to_gpui_color(theme.text_muted))
                .child(label.to_string()),
        )
}

pub fn hint_row(pairs: &[(&str, &str)], theme: &Theme) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(10.))
        .children(pairs.iter().map(|(k, v)| hint_pill(k, v, theme)))
}

pub fn progress_fraction_width(fraction: f32, width_px: f32) -> f32 {
    (fraction.clamp(0.0, 1.0) * width_px.max(0.0)).round()
}

pub fn progress_bar(
    fraction: f32,
    width_px: f32,
    fill_color: gpui::Rgba,
    track_color: gpui::Rgba,
) -> Div {
    div()
        .w(px(width_px))
        .h(px(6.))
        .rounded(px(3.))
        .bg(track_color)
        .overflow_hidden()
        .child(
            div()
                .w(px(progress_fraction_width(fraction, width_px)))
                .h_full()
                .bg(fill_color),
        )
}

pub fn badge(label: impl Into<SharedString>, fg: gpui::Rgba, bg: gpui::Rgba) -> Div {
    div()
        .px(px(5.))
        .py(px(1.))
        .rounded(px(3.))
        .bg(bg)
        .text_color(fg)
        .text_size(px(10.))
        .font_weight(gpui::FontWeight::BOLD)
        .child(label.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn dialog_kind_color_maps_semantic_theme_tokens() {
        let theme = Theme::default();
        assert_eq!(
            dialog_kind_color(DialogKind::Info, &theme),
            to_gpui_color(theme.info)
        );
        assert_eq!(
            dialog_kind_color(DialogKind::Progress, &theme),
            to_gpui_color(theme.info)
        );
        assert_eq!(
            dialog_kind_color(DialogKind::Confirm, &theme),
            to_gpui_color(theme.accent)
        );
        assert_eq!(
            dialog_kind_color(DialogKind::Input, &theme),
            to_gpui_color(theme.accent)
        );
        assert_eq!(
            dialog_kind_color(DialogKind::Danger, &theme),
            to_gpui_color(theme.error)
        );
        assert_eq!(
            dialog_kind_color(DialogKind::Error, &theme),
            to_gpui_color(theme.error)
        );
    }

    #[test]
    fn spinner_char_returns_braille_frames() {
        let now = Instant::now();
        assert_eq!(spinner_char(now), "⠋");
        if let Some(earlier) = now.checked_sub(Duration::from_millis(85)) {
            assert_eq!(spinner_char(earlier), "⠙");
        }
    }

    #[test]
    fn progress_fraction_width_clamps_and_rounds() {
        assert_eq!(progress_fraction_width(-0.5, 200.0), 0.0);
        assert_eq!(progress_fraction_width(0.0, 200.0), 0.0);
        assert_eq!(progress_fraction_width(0.25, 200.0), 50.0);
        assert_eq!(progress_fraction_width(0.333, 100.0), 33.0);
        assert_eq!(progress_fraction_width(1.0, 200.0), 200.0);
        assert_eq!(progress_fraction_width(1.5, 200.0), 200.0);
        assert_eq!(progress_fraction_width(0.5, -50.0), 0.0);
    }
}
