use std::time::Instant;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListState, Paragraph},
    Frame,
};

use crate::app::App;
use crate::services::file_info::{human, list_note, spinner_char, SizeInfo};
use crate::theme::icons::{icon_cols, icon_for, pad_icon, IconSet};
use crate::theme::Theme;
use crate::ui::chrome::panel_block;
/// Renders one file-browser pane. When `active`, the pane shows its cursor and
/// any active search filter; otherwise it renders dimmed with no cursor.
/// Dispatches to the pane's preview mode: grid, list + own preview column,
/// the details list, or the plain list. Modes are per pane — switching panes
/// with Tab never touches either pane's mode.
pub fn render(f: &mut Frame, app: &mut App, area: Rect, pane_index: usize, active: bool) {
    match app.panes[pane_index].preview_mode {
        crate::app::PreviewMode::Grid => {
            render_grid(f, app, area, pane_index, active);
        }
        crate::app::PreviewMode::Column => {
            let with_preview = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(24),
                    Constraint::Max(crate::components::preview_ui::PREVIEW_COLS),
                ])
                .split(area);
            render_list(f, app, with_preview[0], pane_index, active);
            crate::components::preview_ui::render(f, app, with_preview[1], pane_index);
        }
        crate::app::PreviewMode::Details => {
            render_details(f, app, area, pane_index, active);
        }
        crate::app::PreviewMode::Off => {
            render_list(f, app, area, pane_index, active);
        }
    }
}

fn render_list(f: &mut Frame, app: &mut App, area: Rect, pane_index: usize, active: bool) {
    let height = area.height.saturating_sub(2) as usize;
    let theme = app.theme;
    let icons = app.icons;

    let (title, file_spans, window_start, cursor) = {
        let pane = &app.panes[pane_index];
        let Some(folder) = &pane.folder else {
            f.render_widget(
                Paragraph::new("").block(panel_block(Line::raw("  "), active, &theme)),
                area,
            );
            return;
        };

        let title = pane_title(
            &folder.label,
            &folder.path,
            if active {
                app.search_query.as_deref().or(pane.filter_query.as_deref())
            } else {
                None
            },
            pane.filter_query.is_some() && active && app.search_query.is_none(),
            None,
            &theme,
        );

        // Folders being measured show an animated spinner as their icon;
        // folders with a completed measurement carry a size annotation
        // after their name.
        let walk_started = |f: &crate::services::list_files::FEntry| {
            app.size_walk_started(&f.path).filter(|_| f.is_dir)
        };
        let size_note = |f: &crate::services::list_files::FEntry| {
            app.size_info(&f.path).filter(|s| s.complete && f.is_dir)
        };
        // Folders queued for background deletion show the spinner too.
        let deleting_started = |f: &crate::services::list_files::FEntry| {
            app.deleting_started(&f.path).filter(|_| f.is_dir)
        };

        // Build spans ONLY for the visible window: with 200k+ entries, one
        // Span allocation per row per frame is what makes the UI feel stuck.
        // `pane.render_scroll` (kept by the integrator on `Pane`) pins the
        // window across frames; the helper keeps the cursor inside it.
        // While a listing is still streaming in (first frames after
        // entering a folder), show a Loading hint instead of a partial
        // unsorted view.
        let selected = pane.state.selected();
        let loading = !pane.listing_settled && pane.files.is_empty();
        let filtered = pane.filter_query.is_some() && !pane.filter_indices.is_empty();
        let (start, file_spans) = if loading {
            (
                0,
                vec![Line::styled(
                    " Loading…",
                    Style::default().fg(theme.text_muted),
                )],
            )
        } else if filtered || (active && app.is_searching()) {
            let rows = app.pane_visible_rows(pane_index);
            let (start, end) = visible_window(rows.len(), selected, pane.render_scroll, height);
            let spans: Vec<Line> = rows[start..end]
                .iter()
                .map(|(i, f)| {
                    row_span(
                        pane.selected.get(*i).copied().unwrap_or(false),
                        f,
                        walk_started(f),
                        size_note(f),
                        deleting_started(f),
                        &theme,
                        icons,
                    )
                })
                .collect();
            (start, spans)
        } else {
            let (start, end) =
                visible_window(pane.files.len(), selected, pane.render_scroll, height);
            let spans: Vec<Line> = pane.files[start..end]
                .iter()
                .enumerate()
                .map(|(rel, f)| {
                    row_span(
                        pane.selected.get(start + rel).copied().unwrap_or(false),
                        f,
                        walk_started(f),
                        size_note(f),
                        deleting_started(f),
                        &theme,
                        icons,
                    )
                })
                .collect();
            (start, spans)
        };

        (title, file_spans, start, selected.map(|s| s - start))
    };

    let list = List::new(file_spans)
        .block(panel_block(title, active, &theme))
        .highlight_style(Style::new().fg(theme.cursor_fg).bg(theme.cursor_bg))
        .highlight_symbol("▌")
        .repeat_highlight_symbol(true);

    // Fresh ListState each frame: the window offset is ours (`render_scroll`),
    // and `cursor` is already relative to the rendered slice.
    let mut render_state = ListState::default();
    if active {
        render_state.select(cursor);
        let pane = &mut app.panes[pane_index];
        pane.render_scroll = window_start;
        f.render_stateful_widget(list, area, &mut render_state);
    } else {
        f.render_stateful_widget(list, area, &mut render_state);
    }
}
/// Right-hand column widths of the details list: size ("1023.9 MiB") and
/// relative modification time ("3.8 years ago"), two spaces between
/// columns.
const DETAIL_SIZE_W: usize = 10;
const DETAIL_AGO_W: usize = 14;

