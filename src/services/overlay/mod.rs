//! Native image overlay for terminals with no graphics protocol.
//!
//! On macOS Terminal.app the only way to show a real photograph is a GUI
//! surface (Finder does the same). Placement is a pure function of the
//! window frame, the text-area origin, the cell size and a cell rect.

use image::DynamicImage;
use ratatui::layout::Rect;

#[cfg(target_os = "macos")]
mod macos;

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

/// Fit `img` into `w × h` pixels, aspect preserved, centered on black.
pub fn fit_pixels(img: &DynamicImage, w: u32, h: u32) -> DynamicImage {
    if w == 0 || h == 0 {
        return DynamicImage::new_rgb8(1, 1);
    }
    let resized = img.resize(w, h, image::imageops::FilterType::Triangle);
    let mut canvas = image::RgbImage::from_pixel(w, h, image::Rgb([0, 0, 0]));
    let nw = resized.width().min(w);
    let nh = resized.height().min(h);
    let ox = (w - nw) / 2;
    let oy = (h - nh) / 2;
    image::imageops::overlay(&mut canvas, &resized.to_rgb8(), ox.into(), oy.into());
    DynamicImage::ImageRgb8(canvas)
}

/// Composite tiles (cell-rect + image) onto one bitmap covering `area`.
pub fn composite_grid(
    area: Rect,
    cell: CellPx,
    tiles: &[(Rect, &DynamicImage)],
) -> Option<DynamicImage> {
    let w = u32::from(area.width) * u32::from(cell.width);
    let h = u32::from(area.height) * u32::from(cell.height);
    if w == 0 || h == 0 {
        return None;
    }
    let mut canvas = image::RgbImage::from_pixel(w, h, image::Rgb([0, 0, 0]));
    for (tile, img) in tiles {
        let tw = u32::from(tile.width) * u32::from(cell.width);
        let th = u32::from(tile.height) * u32::from(cell.height);
        if tw == 0 || th == 0 {
            continue;
        }
        let fitted = fit_pixels(img, tw, th).to_rgb8();
        let dx = i64::from(tile.x.saturating_sub(area.x)) * i64::from(cell.width);
        let dy = i64::from(tile.y.saturating_sub(area.y)) * i64::from(cell.height);
        image::imageops::overlay(&mut canvas, &fitted, dx, dy);
    }
    Some(DynamicImage::ImageRgb8(canvas))
}

/// Native overlay window. No-op on platforms other than macOS.
pub struct Overlay {
    #[cfg(target_os = "macos")]
    inner: macos::MacOverlay,
}

impl Overlay {
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "macos")]
            inner: macos::MacOverlay::new(),
        }
    }

    pub fn show(&mut self, img: &DynamicImage, rect: ScreenRect) {
        #[cfg(target_os = "macos")]
        self.inner.show(img, rect);
        #[cfg(not(target_os = "macos"))]
        let _ = (img, rect);
    }

    pub fn hide(&mut self) {
        #[cfg(target_os = "macos")]
        self.inner.hide();
    }

    pub fn close(&mut self) {
        #[cfg(target_os = "macos")]
        self.inner.close();
    }

    pub fn pump(&mut self) {
        #[cfg(target_os = "macos")]
        self.inner.pump();
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
    #[cfg(not(target_os = "macos"))]
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
        let out = composite_grid(area, cell, &[(tile, &red)]).unwrap();
        assert_eq!(out.width(), 8);
        assert_eq!(out.height(), 4);
        // Left half stays black; a pixel inside the right tile is red.
        let rgb = out.to_rgb8();
        assert_eq!(rgb.get_pixel(1, 1), &image::Rgb([0, 0, 0]));
        assert_eq!(rgb.get_pixel(6, 1), &image::Rgb([255, 0, 0]));
    }
}
