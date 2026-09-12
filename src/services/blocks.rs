//! 2×2 quadrant block-graphics renderer for terminals without a protocol.
//!
//! Each cell encodes four pixels using a U+2580–259F quadrant glyph plus
//! fg/bg colors. That is twice the resolution of ratatui-image Halfblocks
//! (1×2) and the glyphs are in Menlo, Cascadia Mono, Consolas and DejaVu.
//! When the terminal is not truecolor the colors are quantized to xterm-256
//! so Terminal.app does not get 24-bit SGR it cannot show.

use image::DynamicImage;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

/// One cell of quadrant graphics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cell {
    glyph: char,
    fg: Color,
    bg: Color,
}

/// Fitted quadrant image, ready to draw into a `cols × rows` area.
#[derive(Debug, Clone)]
pub struct BlockImage {
    cols: u16,
    rows: u16,
    cells: Vec<Cell>,
}

/// Quadrant glyphs for bitmasks: bit0=TL, bit1=TR, bit2=BL, bit3=BR.
const QUAD: [char; 16] = [
    ' ', // 0000
    '▘', // 0001 TL
    '▝', // 0010 TR
    '▀', // 0011 top
    '▖', // 0100 BL
    '▌', // 0101 left
    '▞', // 0110 TR+BL
    '▛', // 0111
    '▗', // 1000 BR
    '▚', // 1001 TL+BR
    '▐', // 1010 right
    '▜', // 1011
    '▄', // 1100 bottom
    '▙', // 1101
    '▟', // 1110
    '█', // 1111
];

impl BlockImage {
    /// Fit `img` into `cols × rows` cells (2×2 pixels each), aspect
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
        let th = rows as u32 * 2;
        let canvas = fit_on_black(img, tw, th);
        let mut cells = Vec::with_capacity(cols as usize * rows as usize);
        for row in 0..rows {
            for col in 0..cols {
                let x = col as u32 * 2;
                let y = row as u32 * 2;
                let px = [
                    canvas_rgb(&canvas, x, y),
                    canvas_rgb(&canvas, x + 1, y),
                    canvas_rgb(&canvas, x, y + 1),
                    canvas_rgb(&canvas, x + 1, y + 1),
                ];
                cells.push(best_cell(px, truecolor));
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

fn best_cell(px: [[u8; 3]; 4], truecolor: bool) -> Cell {
    if px[1] == px[0] && px[2] == px[0] && px[3] == px[0] {
        let c = to_color(px[0], truecolor);
        return Cell {
            glyph: '█',
            fg: c,
            bg: c,
        };
    }
    let mut best_err = u64::MAX;
    let mut best_mask = 15u8;
    // Seven unique bipartitions (masks 1..=7); complements are the same
    // split with fg/bg swapped.
    for mask in 1u8..=7 {
        let err = split_error(&px, mask);
        if err < best_err {
            best_err = err;
            best_mask = mask;
        }
    }
    let (fg, bg) = means(&px, best_mask);
    Cell {
        glyph: QUAD[best_mask as usize],
        fg: to_color(fg, truecolor),
        bg: to_color(bg, truecolor),
    }
}

fn split_error(px: &[[u8; 3]; 4], mask: u8) -> u64 {
    let (fg, bg) = means(px, mask);
    let mut err = 0u64;
    for (i, p) in px.iter().enumerate() {
        let mean = if mask & (1 << i) != 0 { fg } else { bg };
        err += sse(p, &mean);
    }
    err
}

fn means(px: &[[u8; 3]; 4], mask: u8) -> ([u8; 3], [u8; 3]) {
    let mut fg = [0u32; 3];
    let mut bg = [0u32; 3];
    let mut nf = 0u32;
    let mut nb = 0u32;
    for (i, p) in px.iter().enumerate() {
        if mask & (1 << i) != 0 {
            fg[0] += p[0] as u32;
            fg[1] += p[1] as u32;
            fg[2] += p[2] as u32;
            nf += 1;
        } else {
            bg[0] += p[0] as u32;
            bg[1] += p[1] as u32;
            bg[2] += p[2] as u32;
            nb += 1;
        }
    }
    (avg3(fg, nf.max(1)), avg3(bg, nb.max(1)))
}

fn avg3(sum: [u32; 3], n: u32) -> [u8; 3] {
    [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8]
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
        // Exact 4×3-cell pixel grid so fit_on_black does not letterbox.
        let img = solid(200, 40, 40, 8, 6);
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
    fn left_right_split_uses_left_half_glyph() {
        // 1×2 cells = 2×4 dest pixels; paint left column red, right blue
        // so each cell is a clean left/right split (no resize blending).
        let mut rgb = RgbImage::new(2, 4);
        for y in 0..4 {
            rgb.put_pixel(0, y, Rgb([255, 0, 0]));
            rgb.put_pixel(1, y, Rgb([0, 0, 255]));
        }
        let blocks = BlockImage::from_image(&DynamicImage::ImageRgb8(rgb), 1, 2, true);
        for row in 0..2 {
            let (g, fg, bg) = blocks.cell(0, row).unwrap();
            assert_eq!(g, '▌', "cell 0,{row}");
            assert_ne!(fg, bg);
        }
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
