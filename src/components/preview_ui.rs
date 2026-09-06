use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Paragraph},
    Frame,
};

use crate::app::App;

/// Width of the preview column in terminal cells.
pub const PREVIEW_COLS: u16 = 40;

/// Renders the image preview column: the active pane's selected entry as a
/// terminal image (protocol chosen at startup by `Picker`), or a contextual
/// placeholder while a background decode runs / when nothing is previewable.
pub fn render(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::bordered().title(" Preview (v) ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Report the drawable area back so the app requests protocols that fit
    // exactly (protocol encoding is area-dependent).
    app.preview_area = (inner.width, inner.height);

    // A modal dialog centered over the files area cannot hide a
    // graphics-protocol image (it lives above the cell grid), so suspend
    // the preview while any overlay is open.
    if app.overlay_covers_preview() {
        frame.render_widget(
            Paragraph::new(Line::raw(" preview paused "))
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }
    match app.preview_request() {
        Some(req) => {
            if let Some(protocol) = app.preview_protocol(&req) {
                frame.render_widget(ratatui_image::Image::new(protocol), inner);
                return;
            }
            // First sight dispatched a background job; the thumbnail appears
            // on a later frame. Never block the render thread here.
            let label = std::path::Path::new(&req.path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            frame.render_widget(
                Paragraph::new(Line::raw(format!(" loading {label}… ")))
                    .style(Style::default().fg(Color::DarkGray)),
                inner,
            );
        }
        None => {
            let reason = match app.selected_visible_entry() {
                None => " select a file ",
                Some(entry) if entry.is_dir => " folders have no image preview ",
                Some(entry) => match crate::services::thumbnails::preview_kind(&entry.path) {
                    Some(crate::services::thumbnails::PreviewKind::Video) => {
                        " install ffmpeg for video previews "
                    }
                    Some(crate::services::thumbnails::PreviewKind::Heic) => {
                        " install ffmpeg for HEIC previews "
                    }
                    _ => " format not supported (png/jpg/gif/bmp/webp/mp4/mov/heic) ",
                },
            };
            frame.render_widget(
                Paragraph::new(Line::raw(reason)).style(
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ),
                inner,
            );
        }
    }
}
