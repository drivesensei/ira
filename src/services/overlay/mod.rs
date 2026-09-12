//! Native image overlay for terminals with no graphics protocol.
//!
//! On macOS Terminal.app the only way to show a real photograph is a GUI
//! surface (Finder does the same). Placement is a pure function of the
//! window frame, the text-area origin, the cell size and a cell rect.

use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use image::DynamicImage;
use ratatui::layout::Rect;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// Global screen rectangle in top-left, y-down pixels (AppleScript /
/// Windows convention). Converted to Cocoa bottom-left when showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Terminal window frame in the same coordinate space as [`ScreenRect`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowGeom {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// One terminal cell in device pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPx {
    pub width: u16,
    pub height: u16,
}

/// Pixel offset of cell (0, 0) inside the window (title bar + padding).
pub fn content_origin(window: WindowGeom, cols: u16, rows: u16, cell: CellPx) -> (u32, u32) {
    let text_w = cols as u32 * cell.width as u32;
    let text_h = rows as u32 * cell.height as u32;
    let ox = window.width.saturating_sub(text_w) / 2;
    // Chrome (title, tabs) sits above the text grid.
    let oy = window.height.saturating_sub(text_h);
    (ox, oy)
}

/// Maps a terminal cell rect to a global screen pixel rect.
pub fn preview_screen_rect(
    window: WindowGeom,
    content_origin: (u32, u32),
    cell: CellPx,
    area: Rect,
) -> ScreenRect {
    ScreenRect {
        x: window.x + content_origin.0 as i32 + i32::from(area.x) * i32::from(cell.width),
        y: window.y + content_origin.1 as i32 + i32::from(area.y) * i32::from(cell.height),
        width: u32::from(area.width) * u32::from(cell.width),
        height: u32::from(area.height) * u32::from(cell.height),
    }
}

/// Estimate cell pixels from the window frame and the terminal grid.
/// Width fills the client; leftover height after a 1:2 cell guess is title/tab chrome.
pub fn cell_px_from_window(window: WindowGeom, cols: u16, rows: u16) -> CellPx {
    let cols = u32::from(cols.max(1));
    let rows = u32::from(rows.max(1));
    let width = (window.width / cols).max(1);
    const MIN_CHROME: u32 = 22;
    const MAX_CHROME: u32 = 72;
    let guessed_text_h = rows.saturating_mul(width.saturating_mul(2));
    let leftover = window.height.saturating_sub(guessed_text_h);
    let chrome = leftover.clamp(
        MIN_CHROME,
        MAX_CHROME.min(window.height.saturating_sub(rows)),
    );
    let height = (window.height.saturating_sub(chrome) / rows).max(1);
    CellPx {
        width: width.min(u32::from(u16::MAX)) as u16,
        height: height.min(u32::from(u16::MAX)) as u16,
    }
}

/// Fit `img` into `w × h` pixels, aspect preserved, centered on transparent.
pub fn fit_pixels(img: &DynamicImage, w: u32, h: u32) -> DynamicImage {
    if w == 0 || h == 0 {
        return DynamicImage::new_rgba8(1, 1);
    }
    let resized = img.resize(w, h, image::imageops::FilterType::Triangle);
    let mut canvas = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 0]));
    let rgba = resized.to_rgba8();
    let nw = rgba.width().min(w);
    let nh = rgba.height().min(h);
    let ox = (w - nw) / 2;
    let oy = (h - nh) / 2;
    image::imageops::overlay(&mut canvas, &rgba, ox.into(), oy.into());
    DynamicImage::ImageRgba8(canvas)
}

/// Grid overlay fill: leave a margin so names stay visible under the panel.
pub const GRID_FILL_PERCENT: u32 = 80;

