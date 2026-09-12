//! 2×4 braille block-graphics renderer for terminals without a protocol.
//!
//! Each cell encodes eight pixels as a U+2800 braille glyph plus fg/bg
//! colors (dots = fg, empty = bg). That is four times the resolution of
//! ratatui-image Halfblocks (1×2) and twice the old quadrant renderer
//! (2×2). A smoothly downscaled photo no longer collapses to a mosaic of
//! solid `█` cells. Menlo, Cascadia Mono, Consolas and DejaVu all cover
//! the braille block. When the terminal is not truecolor the colors are
//! quantized to xterm-256 so Terminal.app does not get 24-bit SGR.

use image::DynamicImage;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

/// One cell of braille graphics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cell {
    glyph: char,
    fg: Color,
    bg: Color,
}

/// Fitted braille image, ready to draw into a `cols × rows` area.
#[derive(Debug, Clone)]
pub struct BlockImage {
    cols: u16,
    rows: u16,
    cells: Vec<Cell>,
}

/// Braille dot bits for a 2×4 pixel cell: (dx, dy, bit).
///
/// ```text
/// 1 4     0x01 0x08
/// 2 5     0x02 0x10
/// 3 6     0x04 0x20
/// 7 8     0x40 0x80
/// ```
const BRAILLE_DOTS: [(u32, u32, u8); 8] = [
    (0, 0, 0x01),
    (0, 1, 0x02),
    (0, 2, 0x04),
    (0, 3, 0x40),
    (1, 0, 0x08),
    (1, 1, 0x10),
    (1, 2, 0x20),
    (1, 3, 0x80),
];

/// Rec. 601 luma, scaled by 1000.
fn luma(p: [u8; 3]) -> u32 {
    299 * p[0] as u32 + 587 * p[1] as u32 + 114 * p[2] as u32
}

impl BlockImage {
    /// Fit `img` into `cols × rows` cells (2×4 pixels each), aspect
    /// preserved, centered on black. Alpha is composited on black.
    pub fn from_image(img: &DynamicImage, cols: u16, rows: u16, truecolor: bool) -> Self {
        if cols == 0 || rows == 0 {
            return Self {
                cols,
                rows,
                cells: Vec::new(),
            };
        }
        let tw = cols as u32 * 2;
        let th = rows as u32 * 4;
        let canvas = fit_on_black(img, tw, th);
        let mut cells = Vec::with_capacity(cols as usize * rows as usize);
        for row in 0..rows {
            for col in 0..cols {
                let x = col as u32 * 2;
                let y = row as u32 * 4;
                let mut px = [[0u8; 3]; 8];
                for (i, &(dx, dy, _)) in BRAILLE_DOTS.iter().enumerate() {
                    px[i] = canvas_rgb(&canvas, x + dx, y + dy);
                }
                cells.push(braille_cell(px, truecolor));
            }
        }
        Self { cols, rows, cells }
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    #[cfg(test)]
    fn cell(&self, col: u16, row: u16) -> Option<(char, Color, Color)> {
        let i = row as usize * self.cols as usize + col as usize;
        self.cells.get(i).map(|c| (c.glyph, c.fg, c.bg))
    }
}

impl Widget for &BlockImage {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let w = area.width.min(self.cols);
        let h = area.height.min(self.rows);
        for y in 0..h {
            for x in 0..w {
                let i = y as usize * self.cols as usize + x as usize;
                let Some(cell) = self.cells.get(i) else {
                    continue;
                };
                let pos = (area.x + x, area.y + y);
                if let Some(dst) = buf.cell_mut(pos) {
                    dst.set_char(cell.glyph);
                    dst.set_style(Style::default().fg(cell.fg).bg(cell.bg));
                }
            }
        }
    }
}

fn fit_on_black(img: &DynamicImage, tw: u32, th: u32) -> image::RgbaImage {
    let resized = img.resize(tw, th, image::imageops::FilterType::Triangle);
    let mut canvas = image::RgbaImage::from_pixel(tw, th, image::Rgba([0, 0, 0, 255]));
    let nw = resized.width().min(tw);
    let nh = resized.height().min(th);
    let ox = (tw - nw) / 2;
    let oy = (th - nh) / 2;
    image::imageops::overlay(&mut canvas, &resized, ox.into(), oy.into());
    canvas
}

fn canvas_rgb(img: &image::RgbaImage, x: u32, y: u32) -> [u8; 3] {
    let p = img
        .get_pixel(x.min(img.width() - 1), y.min(img.height() - 1))
        .0;
    composite_on_black(p)
}

fn composite_on_black(p: [u8; 4]) -> [u8; 3] {
    if p[3] == 255 {
        return [p[0], p[1], p[2]];
    }
    if p[3] == 0 {
        return [0, 0, 0];
    }
    let a = p[3] as u32;
    [
        ((p[0] as u32 * a) / 255) as u8,
        ((p[1] as u32 * a) / 255) as u8,
        ((p[2] as u32 * a) / 255) as u8,
    ]
}