/// Renders the file list in details mode: the same rows as the plain list,
/// plus right-aligned size and relative modification columns. Folders
/// show no size — recursive measurement is deliberately NOT triggered in
/// this mode (its only purpose is a quick overview of a folder's files);
/// sizes for files come from the already-stat'd entry.
fn render_details(f: &mut Frame, app: &mut App, area: Rect, pane_index: usize, active: bool) {
    let theme = app.theme;
    let icons = app.icons;
    let width = area.width.saturating_sub(2) as usize;
    let height = area.height.saturating_sub(2) as usize;
    // Row budget inside the bordered pane: the `▌` highlight symbol is
    // repeated on EVERY row (`repeat_highlight_symbol`), so it consumes 1
    // column, then " [*] <icon> " is 6 + icon_cols, then two separators
    // between the three columns. Under-counting this budget clips the
    // last column (the "1403 days ag" bug).
    let prefix = 6 + icon_cols(icons);
    let name_w = width
        .saturating_sub(1 + prefix + DETAIL_SIZE_W + DETAIL_AGO_W + 4)
        .max(8);

    let (title, file_spans, window_start, cursor) = {
        let pane = &app.panes[pane_index];
        let Some(folder) = &pane.folder else {
            f.render_widget(
                Paragraph::new("").block(panel_block(Line::raw("  "), active, &theme)),
                area,
            );
            return;
        };
        let title = pane_title(
            &folder.label,
            &folder.path,
            None,
            false,
            Some("details"),
            &theme,
        );

        let selected = pane.state.selected();
        let loading = !pane.listing_settled && pane.files.is_empty();
        let filtered = pane.filter_query.is_some() && !pane.filter_indices.is_empty();
        let (start, file_spans) = if loading {
            (
                0,
                vec![Line::styled(
                    " Loading…",
                    Style::default().fg(theme.text_muted),
                )],
            )
        } else if filtered || (active && app.is_searching()) {
            let rows = app.pane_visible_rows(pane_index);
            let (start, end) = visible_window(rows.len(), selected, pane.render_scroll, height);
            let spans: Vec<Line> = rows[start..end]
                .iter()
                .map(|(i, entry)| {
                    detail_row(
                        pane.selected.get(*i).copied().unwrap_or(false),
                        entry,
                        name_w,
                        app.deleting_started(&entry.path),
                        &theme,
                        icons,
                    )
                })
                .collect();
            (start, spans)
        } else {
            let (start, end) =
                visible_window(pane.files.len(), selected, pane.render_scroll, height);
            let spans: Vec<Line> = pane.files[start..end]
                .iter()
                .enumerate()
                .map(|(rel, entry)| {
                    detail_row(
                        pane.selected.get(start + rel).copied().unwrap_or(false),
                        entry,
                        name_w,
                        app.deleting_started(&entry.path),
                        &theme,
                        icons,
                    )
                })
                .collect();
            (start, spans)
        };
        (title, file_spans, start, selected.map(|s| s - start))
    };

    let list = List::new(file_spans)
        .block(panel_block(title, active, &theme))
        .highlight_style(Style::new().fg(theme.cursor_fg).bg(theme.cursor_bg))
        .highlight_symbol("▌")
        .repeat_highlight_symbol(true);

    let mut render_state = ListState::default();
    if active {
        render_state.select(cursor);
        let pane = &mut app.panes[pane_index];
        pane.render_scroll = window_start;
        f.render_stateful_widget(list, area, &mut render_state);
    } else {
        f.render_stateful_widget(list, area, &mut render_state);
    }
}