/// Composite tiles (cell-rect + image) onto one bitmap covering `area`.
/// `fill_percent` shrinks each tile (centered) so 80 leaves a gap above the name.
pub fn composite_grid(
    area: Rect,
    cell: CellPx,
    tiles: &[(Rect, &DynamicImage)],
    fill_percent: u32,
) -> Option<DynamicImage> {
    let w = u32::from(area.width) * u32::from(cell.width);
    let h = u32::from(area.height) * u32::from(cell.height);
    if w == 0 || h == 0 {
        return None;
    }
    let fill = fill_percent.clamp(1, 100);
    let mut canvas = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 0]));
    for (tile, img) in tiles {
        let tw = u32::from(tile.width) * u32::from(cell.width);
        let th = u32::from(tile.height) * u32::from(cell.height);
        if tw == 0 || th == 0 {
            continue;
        }
        let fw = (tw * fill / 100).max(1);
        let fh = (th * fill / 100).max(1);
        let fitted = fit_pixels(img, fw, fh).to_rgba8();
        let dx = i64::from(tile.x.saturating_sub(area.x)) * i64::from(cell.width)
            + i64::from((tw - fw) / 2);
        let dy = i64::from(tile.y.saturating_sub(area.y)) * i64::from(cell.height)
            + i64::from((th - fh) / 2);
        image::imageops::overlay(&mut canvas, &fitted, dx, dy);
    }
    Some(DynamicImage::ImageRgba8(canvas))
}

/// Stable hash of overlay placement + which thumbs are on screen.
pub fn placement_hash(
    window: WindowGeom,
    cell: CellPx,
    term: (u16, u16),
    area: Rect,
    tiles: &[(Rect, &str)],
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    window.x.hash(&mut hasher);
    window.y.hash(&mut hasher);
    window.width.hash(&mut hasher);
    window.height.hash(&mut hasher);
    cell.width.hash(&mut hasher);
    cell.height.hash(&mut hasher);
    term.hash(&mut hasher);
    area.x.hash(&mut hasher);
    area.y.hash(&mut hasher);
    area.width.hash(&mut hasher);
    area.height.hash(&mut hasher);
    tiles.len().hash(&mut hasher);
    for (tile, key) in tiles {
        tile.x.hash(&mut hasher);
        tile.y.hash(&mut hasher);
        tile.width.hash(&mut hasher);
        tile.height.hash(&mut hasher);
        key.hash(&mut hasher);
    }
    hasher.finish()
}

/// How often to re-read the terminal window (AppleScript is ~50–200 ms).
const WINDOW_CACHE_TTL: Duration = Duration::from_millis(200);

/// Native overlay window. No-op except on macOS and Windows.
pub struct Overlay {
    #[cfg(target_os = "macos")]
    inner: macos::MacOverlay,
    #[cfg(windows)]
    inner: windows::WinOverlay,
    window_cache: Option<(Instant, WindowGeom)>,
    cache_term: (u16, u16),
    shown_fp: Option<u64>,
}

impl Overlay {
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "macos")]
            inner: macos::MacOverlay::new(),
            #[cfg(windows)]
            inner: windows::WinOverlay::new(),
            window_cache: None,
            cache_term: (0, 0),
            shown_fp: None,
        }
    }

    /// Window bounds, reused for [`WINDOW_CACHE_TTL`] unless the cell grid changed.
    pub fn front_window_cached(&mut self, term: (u16, u16)) -> Option<WindowGeom> {
        if term != self.cache_term {
            self.window_cache = None;
        }
        if let Some((at, geom)) = self.window_cache {
            if at.elapsed() < WINDOW_CACHE_TTL {
                return Some(geom);
            }
        }
        let geom = front_window()?;
        self.window_cache = Some((Instant::now(), geom));
        self.cache_term = term;
        Some(geom)
    }

    pub fn is_current(&self, fp: u64) -> bool {
        self.shown_fp == Some(fp)
    }

    pub fn mark_current(&mut self, fp: u64) {
        self.shown_fp = Some(fp);
    }

    pub fn show(&mut self, img: &DynamicImage, rect: ScreenRect) {
        #[cfg(any(target_os = "macos", windows))]
        self.inner.show(img, rect);
        #[cfg(not(any(target_os = "macos", windows)))]
        let _ = (img, rect);
    }

    pub fn hide(&mut self) {
        self.shown_fp = None;
        #[cfg(any(target_os = "macos", windows))]
        self.inner.hide();
    }

    pub fn close(&mut self) {
        self.shown_fp = None;
        self.window_cache = None;
        #[cfg(any(target_os = "macos", windows))]
        self.inner.close();
    }
}

impl Default for Overlay {
    fn default() -> Self {
        Self::new()
    }
}

