use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Paragraph},
    Frame,
};

use crate::app::App;

/// Width of the preview column in terminal cells (upper bound; the column
/// yields space to the panes on narrow terminals).
pub const PREVIEW_COLS: u16 = 40;

/// Renders one pane's preview column: that pane's selected entry as a
/// terminal image (protocol chosen at startup by `Picker`), or a contextual
/// placeholder while a background decode runs / when nothing is previewable.
/// Also prefetches the next images in the pane's visible order so scrolling
/// shows already-decoded thumbnails.
pub fn render(frame: &mut Frame, app: &mut App, area: Rect, pane_index: usize) {
    // Editing takes over the column while this pane's editor is focused.
    let editing = app.edit_focus
        && app
            .edit
            .as_ref()
            .is_some_and(|e| e.pane_index == pane_index && !e.read_only);
    let editing_ro = app
        .edit
        .as_ref()
        .is_some_and(|e| e.pane_index == pane_index && e.read_only);
    let title = if editing {
        let dirty = app.edit.as_ref().is_some_and(|e| e.dirty);
        if dirty {
            " * Editing — s save · Esc exit ".to_string()
        } else {
            " Editing — s save · Esc exit ".to_string()
        }
    } else if editing_ro {
        " Preview (read-only) ".to_string()
    } else {
        " Preview (v) ".to_string()
    };
    let block = Block::bordered().title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if editing {
        let edit = app.edit.as_ref().expect("checked above");
        frame.render_widget(edit.textarea.widget(), inner);
        return;
    }
    if editing_ro {
        frame.render_widget(
            Paragraph::new(Line::raw(" read-only file "))
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }

    // Report the drawable area back so the app requests protocols that fit
    // exactly (protocol encoding is area-dependent).
    app.panes[pane_index].preview_area = (inner.width, inner.height);

    let suspended = app.overlay_covers_preview();
    if suspended {
        // A modal dialog centered over the files area cannot hide a
        // graphics-protocol image (it lives above the cell grid), so
        // suspend the preview while any overlay is open.
        frame.render_widget(
            Paragraph::new(Line::raw(" preview paused "))
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }

    // Text files render natively as cells — no protocol, no thumbnail.
    let is_text = app.selected_visible_entry_for(pane_index).is_some_and(|e| {
        !e.is_dir
            && crate::services::thumbnails::preview_kind(&e.path)
                == Some(crate::services::thumbnails::PreviewKind::Text)
    });
    if is_text {
        match app.text_preview(pane_index) {
            Some(preview) if preview.binary => frame.render_widget(
                Paragraph::new(Line::raw(" binary file "))
                    .style(Style::default().fg(Color::DarkGray)),
                inner,
            ),
            Some(preview) if preview.content.is_empty() => frame.render_widget(
                Paragraph::new(Line::raw(" (empty file) "))
                    .style(Style::default().fg(Color::DarkGray)),
                inner,
            ),
            Some(preview) => {
                let mut lines: Vec<Line> = preview
                    .content
                    .lines()
                    .map(|l| Line::raw(l.replace('\t', "    ")))
                    .collect();
                if preview.truncated {
                    lines.push(
                        Line::raw(" … truncated").style(Style::default().fg(Color::DarkGray)),
                    );
                }
                lines.truncate(inner.height as usize);
                frame.render_widget(Paragraph::new(lines), inner);
            }
            None => frame.render_widget(
                Paragraph::new(Line::raw(" loading… ")).style(Style::default().fg(Color::DarkGray)),
                inner,
            ),
        }
        return;
    }

    match app.preview_request_for(pane_index) {
        Some(req) => {
            if let Some(protocol) = app.preview_protocol(&req) {
                frame.render_widget(ratatui_image::Image::new(protocol), inner);
            } else {
                // First sight dispatched a background job; the thumbnail
                // appears on a later frame. Never block the render thread.
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
        }
        None => {
            let reason = match app.selected_visible_entry_for(pane_index) {
                None => " select a file ",
                Some(entry) if entry.is_dir => " folders have no image preview ",
                Some(entry) => match crate::services::thumbnails::preview_kind(&entry.path) {
                    Some(crate::services::thumbnails::PreviewKind::Video) => {
                        " install ffmpeg for video previews "
                    }
                    Some(crate::services::thumbnails::PreviewKind::Heic) => {
                        " install ffmpeg for HEIC previews "
                    }
                    Some(crate::services::thumbnails::PreviewKind::Pdf) => {
                        " install poppler (pdftoppm) for PDF previews "
                    }
                    _ => " format not supported (png/jpg/gif/bmp/webp/mp4/mov/heic/pdf) ",
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

    // Prefetch the next images in visible order ("instant" scrolling). A
    // no-op while an overlay is open.
    app.prefetch_column(pane_index, 8);
}