/// One details row: `[*] icon name…  <size>  <3 days ago>`. Folders never
/// get a size (`—`): their bytes would need a recursive walk, which this
/// mode deliberately does not start.
fn detail_row(
    selected: bool,
    entry: &crate::services::list_files::FEntry,
    name_w: usize,
    deleting: Option<Instant>,
    theme: &Theme,
    icons: IconSet,
) -> Line<'static> {
    let mark = if selected { "[*]" } else { "[ ]" };
    let (glyph, cat) = icon_for(entry, icons);
    let icon = match deleting {
        Some(started) => pad_icon(&spinner_char(started).to_string(), icons),
        None => pad_icon(glyph, icons),
    };
    let size = if entry.is_dir {
        "—".to_string()
    } else {
        human(entry.size)
    };
    let name = truncate_chars(&entry.label, name_w);
    let mark_style = if selected {
        Style::default().fg(theme.selection)
    } else {
        Style::default().fg(theme.text_muted)
    };
    let name_style = name_style_for(entry, theme);
    let muted = Style::default().fg(theme.text_muted);
    Line::from(vec![
        Span::raw(" "),
        Span::styled(mark, mark_style),
        Span::raw(" "),
        Span::styled(icon, Style::default().fg(theme.color_for(cat))),
        Span::raw(" "),
        Span::styled(format!("{name:<name_w$}"), name_style),
        Span::raw("  "),
        Span::styled(format!("{size:>DETAIL_SIZE_W$}"), muted),
        Span::raw("  "),
        Span::styled(
            format!("{:>DETAIL_AGO_W$}", modified_ago(entry.modified)),
            muted,
        ),
    ])
}

/// Char-based truncation with an ellipsis (labels are treated as single-width
/// throughout the list renderer; multi-column runes degrade, not misalign).
fn truncate_chars(s: &str, max: usize) -> String {
    let mut n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    n = out.chars().count();
    while n < max.saturating_sub(1) {
        out.push(' ');
        n += 1;
    }
    out.push('…');
    out
}

/// Human relative modification time: "3 days ago". `None` (stat failed)
/// renders as `—`; timestamps in the future (clock skew) as "just now".
fn modified_ago(modified: Option<i64>) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    let Some(epoch) = modified else {
        return "—".into();
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let ago = (now - epoch).max(0);
    let unit = |n: i64, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one} ago")
        } else {
            format!("{n} {many} ago")
        }
    };
    if ago < MINUTE {
        "just now".to_string()
    } else if ago < HOUR {
        unit(ago / MINUTE, "minute", "minutes")
    } else if ago < DAY {
        unit(ago / HOUR, "hour", "hours")
    } else {
        let days = ago / DAY;
        if days < 365 {
            unit(days, "day", "days")
        } else {
            // Past a year the day count stops being useful ("1403 days
            // ago"); one decimal in years keeps it compact ("3.8 years
            // ago"), integers from ten up, singular at exactly one.
            let years = (days as f64 / 365.25 * 10.0).round() / 10.0;
            if years >= 10.0 {
                format!("{years:.0} years ago")
            } else if years == 1.0 {
                "1 year ago".to_string()
            } else {
                format!("{years:.1} years ago")
            }
        }
    }
}

