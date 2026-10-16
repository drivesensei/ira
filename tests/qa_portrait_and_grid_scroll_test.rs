use image::DynamicImage;
use ira::app::{App, PreviewMode};
use ira::handler::{handle_key_events, handle_mouse_events};
use ira::services::list_files::FEntry;
use ira::services::overlay::{
    composite_grid, grid_tile_px, CellPx, GRID_FILL_H_PERCENT, GRID_FILL_PERCENT,
    GRID_FILL_W_PERCENT,
};
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

fn mock_entry(path: &str) -> FEntry {
    FEntry {
        path: path.to_string(),
        label: path.rsplit('/').next().unwrap_or(path).to_string(),
        is_dir: false,
        size: 1024,
        modified: None,
    }
}

#[test]
fn qa_portrait_and_square_grid_tile_leaves_margin_above_name_area() {
    // Standard grid cell and tile dimensions:
    // Tile: 20 cells wide by 8 cells high (matches GRID_CELL_W and GRID_IMG_H)
    // Cell: 10px wide by 20px high -> Tile pixel size: 200px wide x 160px high
    let tile = Rect::new(0, 0, 20, 8);
    let area = tile;
    let cell = CellPx {
        width: 10,
        height: 20,
    };
    let tile_w_px = 200u32;
    let tile_h_px = 160u32;
    let expected_margin_px = 20u32; // 1 full row (20px >= 16px)

    // 1. Portrait image (aspect ratio 1:2 -> 100w x 200h)
    let portrait = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        100,
        200,
        image::Rgba([200, 50, 50, 255]),
    ));
    let out_portrait = composite_grid(area, cell, &[(tile, &portrait)], GRID_FILL_PERCENT)
        .expect("composite_grid should produce an image");
    let rgba_p = out_portrait.to_rgba8();

    // Verify transparent margin (alpha == 0) across all columns in the bottom row (16+ px)
    for y in (tile_h_px - expected_margin_px)..tile_h_px {
        for x in 0..tile_w_px {
            assert_eq!(
                rgba_p.get_pixel(x, y)[3],
                0,
                "Portrait pixel at ({x}, {y}) must have alpha 0 in margin above name area"
            );
        }
    }

    // 2. Square image (aspect ratio 1:1 -> 200w x 200h)
    let square = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        200,
        200,
        image::Rgba([50, 200, 50, 255]),
    ));
    let out_square = composite_grid(area, cell, &[(tile, &square)], GRID_FILL_PERCENT)
        .expect("composite_grid should produce an image");
    let rgba_sq = out_square.to_rgba8();

    for y in (tile_h_px - expected_margin_px)..tile_h_px {
        for x in 0..tile_w_px {
            assert_eq!(
                rgba_sq.get_pixel(x, y)[3],
                0,
                "Square pixel at ({x}, {y}) must have alpha 0 in margin above name area"
            );
        }
    }

    // 3. Landscape image (aspect ratio 2:1 -> 400w x 200h)
    let landscape = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        400,
        200,
        image::Rgba([50, 50, 200, 255]),
    ));
    let out_landscape = composite_grid(area, cell, &[(tile, &landscape)], GRID_FILL_PERCENT)
        .expect("composite_grid should produce an image");
    let rgba_l = out_landscape.to_rgba8();

    // Landscape fills width substantially (~95%)
    let mut min_x = u32::MAX;
    let mut max_x = 0;
    for y in 0..tile_h_px {
        for x in 0..tile_w_px {
            if rgba_l.get_pixel(x, y)[3] > 0 {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
    }
    let landscape_w = max_x.saturating_sub(min_x) + 1;
    assert!(
        landscape_w >= 180,
        "Landscape width should be at least 180px (~90-95%), got {landscape_w}"
    );

    // Verify constants and grid_tile_px capping
    assert_eq!(GRID_FILL_W_PERCENT, 95);
    assert_eq!(GRID_FILL_H_PERCENT, 78);

    let (tw, th) = grid_tile_px(tile, cell, GRID_FILL_PERCENT);
    assert_eq!(tw, 200 * 95 / 100);
    assert_eq!(th, 160 * 78 / 100);
}

#[test]
fn qa_mouse_scroll_and_page_navigation_in_list_and_grid_with_modals() {
    let mut app = App::default();
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());
    let scroll_down = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 5,
        row: 5,
        modifiers: KeyModifiers::empty(),
    };
    let scroll_up = MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 5,
        row: 5,
        modifiers: KeyModifiers::empty(),
    };

    // Populate pane with 50 files
    app.panes[0].files = (0..50).map(|i| mock_entry(&format!("/f_{i}.txt"))).collect();
    app.panes[0].state.select(Some(0));

    // --- LIST MODE ---
    // ScrollDown advances by 1
    handle_mouse_events(scroll_down, &mut app).unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(1));

    // ScrollUp decreases by 1
    handle_mouse_events(scroll_up, &mut app).unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(0));

    // PageDown advances by 10
    handle_key_events(key(KeyCode::PageDown), &mut app).unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(10));
    handle_key_events(key(KeyCode::PageDown), &mut app).unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(20));

    // PageUp decreases by 10
    handle_key_events(key(KeyCode::PageUp), &mut app).unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(10));
    handle_key_events(key(KeyCode::PageUp), &mut app).unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(0));

    // Clamping: PageUp at 0 stays at 0
    handle_key_events(key(KeyCode::PageUp), &mut app).unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(0));

    // Clamping: PageDown past end clamps to 49
    for _ in 0..10 {
        handle_key_events(key(KeyCode::PageDown), &mut app).unwrap();
    }
    assert_eq!(app.panes[0].state.selected(), Some(49));

    // --- MODAL SUPPRESSION ---
    // When modal dialog is open, mouse scroll must be suppressed
    app.keybindings_visible = true;
    handle_mouse_events(scroll_down, &mut app).unwrap();
    assert_eq!(
        app.panes[0].state.selected(),
        Some(49),
        "ScrollDown suppressed while keybindings modal is open"
    );
    app.keybindings_visible = false;

    // --- GRID MODE ---
    app.panes[0].preview_mode = PreviewMode::Grid;
    app.panes[0].grid_cols = 4;
    app.panes[0].grid_rows = 3; // Page size = 4 * 3 = 12 items
    app.panes[0].state.select(Some(0));

    // ScrollDown advances by cols (4)
    handle_mouse_events(scroll_down, &mut app).unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(4));

    // ScrollUp decreases by cols (4)
    handle_mouse_events(scroll_up, &mut app).unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(0));

    // PageDown advances by 12 (cols * rows)
    handle_key_events(key(KeyCode::PageDown), &mut app).unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(12));
    handle_key_events(key(KeyCode::PageDown), &mut app).unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(24));

    // PageUp decreases by 12
    handle_key_events(key(KeyCode::PageUp), &mut app).unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(12));
    handle_key_events(key(KeyCode::PageUp), &mut app).unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(0));

    // Empty pane boundary test
    let mut empty_app = App::default();
    empty_app.panes[0].preview_mode = PreviewMode::Grid;
    empty_app.page_down();
    assert_eq!(empty_app.panes[0].state.selected(), None);
    empty_app.page_up();
    assert_eq!(empty_app.panes[0].state.selected(), None);
}