/// Frontmost terminal window, if the OS lets us read it.
pub fn front_window() -> Option<WindowGeom> {
    #[cfg(target_os = "macos")]
    {
        return macos::front_window();
    }
    #[cfg(windows)]
    {
        return windows::front_window();
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    None
}

/// Cell size from `TIOCGWINSZ` when the kernel fills pixel fields.
pub fn ioctl_cell_px() -> Option<CellPx> {
    #[cfg(unix)]
    {
        let mut ws = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) } == 0;
        if ok && ws.ws_col > 0 && ws.ws_row > 0 && ws.ws_xpixel > 0 && ws.ws_ypixel > 0 {
            return Some(CellPx {
                width: ws.ws_xpixel / ws.ws_col,
                height: ws.ws_ypixel / ws.ws_row,
            });
        }
        return None;
    }
    #[cfg(not(unix))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_origin_puts_chrome_on_top() {
        let window = WindowGeom {
            x: 100,
            y: 50,
            width: 800,
            height: 600,
        };
        let cell = CellPx {
            width: 10,
            height: 20,
        };
        // 80×25 cells = 800×500; 100 px leftover is the title/tab bar.
        let (ox, oy) = content_origin(window, 80, 25, cell);
        assert_eq!((ox, oy), (0, 100));
    }

    #[test]
    fn preview_screen_rect_maps_cells_to_pixels() {
        let window = WindowGeom {
            x: 100,
            y: 50,
            width: 800,
            height: 600,
        };
        let cell = CellPx {
            width: 10,
            height: 20,
        };
        let origin = content_origin(window, 80, 25, cell);
        let area = Rect {
            x: 40,
            y: 3,
            width: 38,
            height: 20,
        };
        let r = preview_screen_rect(window, origin, cell, area);
        assert_eq!(r.x, 100 + 0 + 40 * 10);
        assert_eq!(r.y, 50 + 100 + 3 * 20);
        assert_eq!(r.width, 38 * 10);
        assert_eq!(r.height, 20 * 20);
    }

    #[test]
    fn composite_grid_places_a_tile() {
        let red =
            DynamicImage::ImageRgb8(image::RgbImage::from_pixel(4, 4, image::Rgb([255, 0, 0])));
        let area = Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 2,
        };
        let tile = Rect {
            x: 2,
            y: 0,
            width: 2,
            height: 2,
        };
        let cell = CellPx {
            width: 2,
            height: 2,
        };
        let out = composite_grid(area, cell, &[(tile, &red)], 100).unwrap();
        assert_eq!(out.width(), 8);
        assert_eq!(out.height(), 4);
        // Left half stays transparent; a pixel inside the right tile is red.
        let rgba = out.to_rgba8();
        assert_eq!(rgba.get_pixel(1, 1), &image::Rgba([0, 0, 0, 0]));
        assert_eq!(rgba.get_pixel(6, 1)[0], 255);
        assert_eq!(rgba.get_pixel(6, 1)[3], 255);
    }

    #[test]
    fn resize_changes_overlay_rect() {
        let area = Rect {
            x: 10,
            y: 2,
            width: 20,
            height: 8,
        };
        let small = WindowGeom {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        };
        let large = WindowGeom {
            x: 0,
            y: 0,
            width: 1600,
            height: 900,
        };
        let c1 = cell_px_from_window(small, 80, 25);
        let c2 = cell_px_from_window(large, 80, 25);
        assert!(c1.width > 0 && c1.height > 0);
        assert_ne!(c1, c2);
        let r1 = preview_screen_rect(small, content_origin(small, 80, 25, c1), c1, area);
        let r2 = preview_screen_rect(large, content_origin(large, 80, 25, c2), c2, area);
        assert_ne!((r1.width, r1.height), (r2.width, r2.height));
    }

    #[test]
    fn grid_fill_leaves_a_margin() {
        let red =
            DynamicImage::ImageRgb8(image::RgbImage::from_pixel(8, 8, image::Rgb([255, 0, 0])));
        let area = Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        };
        let tile = area;
        let cell = CellPx {
            width: 5,
            height: 5,
        };
        let out = composite_grid(area, cell, &[(tile, &red)], GRID_FILL_PERCENT).unwrap();
        let rgba = out.to_rgba8();
        // 10×10 tile at 80% is 8×8 centered → 1 px margin.
        assert_eq!(rgba.get_pixel(0, 0), &image::Rgba([0, 0, 0, 0]));
        assert_eq!(rgba.get_pixel(5, 5)[0], 255);
        assert_eq!(rgba.get_pixel(5, 5)[3], 255);
    }
}