/// Grid-cell geometry: image area on top, one name line below.
///
/// Sized for recognizability, not density: at 7×17 px cells (JetBrainsMono
/// 9pt) a cell is a 140×136 px thumbnail, which keeps real photographs
/// recognizable. The previous 14×4 cell produced 98×68 px images — simple
/// shapes survived, but photos degraded to unrecognizable mush (and on
/// braille-block terminals like Terminal.app, 8 px per cell turns even
/// the larger cell into just 40×32 px).
const GRID_CELL_W: u16 = 20;
const GRID_IMG_H: u16 = 8;
const GRID_NAME_H: u16 = 1;

/// Thumbnail grid for one pane: every visible image entry is rendered as a
/// thumbnail (dispatched through the same cache/pool as the preview column),
/// folders and unsupported files as centered glyphs. Visible index space is
/// the pane's rendered rows (live search / filter / full list), matching
/// `state.selected()`.
fn render_grid(f: &mut Frame, app: &mut App, area: Rect, pane_index: usize, active: bool) {
    let theme = app.theme;
    let icons = app.icons;
    let inner_w = area.width.saturating_sub(2) as usize;
    let inner_h = area.height.saturating_sub(2) as usize;
    let cols = (inner_w / GRID_CELL_W as usize).max(1);
    let grid_rows = (inner_h / (GRID_IMG_H + GRID_NAME_H) as usize).max(1);
    let per_screen = cols * grid_rows;

    let (title, top, window, prefetch) = {
        let pane = &app.panes[pane_index];
        let Some(folder) = &pane.folder else {
            f.render_widget(
                Paragraph::new("").block(panel_block(Line::raw("  "), active, &theme)),
                area,
            );
            return;
        };
        let title = pane_title(&folder.label, &folder.path, None, false, None, &theme);
        let rows = app.pane_visible_rows(pane_index);
        let total = rows.len();
        let selected = pane.state.selected();
        let top = grid_window(total, selected, pane.grid_top, per_screen);
        let end = (top + per_screen).min(total);
        let window: Vec<(usize, crate::services::list_files::FEntry)> = rows[top..end]
            .iter()
            .map(|(i, e)| (*i, (*e).clone()))
            .collect();
        // Prefetch one screen above and below the visible window so
        // scrolling shows already-decoded thumbnails. Sliced, not filtered:
        // O(window) per frame, not O(folder).
        let pf_from = top.saturating_sub(per_screen);
        let pf_to = (end + per_screen).min(total);
        let prefetch: Vec<crate::services::list_files::FEntry> = rows[pf_from..top]
            .iter()
            .chain(rows[end..pf_to].iter())
            .map(|(_, e)| (*e).clone())
            .collect();
        (title, top, window, prefetch)
    };
    app.panes[pane_index].grid_top = top;

    let block = panel_block(title, active, &theme);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let selected = app.panes[pane_index].state.selected();
    let mut overlay_tiles: Vec<(Rect, crate::services::thumbnails::ThumbRequest)> = Vec::new();
    for (k, (file_idx, entry)) in window.iter().enumerate() {
        let col = (k % cols) as u16;
        let row = (k / cols) as u16;
        let cell_x = inner.x + col * GRID_CELL_W;
        let cell_y = inner.y + row * (GRID_IMG_H + GRID_NAME_H);
        let img_area = Rect {
            x: cell_x,
            y: cell_y,
            width: GRID_CELL_W,
            height: GRID_IMG_H,
        };
        // One column of padding on each side so neighboring names don't
        // read as one word; the thumbnail above stays full-bleed.
        let name_area = Rect {
            x: cell_x + 1,
            y: cell_y + GRID_IMG_H,
            width: GRID_CELL_W.saturating_sub(2),
            height: GRID_NAME_H,
        };
        let is_selected = active && selected == Some(top + k);
        let dim = Style::default().fg(theme.text_muted);
        let (glyph, cat) = icon_for(entry, icons);
        let glyph_style = Style::default().fg(theme.color_for(cat));

        // Image area: thumbnail, or a kind glyph for folders/unsupported.
        // Text files stay glyphs in the grid (cells can't show text) — the
        // column mode renders them natively.
        if !entry.is_dir
            && crate::services::thumbnails::preview_kind(&entry.path)
                == Some(crate::services::thumbnails::PreviewKind::Text)
        {
            f.render_widget(
                Paragraph::new(Span::styled(glyph, glyph_style)).centered(),
                img_area,
            );
        } else if !entry.is_dir && app.preview_supported(&entry.path) {
            let req = crate::services::thumbnails::ThumbRequest {
                path: entry.path.clone(),
                mtime: entry.modified,
                size: entry.size,
                cols: GRID_CELL_W,
                rows: GRID_IMG_H,
            };
            match app.preview_image(&req) {
                Some(rendered) => {
                    rendered.render(img_area, f.buffer_mut());
                    overlay_tiles.push((img_area, req));
                }
                None => f.render_widget(Paragraph::new(Span::raw(" …").style(dim)), img_area),
            }
        } else {
            f.render_widget(
                Paragraph::new(Span::styled(glyph, glyph_style)).centered(),
                img_area,
            );
        }

        // Name line: cursor colors for the selection; `*` marks multi-select.
        let marker = if app.panes[pane_index]
            .selected
            .get(*file_idx)
            .copied()
            .unwrap_or(false)
        {
            "*"
        } else {
            ""
        };
        let name_style = if is_selected {
            Style::default().fg(theme.cursor_fg).bg(theme.cursor_bg)
        } else if !active {
            dim
        } else {
            name_style_for(entry, &theme)
        };
        f.render_widget(
            Paragraph::new(Span::styled(format!("{marker}{}", entry.label), name_style)),
            name_area,
        );
    }

    if !overlay_tiles.is_empty() {
        app.set_overlay_job(crate::app::OverlayJob::Grid {
            area: inner,
            tiles: overlay_tiles,
        });
    }

    // Prefetch the screens adjacent to the viewport (no-ops for cached
    // entries; the job queue caps how much this can enqueue per frame).
    app.prefetch_grid_cells(pane_index, &prefetch, GRID_CELL_W, GRID_IMG_H);
}