fn braille_cell(px: [[u8; 3]; 8], truecolor: bool) -> Cell {
    let mut min_l = u32::MAX;
    let mut max_l = 0u32;
    let mut sum_l = 0u32;
    for p in &px {
        let l = luma(*p);
        min_l = min_l.min(l);
        max_l = max_l.max(l);
        sum_l += l;
    }
    // Flat cell: a solid block reads cleaner than a field of braille dots.
    // ~8% of full luma range (0..255000).
    if max_l.saturating_sub(min_l) < 20_000 {
        let mean = avg_n(&px);
        let c = to_color(mean, truecolor);
        return Cell {
            glyph: '█',
            fg: c,
            bg: c,
        };
    }
    let mean_l = sum_l / 8;
    let mut mask = 0u8;
    let mut fg_px = Vec::with_capacity(8);
    let mut bg_px = Vec::with_capacity(8);
    for (i, p) in px.iter().enumerate() {
        if luma(*p) >= mean_l {
            mask |= BRAILLE_DOTS[i].2;
            fg_px.push(*p);
        } else {
            bg_px.push(*p);
        }
    }
    // Degenerate split (every pixel on one side of the mean): still a
    // solid, because a 0-dot or 8-dot braille cell is just a color wash.
    if fg_px.is_empty() || bg_px.is_empty() {
        let mean = avg_n(&px);
        let c = to_color(mean, truecolor);
        return Cell {
            glyph: '█',
            fg: c,
            bg: c,
        };
    }
    Cell {
        glyph: char::from_u32(0x2800 + mask as u32).unwrap_or('█'),
        fg: to_color(avg_n(&fg_px), truecolor),
        bg: to_color(avg_n(&bg_px), truecolor),
    }
}

fn avg_n(px: &[[u8; 3]]) -> [u8; 3] {
    let n = px.len().max(1) as u32;
    let mut s = [0u32; 3];
    for p in px {
        s[0] += p[0] as u32;
        s[1] += p[1] as u32;
        s[2] += p[2] as u32;
    }
    [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]
}

fn sse(a: &[u8; 3], b: &[u8; 3]) -> u64 {
    let mut s = 0u64;
    for i in 0..3 {
        let d = a[i] as i32 - b[i] as i32;
        s += (d * d) as u64;
    }
    s
}

fn to_color(rgb: [u8; 3], truecolor: bool) -> Color {
    if truecolor {
        Color::Rgb(rgb[0], rgb[1], rgb[2])
    } else {
        Color::Indexed(to_xterm256(rgb[0], rgb[1], rgb[2]))
    }
}

/// Nearest xterm-256 index (6³ cube or 24-step gray).
fn to_xterm256(r: u8, g: u8, b: u8) -> u8 {
    const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let nearest = |c: u8| {
        let mut best_i = 0u8;
        let mut best_d = 255u32;
        for (i, &v) in CUBE.iter().enumerate() {
            let d = (c as i32 - v as i32).unsigned_abs();
            if d < best_d {
                best_d = d;
                best_i = i as u8;
            }
        }
        (best_i, CUBE[best_i as usize])
    };
    let (ri, rv) = nearest(r);
    let (gi, gv) = nearest(g);
    let (bi, bv) = nearest(b);
    let cube = 16 + 36 * ri + 6 * gi + bi;
    let cube_err = sse(&[r, g, b], &[rv, gv, bv]);

    let gray = ((r as u32 + g as u32 + b as u32) / 3) as u8;
    let gi = ((gray.saturating_sub(8)) as u32 / 10).min(23);
    let gv = (8 + 10 * gi) as u8;
    let gray_idx = 232 + gi as u8;
    let gray_err = sse(&[r, g, b], &[gv, gv, gv]);
    if gray_err < cube_err {
        gray_idx
    } else {
        cube
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn solid(r: u8, g: u8, b: u8, w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, Rgb([r, g, b])))
    }

    #[test]
    fn solid_image_is_full_block_one_color() {
        // Exact 4×3-cell pixel grid (2×4 per cell) so fit_on_black does
        // not letterbox.
        let img = solid(200, 40, 40, 8, 12);
        let blocks = BlockImage::from_image(&img, 4, 3, true);
        assert_eq!((blocks.cols(), blocks.rows()), (4, 3));
        for row in 0..3 {
            for col in 0..4 {
                let (g, fg, bg) = blocks.cell(col, row).unwrap();
                assert_eq!(g, '█');
                assert_eq!(fg, bg);
                match fg {
                    Color::Rgb(r, _, _) => assert!(r > 150, "red channel {r}"),
                    other => panic!("expected rgb, got {other:?}"),
                }
            }
        }
    }

    #[test]
    fn left_right_split_sets_left_braille_dots() {
        // 1×1 cell = 2×4 dest pixels; left column red (brighter), right
        // blue. Dots 1,2,3,7 = 0x47 → U+2847.
        let mut rgb = RgbImage::new(2, 4);
        for y in 0..4 {
            rgb.put_pixel(0, y, Rgb([255, 0, 0]));
            rgb.put_pixel(1, y, Rgb([0, 0, 255]));
        }
        let blocks = BlockImage::from_image(&DynamicImage::ImageRgb8(rgb), 1, 1, true);
        let (g, fg, bg) = blocks.cell(0, 0).unwrap();
        assert_eq!(g, char::from_u32(0x2847).unwrap());
        assert_ne!(fg, bg);
    }

    #[test]
    fn output_dims_match_the_area() {
        let img = solid(10, 10, 10, 8, 8);
        let blocks = BlockImage::from_image(&img, 7, 5, true);
        assert_eq!(blocks.cols(), 7);
        assert_eq!(blocks.rows(), 5);
        assert_eq!(blocks.cells.len(), 35);
    }

    #[test]
    fn indexed_colors_when_not_truecolor() {
        let img = solid(200, 40, 40, 8, 8);
        let blocks = BlockImage::from_image(&img, 1, 1, false);
        let (_, fg, bg) = blocks.cell(0, 0).unwrap();
        assert!(matches!(fg, Color::Indexed(_)));
        assert!(matches!(bg, Color::Indexed(_)));
    }

    #[test]
    fn renders_into_a_buffer_without_panic() {
        let img = solid(80, 80, 200, 16, 16);
        let blocks = BlockImage::from_image(&img, 6, 4, true);
        let backend = ratatui::backend::TestBackend::new(8, 6);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| f.render_widget(&blocks, f.area()))
            .unwrap();
    }
}