/// Computes the first visible index of a grid window of `per_screen` cells,
/// keeping the cursor inside, seeded from the previous window start.
fn grid_window(total: usize, selected: Option<usize>, scroll: usize, per_screen: usize) -> usize {
    if total == 0 || per_screen == 0 {
        return 0;
    }
    let max_top = total.saturating_sub(per_screen);
    let mut top = scroll.min(max_top);
    if let Some(sel) = selected {
        let sel = sel.min(total - 1);
        if sel < top {
            top = sel;
        } else if sel >= top + per_screen {
            top = sel + 1 - per_screen;
        }
    }
    top
}

/// Computes the `(start, end)` row window to render for a list of `total`
/// rows, keeping the cursor row (`selected`) inside a viewport of `height`
/// rows, seeded from the previous window start (`scroll`). Clamps `end` to
/// `total` and a stale cursor to `total - 1`. Returns `(0, 0)` for an empty
/// list or zero height.
fn visible_window(
    total: usize,
    selected: Option<usize>,
    scroll: usize,
    height: usize,
) -> (usize, usize) {
    if total == 0 || height == 0 {
        return (0, 0);
    }
    // No cursor: park the window at `scroll`, clamped so a full viewport
    // fits inside the list.
    let selected = selected
        .map(|s| s.min(total - 1))
        .unwrap_or_else(|| scroll.min(total.saturating_sub(height)));
    let start = if selected < scroll {
        selected
    } else if selected >= scroll + height {
        selected + 1 - height
    } else {
        scroll
    };
    (start, (start + height).min(total))
}

fn row_span(
    selected: bool,
    file: &crate::services::list_files::FEntry,
    walking: Option<Instant>,
    note: Option<&SizeInfo>,
    deleting: Option<Instant>,
    theme: &Theme,
    icons: IconSet,
) -> Line<'static> {
    let mark = if selected { "[*]" } else { "[ ]" };
    let (glyph, cat) = icon_for(file, icons);
    let icon = match (deleting, walking) {
        (Some(started), _) => pad_icon(&spinner_char(started).to_string(), icons),
        (None, Some(started)) => pad_icon(&spinner_char(started).to_string(), icons),
        (None, None) => pad_icon(glyph, icons),
    };
    let mark_style = if selected {
        Style::default().fg(theme.selection)
    } else {
        Style::default().fg(theme.text_muted)
    };
    let name_style = name_style_for(file, theme);
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(mark, mark_style),
        Span::raw(" "),
        Span::styled(icon, Style::default().fg(theme.color_for(cat))),
        Span::raw(" "),
        Span::styled(file.label.clone(), name_style),
    ];
    if let Some(si) = note {
        spans.push(Span::styled(
            format!(" ({})", list_note(si)),
            Style::default().fg(theme.text_muted),
        ));
    }
    Line::from(spans)
}

/// Folder names are bold accent; dotfiles are muted; others use body text.
fn name_style_for(entry: &crate::services::list_files::FEntry, theme: &Theme) -> Style {
    if entry.label.starts_with('.') {
        Style::default().fg(theme.hidden)
    } else if entry.is_dir {
        Style::default().fg(theme.dir).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text)
    }
}

/// Pane title: bold folder label, muted path, optional `/query` in accent.
fn pane_title(
    label: &str,
    path: &str,
    query: Option<&str>,
    filter_hint: bool,
    mode: Option<&str>,
    theme: &Theme,
) -> Line<'static> {
    let mut spans = vec![
        Span::styled(
            format!("  {label}  "),
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{path}  "), Style::default().fg(theme.text_muted)),
    ];
    if let Some(q) = query {
        spans.push(Span::styled(
            format!("/{q}"),
            Style::default().fg(theme.accent),
        ));
        if filter_hint {
            spans.push(Span::styled(
                "  (Esc clears)",
                Style::default().fg(theme.text_muted),
            ));
        }
        spans.push(Span::raw(" "));
    }
    if let Some(mode) = mode {
        spans.push(Span::styled(
            format!("({mode})  "),
            Style::default().fg(theme.text_muted),
        ));
    }
    Line::from(spans)
}

/// Concatenate a `Line`'s spans (unit tests compare buffer-like text).
#[cfg(test)]
fn line_text(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        detail_row, grid_window, line_text, modified_ago, row_span, truncate_chars, visible_window,
    };
    use crate::theme::icons::IconSet;
    use crate::theme::Theme;

    fn theme() -> Theme {
        Theme::default()
    }

    #[test]
    fn details_rows_show_file_size_and_relative_time() {
        let file = FEntry {
            path: "/x/big.bin".to_string(),
            label: "big.bin".to_string(),
            is_dir: false,
            size: 1_572_864, // 1.5 MiB
            modified: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
                    - 3 * 24 * 3600,
            ),
        };
        let row = line_text(&detail_row(
            false,
            &file,
            20,
            None,
            &theme(),
            IconSet::Unicode,
        ));
        assert!(row.contains("big.bin"), "name present: {row:?}");
        assert!(row.contains("1.5 MiB"), "file size rendered: {row:?}");
        assert!(
            row.contains("3 days ago"),
            "relative mtime rendered: {row:?}"
        );
    }

    #[test]
    fn relative_time_units_and_unknown() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert_eq!(modified_ago(Some(now - 30)), "just now");
        assert_eq!(modified_ago(Some(now - 2 * 60)), "2 minutes ago");
        assert_eq!(modified_ago(Some(now - 3600)), "1 hour ago");
        assert_eq!(modified_ago(Some(now - 2 * 86400)), "2 days ago");
        // Past a year: one-decimal years, integer from ten, singular at one.
        assert_eq!(modified_ago(Some(now - 364 * 86400)), "364 days ago");
        assert_eq!(modified_ago(Some(now - 365 * 86400)), "1 year ago");
        assert_eq!(modified_ago(Some(now - 1403 * 86400)), "3.8 years ago");
        assert_eq!(modified_ago(Some(now - 4000 * 86400)), "11 years ago");
    }

    #[test]
    fn emoji_rows_occupy_the_same_cells_as_nerd_rows() {
        // A two-cell emoji must land the name column exactly where the
        // Nerd glyph + pad space does, so switching sets never shifts text.
        use unicode_width::UnicodeWidthStr;
        let file = FEntry {
            path: "/x/main.rs".to_string(),
            label: "main.rs".to_string(),
            is_dir: false,
            size: 4096,
            modified: None,
        };
        let name_w = 30usize;
        let expect = |set: IconSet| {
            let prefix = 6 + crate::theme::icons::icon_cols(set);
            name_w + prefix + 2 + super::DETAIL_SIZE_W + 2 + super::DETAIL_AGO_W
        };
        for set in [IconSet::Nerd, IconSet::Emoji, IconSet::Unicode] {
            let text = line_text(&detail_row(false, &file, name_w, None, &theme(), set));
            assert_eq!(
                UnicodeWidthStr::width(text.as_str()),
                expect(set),
                "{set:?} row must fill its cell budget: {text:?}"
            );
        }
        assert_eq!(expect(IconSet::Nerd), expect(IconSet::Emoji));
    }

    #[test]
    fn details_rows_fit_the_pane_width_budget() {
        // The clipped "1403 days ag" bug: the span must fit the per-row
        // budget — highlight symbol (1) + borders (2) + this span = width.
        let file = FEntry {
            path: "/x/old.bin".to_string(),
            label: "old.bin".to_string(),
            is_dir: false,
            size: 4096,
            modified: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
                    - 1403 * 86400,
            ),
        };
        for pane_width in [80usize, 100, 140] {
            let prefix = 6 + crate::theme::icons::icon_cols(IconSet::Unicode);
            let name_w =
                pane_width - 2 - (1 + prefix + super::DETAIL_SIZE_W + super::DETAIL_AGO_W + 4);
            let row = detail_row(false, &file, name_w, None, &theme(), IconSet::Unicode);
            let text = line_text(&row);
            assert_eq!(
                text.chars().count(),
                name_w + prefix + 2 + super::DETAIL_SIZE_W + 2 + super::DETAIL_AGO_W,
                "row must exactly fill its budget at pane width {pane_width}"
            );
            assert!(
                text.ends_with("3.8 years ago"),
                "ago column fully present, not clipped: {text:?}"
            );
        }
    }

    #[test]
    fn details_rows_never_show_a_folder_size() {
        let dir = FEntry {
            path: "/x/src".to_string(),
            label: "src".to_string(),
            is_dir: true,
            size: 0,
            modified: None,
        };
        let row = line_text(&detail_row(
            false,
            &dir,
            20,
            None,
            &theme(),
            IconSet::Unicode,
        ));
        assert!(row.contains(" — "), "folder size is a dash: {row:?}");
        assert!(!row.contains("B "), "no byte size for folders: {row:?}");
        assert!(row.contains("—"), "unknown mtime is a dash: {row:?}");
    }

    #[test]
    fn truncation_fits_the_name_column() {
        assert_eq!(truncate_chars("short", 10), "short");
        assert_eq!(truncate_chars("0123456789abc", 10), "012345678…");
        assert_eq!(truncate_chars("", 10).chars().count(), 0);
    }
    use crate::services::list_files::FEntry;

    #[test]
    fn folder_and_file_rows_have_distinct_icons() {
        let dir = FEntry {
            path: "/x/src".to_string(),
            label: "src".to_string(),
            is_dir: true,
            size: 0,
            modified: None,
        };
        let file = FEntry {
            path: "/x/r.txt".to_string(),
            label: "r.txt".to_string(),
            is_dir: false,
            size: 0,
            modified: None,
        };

        let d = line_text(&row_span(
            false,
            &dir,
            None,
            None,
            None,
            &theme(),
            IconSet::Unicode,
        ));
        let f = line_text(&row_span(
            false,
            &file,
            None,
            None,
            None,
            &theme(),
            IconSet::Unicode,
        ));
        assert!(
            d.contains('□'),
            "folder row should carry the folder square: {d:?}"
        );
        assert!(
            f.contains('≡'),
            "text file row should carry the text glyph: {f:?}"
        );
        assert_ne!(d, f, "folder and file rows must render differently");
    }

    #[test]
    fn completed_size_annotates_the_folder_name() {
        use crate::services::file_info::SizeInfo;
        use std::time::SystemTime;
        let dir = FEntry {
            path: "/x/cybertouch".to_string(),
            label: "cybertouch".to_string(),
            is_dir: true,
            size: 0,
            modified: None,
        };
        let si = SizeInfo {
            bytes: 322_000_000_000,
            items: 248_662,
            on_disk: 400_000_000_000,
            complete: true,
            updated: SystemTime::now(),
        };
        let row = line_text(&row_span(
            false,
            &dir,
            None,
            Some(&si),
            None,
            &theme(),
            IconSet::Unicode,
        ));
        assert!(
            row.starts_with(" [ ] □ cybertouch ("),
            "folder row keeps mark and icon: {row:?}"
        );
        assert!(row.contains("cybertouch ("), "{row:?}");
        assert!(
            row.contains("data / 372.5 GiB on disk - last updated: "),
            "{row:?}"
        );
        assert!(!row.contains("before 1970"), "{row:?}");
    }

    #[test]
    fn walking_folder_shows_spinner_icon() {
        use std::time::Instant;
        let dir = FEntry {
            path: "/x/src".to_string(),
            label: "src".to_string(),
            is_dir: true,
            size: 0,
            modified: None,
        };
        let plain = line_text(&row_span(
            false,
            &dir,
            None,
            None,
            None,
            &theme(),
            IconSet::Unicode,
        ));
        let walking = line_text(&row_span(
            false,
            &dir,
            Some(Instant::now()),
            None,
            None,
            &theme(),
            IconSet::Unicode,
        ));
        assert_ne!(plain, walking, "walking folder swaps its icon");
        assert!(walking.contains("src"));
    }

    #[test]
    fn window_follows_cursor_down() {
        // Cursor walks past the bottom edge; the window slides just enough
        // to keep the cursor on the last visible row.
        assert_eq!(visible_window(1000, Some(0), 0, 20), (0, 20));
        assert_eq!(visible_window(1000, Some(19), 0, 20), (0, 20));
        assert_eq!(visible_window(1000, Some(20), 0, 20), (1, 21));
        // Stepping further, seeded from the previous window start.
        assert_eq!(visible_window(1000, Some(45), 1, 20), (26, 46));
        assert_eq!(visible_window(1000, Some(45), 26, 20), (26, 46));
    }

    #[test]
    fn window_jumps_up_when_cursor_moves_above_scroll() {
        // Cursor jumps to the top (e.g. Home): the window snaps back.
        assert_eq!(visible_window(1000, Some(0), 500, 20), (0, 20));
        assert_eq!(visible_window(1000, Some(5), 10, 20), (5, 25));
    }

    #[test]
    fn window_clamps_to_list_bounds() {
        // Fewer rows than the viewport: window is the whole list.
        assert_eq!(visible_window(10, Some(8), 0, 20), (0, 10));
        // Cursor near the end: end clamps to total, window still full-height
        // where possible.
        assert_eq!(visible_window(1000, Some(999), 0, 20), (980, 1000));
        // Stale cursor beyond the list (list shrank underneath it).
        assert_eq!(visible_window(100, Some(150), 0, 20), (80, 100));
    }

    #[test]
    fn window_handles_empty_list_and_zero_height() {
        assert_eq!(visible_window(0, Some(0), 0, 20), (0, 0));
        assert_eq!(visible_window(0, None, 0, 20), (0, 0));
        assert_eq!(visible_window(100, Some(3), 0, 0), (0, 0));
        // No selection keeps the window parked at the clamped scroll.
        assert_eq!(visible_window(1000, None, 30, 20), (30, 50));
        assert_eq!(visible_window(10, None, 30, 20), (0, 10));
    }

    #[test]
    fn grid_window_keeps_cursor_inside_viewport() {
        // Fewer cells than the viewport: window stays at 0.
        assert_eq!(grid_window(10, Some(9), 0, 20), 0);
        // Cursor walks past the window: window follows.
        assert_eq!(grid_window(100, Some(25), 0, 20), 6);
        // Cursor walks back above it: window follows.
        assert_eq!(grid_window(100, Some(5), 6, 20), 5);
        // Stale scroll beyond the last full page clamps.
        assert_eq!(grid_window(30, None, 25, 20), 10);
        // Empty list / zero viewport.
        assert_eq!(grid_window(0, Some(0), 0, 20), 0);
        assert_eq!(grid_window(10, Some(0), 0, 0), 0);
    }
}
