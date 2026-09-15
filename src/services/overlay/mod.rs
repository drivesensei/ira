//! Native image overlay for terminals with no graphics protocol.
//!
//! On macOS Terminal.app the only way to show a real photograph is a GUI
//! surface (Finder does the same). Placement is a pure function of the
//! window frame, the text-area origin, the cell size and a cell rect.

use std::hash::{Hash, Hasher};
#[cfg(any(target_os = "macos", windows))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(any(target_os = "macos", windows))]
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use image::DynamicImage;
use ratatui::layout::Rect;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// Split view can show at most two panes, so at most two overlay surfaces.
pub const OVERLAY_SLOTS: usize = 2;

/// Hard cap on a terminal cell in device pixels (absurd fonts / bad metrics).
pub const MAX_CELL_PX_W: u16 = 256;
pub const MAX_CELL_PX_H: u16 = 512;

/// Hard cap on an overlay bitmap side. Stops `w * h * 4` overflowing
/// into an undersized `from_raw_parts_mut` on Windows.
pub const MAX_OVERLAY_PX: u32 = 4096;

/// How often the background tracker re-reads the terminal window while a
/// preview overlay is needed.
#[cfg(any(target_os = "macos", windows))]
const GEOM_POLL: Duration = Duration::from_millis(300);
/// Backoff while previews are off — no AppleScript / Win32 query at all
/// between idle waits.
#[cfg(any(target_os = "macos", windows))]
const GEOM_IDLE: Duration = Duration::from_millis(1500);
/// A tracker sample older than this is treated as Hidden (dead thread, or
/// polling was off long enough that the last Ready is no longer trustworthy).
pub const GEOM_STALE: Duration = Duration::from_secs(2);

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

/// Result of a cheap (cached) front-window lookup. The OS query itself
/// never runs on the render thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontWindow {
    /// Tracker has not produced a sample yet; leave the overlay as-is.
    Pending,
    /// ira's terminal is not front, or the host is not a known terminal.
    Hidden,
    Ready(WindowGeom),
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

/// Title/tab leftover that still counts as chrome rather than a bad cell height.
pub const MAX_CHROME: u32 = 72;

/// Estimate cell pixels from the window frame and the terminal grid.
/// Width fills the client; leftover height after a 1:2 cell guess is title/tab chrome.
/// Chrome may be zero (conhost / tabs hidden / client-rect hosts).
pub fn cell_px_from_window(window: WindowGeom, cols: u16, rows: u16) -> CellPx {
    let cols = u32::from(cols.max(1));
    let rows = u32::from(rows.max(1));
    let width = (window.width / cols).max(1);
    let chrome = leftover_chrome(window.height, rows, width.saturating_mul(2));
    let height = (window.height.saturating_sub(chrome) / rows).max(1);
    clamp_cell(CellPx {
        width: width.min(u32::from(u16::MAX)) as u16,
        height: height.min(u32::from(u16::MAX)) as u16,
    })
}

fn leftover_chrome(window_h: u32, rows: u32, guessed_cell_h: u32) -> u32 {
    let guessed_text_h = rows.saturating_mul(guessed_cell_h);
    let leftover = window_h.saturating_sub(guessed_text_h);
    leftover.min(MAX_CHROME.min(window_h.saturating_sub(rows)))
}

/// Keep `0 <= window_h - rows*cell_h <= MAX_CHROME`. Undersized cells (font
/// em vs line-box) get grown; oversized cells get shrunk. Width follows the
/// client so the overlay columns line up with the text grid.
pub fn snap_cell_to_window(window: WindowGeom, cols: u16, rows: u16, cell: CellPx) -> CellPx {
    snap_cell(window, cols, rows, cell, true)
}

/// Trust a measured cell height (e.g. `GetConsoleFontSize`). Only shrink when
/// the text grid would overflow the client — do not grow to assume `MAX_CHROME`
/// of title/tab leftover (a ~34 px tab strip would otherwise sit the overlay
/// ~38 px low).
pub fn snap_trusted_cell(window: WindowGeom, cols: u16, rows: u16, cell: CellPx) -> CellPx {
    snap_cell(window, cols, rows, cell, false)
}

fn snap_cell(
    window: WindowGeom,
    cols: u16,
    rows: u16,
    cell: CellPx,
    grow_to_chrome: bool,
) -> CellPx {
    let cols = u32::from(cols.max(1));
    let rows = u32::from(rows.max(1));
    let width = (window.width / cols).max(1);
    let mut height = u32::from(cell.height).max(1);
    let text_h = rows.saturating_mul(height);
    if text_h > window.height {
        height = (window.height / rows).max(1);
    } else if grow_to_chrome && window.height - text_h > MAX_CHROME {
        let target_text = window.height.saturating_sub(MAX_CHROME.min(window.height));
        height = target_text.div_ceil(rows).max(1);
        if rows.saturating_mul(height) > window.height {
            height = (window.height / rows).max(1);
        }
    }
    clamp_cell(CellPx {
        width: width.min(u32::from(u16::MAX)) as u16,
        height: height.min(u32::from(u16::MAX)) as u16,
    })
}

/// Fitted overlay tile size inside a grid cell (fill_percent of the cell).
pub fn grid_tile_px(tile: Rect, cell: CellPx, fill_percent: u32) -> (u32, u32) {
    let tw = u32::from(tile.width) * u32::from(cell.width);
    let th = u32::from(tile.height) * u32::from(cell.height);
    let fill = fill_percent.clamp(1, 100);
    ((tw * fill / 100).max(1), (th * fill / 100).max(1))
}

/// Clamp absurd cell metrics before they feed blit / composite.
pub fn clamp_cell(cell: CellPx) -> CellPx {
    CellPx {
        width: cell.width.clamp(1, MAX_CELL_PX_W),
        height: cell.height.clamp(1, MAX_CELL_PX_H),
    }
}

/// Byte length of a 32-bit overlay bitmap, or `None` if it cannot be
/// addressed. Caps each side at [`MAX_OVERLAY_PX`].
pub fn overlay_bitmap_bytes(w: u32, h: u32) -> Option<usize> {
    let w = w.min(MAX_OVERLAY_PX) as usize;
    let h = h.min(MAX_OVERLAY_PX) as usize;
    w.checked_mul(h)?.checked_mul(4)
}

/// Fit `img` into `w × h` pixels, aspect preserved, centered on transparent.
/// Exact dest size is a clone (composite memcpy path). Smaller sources are
/// upscaled with Triangle so a 256 px overlay store still fills a column.
pub fn fit_pixels(img: &DynamicImage, w: u32, h: u32) -> DynamicImage {
    if w == 0 || h == 0 {
        return DynamicImage::new_rgba8(1, 1);
    }
    if img.width() == w && img.height() == h {
        return img.clone();
    }
    let src = img
        .resize(w, h, image::imageops::FilterType::Triangle)
        .to_rgba8();
    if src.width() == w && src.height() == h {
        return DynamicImage::ImageRgba8(src);
    }
    let mut canvas = image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 0]));
    let nw = src.width().min(w);
    let nh = src.height().min(h);
    let ox = (w - nw) / 2;
    let oy = (h - nh) / 2;
    image::imageops::overlay(&mut canvas, &src, ox.into(), oy.into());
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
        // Already-fitted tiles (cached at dest size) are a memcpy onto the canvas.
        let fitted = if img.width() == fw && img.height() == fh {
            img.to_rgba8()
        } else {
            fit_pixels(img, fw, fh).to_rgba8()
        };
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

/// Hash of which thumbs are drawn (not where the terminal window sits).
pub fn content_hash(cell: CellPx, term: (u16, u16), area: Rect, tiles: &[(Rect, &str)]) -> u64 {
    placement_hash(
        WindowGeom {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        },
        cell,
        term,
        area,
        tiles,
    )
}

struct OverlaySlot {
    #[cfg(target_os = "macos")]
    inner: macos::MacOverlay,
    #[cfg(windows)]
    inner: windows::WinOverlay,
    shown_fp: Option<u64>,
    content_fp: Option<u64>,
    #[cfg(target_os = "macos")]
    png: Option<(u64, Vec<u8>)>,
    visible: bool,
}

impl OverlaySlot {
    fn new() -> Self {
        Self {
            #[cfg(target_os = "macos")]
            inner: macos::MacOverlay::new(),
            #[cfg(windows)]
            inner: windows::WinOverlay::new(),
            shown_fp: None,
            content_fp: None,
            #[cfg(target_os = "macos")]
            png: None,
            visible: false,
        }
    }

    fn is_current(&self, fp: u64) -> bool {
        self.visible && self.shown_fp == Some(fp)
    }

    fn same_content(&self, fp: u64) -> bool {
        self.content_fp == Some(fp)
    }

    fn mark_current(&mut self, place_fp: u64, content_fp: u64) {
        self.shown_fp = Some(place_fp);
        self.content_fp = Some(content_fp);
    }

    fn show(&mut self, img: &DynamicImage, rect: ScreenRect, content_fp: u64) {
        #[cfg(target_os = "macos")]
        {
            if self.png.as_ref().map(|(fp, _)| *fp) != Some(content_fp) {
                self.png = Some((content_fp, macos::encode_png(img)));
            }
            let png = self.png.as_ref().map(|(_, b)| b.as_slice()).unwrap_or(&[]);
            self.inner.show_png(png, rect);
            self.visible = self.inner.visible;
        }
        #[cfg(windows)]
        {
            let _ = content_fp;
            self.inner.show(img, rect);
            self.visible = self.inner.visible;
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        let _ = (img, rect, content_fp);
    }

    fn move_to(&mut self, rect: ScreenRect) {
        #[cfg(any(target_os = "macos", windows))]
        {
            self.inner.move_to(rect);
            self.visible = self.inner.visible;
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        let _ = rect;
    }

    /// Transient hide: order out, keep the bitmap so a later show is `move_to`.
    fn suspend(&mut self) {
        if !self.visible && self.shown_fp.is_none() {
            return;
        }
        self.shown_fp = None;
        #[cfg(any(target_os = "macos", windows))]
        self.inner.order_out();
        self.visible = false;
    }

    fn hide(&mut self) {
        if !self.visible && self.shown_fp.is_none() && self.content_fp.is_none() {
            return;
        }
        self.shown_fp = None;
        self.content_fp = None;
        #[cfg(target_os = "macos")]
        {
            self.png = None;
        }
        #[cfg(any(target_os = "macos", windows))]
        self.inner.hide();
        self.visible = false;
    }

    fn close(&mut self) {
        self.shown_fp = None;
        self.content_fp = None;
        #[cfg(target_os = "macos")]
        {
            self.png = None;
        }
        #[cfg(any(target_os = "macos", windows))]
        self.inner.close();
        self.visible = false;
    }
}

/// Native overlay window. No-op except on macOS and Windows.
pub struct Overlay {
    slots: [OverlaySlot; OVERLAY_SLOTS],
}

impl Overlay {
    pub fn new() -> Self {
        Self {
            slots: std::array::from_fn(|_| OverlaySlot::new()),
        }
    }

    pub fn is_current(&self, slot: usize, fp: u64) -> bool {
        self.slots.get(slot).is_some_and(|s| s.is_current(fp))
    }

    pub fn is_visible(&self, slot: usize) -> bool {
        self.slots.get(slot).is_some_and(|s| s.visible)
    }

    pub fn same_content(&self, slot: usize, fp: u64) -> bool {
        self.slots.get(slot).is_some_and(|s| s.same_content(fp))
    }

    pub fn mark_current(&mut self, slot: usize, place_fp: u64, content_fp: u64) {
        if let Some(s) = self.slots.get_mut(slot) {
            s.mark_current(place_fp, content_fp);
        }
    }

    pub fn show(&mut self, slot: usize, img: &DynamicImage, rect: ScreenRect, content_fp: u64) {
        if let Some(s) = self.slots.get_mut(slot) {
            s.show(img, rect, content_fp);
        }
    }

    pub fn move_to(&mut self, slot: usize, rect: ScreenRect) {
        if let Some(s) = self.slots.get_mut(slot) {
            s.move_to(rect);
        }
    }

    pub fn hide_slot(&mut self, slot: usize) {
        if let Some(s) = self.slots.get_mut(slot) {
            s.hide();
        }
    }

    pub fn suspend_all(&mut self) {
        for s in &mut self.slots {
            s.suspend();
        }
    }

    pub fn hide(&mut self) {
        self.hide_all();
    }

    pub fn hide_all(&mut self) {
        for s in &mut self.slots {
            s.hide();
        }
    }

    pub fn close(&mut self) {
        for s in &mut self.slots {
            s.close();
        }
    }
}

impl Default for Overlay {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(target_os = "macos", windows))]
struct GeomSample {
    state: FrontWindow,
    at: Instant,
    /// Last Ready geometry with the time it was sampled. Used when the sample
    /// is Pending so a dialog close can resume the overlay this frame instead
    /// of waiting for the next poll + draw — but only while it is fresh, so a
    /// window moved during the pause is never trusted.
    last_ready: Option<(WindowGeom, Instant)>,
}

#[cfg(any(target_os = "macos", windows))]
struct GeomTracker {
    sample: Mutex<GeomSample>,
    tick: Mutex<()>,
    cv: Condvar,
    wanted: AtomicBool,
    running: Mutex<Option<std::thread::JoinHandle<()>>>,
}

#[cfg(any(target_os = "macos", windows))]
fn geom_tracker() -> &'static GeomTracker {
    static TRACKER: OnceLock<GeomTracker> = OnceLock::new();
    TRACKER.get_or_init(|| GeomTracker {
        sample: Mutex::new(GeomSample {
            state: FrontWindow::Pending,
            at: Instant::now(),
            last_ready: None,
        }),
        tick: Mutex::new(()),
        cv: Condvar::new(),
        wanted: AtomicBool::new(false),
        running: Mutex::new(None),
    })
}

/// What [`front_window`] should report for a cached sample of a given age.
/// Stale Ready/Pending become Hidden so a dead tracker cannot leave a
/// floating overlay; a freshly stamped Pending is left as-is.
pub fn classify_geom_sample(state: FrontWindow, age: Duration) -> FrontWindow {
    if age > GEOM_STALE {
        FrontWindow::Hidden
    } else {
        state
    }
}

/// When the tracker has stamped Pending (wanted just went true), reuse the
/// last Ready geometry so the overlay can resume this frame. Only while that
/// sample is fresher than [`GEOM_STALE`]: a window moved or resized during the
/// pause must not place the overlay at the old rectangle.
pub fn pending_uses_last_ready(
    classified: FrontWindow,
    last_ready: Option<(WindowGeom, Instant)>,
) -> FrontWindow {
    match classified {
        FrontWindow::Pending => last_ready
            .filter(|(_, at)| at.elapsed() <= GEOM_STALE)
            .map(|(w, _)| FrontWindow::Ready(w))
            .unwrap_or(classified),
        other => other,
    }
}

/// On wanted false→true, replace the sample with Pending and a fresh
/// timestamp so a 2 s-old Ready is not reported as Hidden on the first
/// resume frame (Pending means "leave the overlay as-is").
pub fn apply_geom_wanted_transition(
    was: bool,
    wanted: bool,
    state: &mut FrontWindow,
    at: &mut Instant,
) {
    if wanted && !was {
        *state = FrontWindow::Pending;
        *at = Instant::now();
    }
}

/// Copy a Ready sample's geometry into `last_ready`, keeping the instant it
/// was *observed* (`at`) — never `Instant::now()`, which would re-stamp an
/// ancient geometry as fresh and defeat the staleness bound in
/// [`pending_uses_last_ready`].
pub fn promote_last_ready(
    state: &FrontWindow,
    at: Instant,
    last_ready: &mut Option<(WindowGeom, Instant)>,
) {
    if let FrontWindow::Ready(w) = state {
        *last_ready = Some((*w, at));
    }
}

/// Poll geometry only while a native overlay is needed; idle otherwise.
pub fn set_geom_wanted(wanted: bool) {
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = wanted;
    }
    #[cfg(any(target_os = "macos", windows))]
    {
        let t = geom_tracker();
        let was = t.wanted.swap(wanted, Ordering::Relaxed);
        if wanted && !was {
            if let Ok(mut g) = t.sample.lock() {
                let sample = &mut *g;
                let (state, at) = (sample.state, sample.at);
                promote_last_ready(&state, at, &mut sample.last_ready);
                apply_geom_wanted_transition(was, wanted, &mut sample.state, &mut sample.at);
            }
            t.cv.notify_one();
        }
        if wanted {
            ensure_geom_tracker();
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
fn ensure_geom_tracker() {
    let t = geom_tracker();
    let mut running = t.running.lock().unwrap_or_else(|e| e.into_inner());
    if running.as_ref().is_some_and(|h| !h.is_finished()) {
        return;
    }
    let handle = std::thread::Builder::new()
        .name("ira-overlay-geom".into())
        .spawn(geom_tracker_loop)
        .ok();
    *running = handle;
}

#[cfg(any(target_os = "macos", windows))]
fn geom_tracker_loop() {
    let t = geom_tracker();
    loop {
        let wanted = t.wanted.load(Ordering::Relaxed);
        if wanted {
            let next = match query_front_window() {
                Some(w) => FrontWindow::Ready(w),
                None => FrontWindow::Hidden,
            };
            if let Ok(mut g) = t.sample.lock() {
                let now = Instant::now();
                promote_last_ready(&next, now, &mut g.last_ready);
                g.state = next;
                g.at = now;
            }
            if let Ok(guard) = t.tick.lock() {
                let _ = t.cv.wait_timeout(guard, GEOM_POLL);
            } else {
                std::thread::sleep(GEOM_POLL);
            }
        } else if let Ok(guard) = t.tick.lock() {
            let _ = t.cv.wait_timeout(guard, GEOM_IDLE);
        } else {
            std::thread::sleep(GEOM_IDLE);
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
fn query_front_window() -> Option<WindowGeom> {
    #[cfg(target_os = "macos")]
    {
        return macos::query_front_window();
    }
    #[cfg(windows)]
    {
        return windows::query_front_window();
    }
}

/// Frontmost (or tty-matched) terminal window, from the background cache.
/// Never forks `osascript` / talks to the window server on the caller.
pub fn front_window() -> FrontWindow {
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        return FrontWindow::Hidden;
    }
    #[cfg(any(target_os = "macos", windows))]
    {
        ensure_geom_tracker();
        let t = geom_tracker();
        let Ok(g) = t.sample.lock() else {
            return FrontWindow::Hidden;
        };
        let classified = classify_geom_sample(g.state, g.at.elapsed());
        pending_uses_last_ready(classified, g.last_ready)
    }
}

/// Best available cell size: ioctl (non-macOS Unix), Win32 font metrics,
/// then the window-frame guess. macOS skips ioctl so Retina device-pixels
/// are never mixed with AppleScript points. AppleScript / Win32 window
/// bounds include title-bar chrome, so the result is always snapped.
pub fn resolve_cell_px(window: WindowGeom, cols: u16, rows: u16) -> CellPx {
    #[cfg(windows)]
    {
        // GetConsoleFontSize is the real cell; don't grow it to assume 72 px
        // of chrome. WT settings.json is an estimate and still uses the
        // leftover-chrome snap.
        if let Some(cell) = windows::console_font_cell_px() {
            return snap_trusted_cell(window, cols, rows, cell);
        }
        let cell = windows::wt_settings_cell_px()
            .unwrap_or_else(|| cell_px_from_window(window, cols, rows));
        return snap_cell_to_window(window, cols, rows, cell);
    }
    #[cfg(not(windows))]
    {
        let cell = ioctl_cell_px()
            .filter(|c| c.width > 0 && c.height > 0)
            .map(clamp_cell)
            .unwrap_or_else(|| cell_px_from_window(window, cols, rows));
        snap_cell_to_window(window, cols, rows, cell)
    }
}

/// Cell size from `TIOCGWINSZ` when the kernel fills pixel fields.
/// Unused on macOS: those fields are device pixels while AppleScript
/// bounds are points, and mixing them doubles every overlay offset on Retina.
pub fn ioctl_cell_px() -> Option<CellPx> {
    #[cfg(all(unix, not(target_os = "macos")))]
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
    #[cfg(not(all(unix, not(target_os = "macos"))))]
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

    #[test]
    fn content_hash_ignores_window_origin() {
        let area = Rect {
            x: 10,
            y: 2,
            width: 8,
            height: 6,
        };
        let cell = CellPx {
            width: 10,
            height: 20,
        };
        let tiles = [(area, "img")];
        let a = content_hash(cell, (80, 24), area, &tiles);
        let b = placement_hash(
            WindowGeom {
                x: 50,
                y: 80,
                width: 800,
                height: 600,
            },
            cell,
            (80, 24),
            area,
            &tiles,
        );
        let c = placement_hash(
            WindowGeom {
                x: 0,
                y: 0,
                width: 800,
                height: 600,
            },
            cell,
            (80, 24),
            area,
            &tiles,
        );
        assert_eq!(a, content_hash(cell, (80, 24), area, &tiles));
        assert_ne!(b, c);
    }

    #[test]
    fn cell_px_allows_zero_chrome() {
        let exact = WindowGeom {
            x: 0,
            y: 0,
            width: 800,
            height: 500,
        };
        let c = cell_px_from_window(exact, 80, 25);
        assert_eq!(c.width, 10);
        assert_eq!(c.height, 20);
    }

    #[test]
    fn fit_pixels_exact_size_skips_resize() {
        let src = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            4,
            4,
            image::Rgba([1, 2, 3, 255]),
        ));
        let out = fit_pixels(&src, 4, 4);
        assert_eq!(out.width(), 4);
        assert_eq!(out.height(), 4);
        assert_eq!(out.to_rgba8().get_pixel(0, 0), &image::Rgba([1, 2, 3, 255]));
    }

    #[test]
    fn fit_pixels_upscales_smaller_source() {
        let src = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            4,
            4,
            image::Rgba([1, 2, 3, 255]),
        ));
        // Square source fills a square dest (aspect 1:1).
        let filled = fit_pixels(&src, 8, 8);
        assert_eq!(filled.width(), 8);
        assert_eq!(filled.height(), 8);
        assert_eq!(
            filled.to_rgba8().get_pixel(0, 0),
            &image::Rgba([1, 2, 3, 255])
        );
        assert_eq!(
            filled.to_rgba8().get_pixel(7, 7),
            &image::Rgba([1, 2, 3, 255])
        );
        // Wide source: resize into dest then center on the unused axis.
        let wide = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            4,
            2,
            image::Rgba([1, 2, 3, 255]),
        ));
        let padded = fit_pixels(&wide, 8, 8);
        assert_eq!(padded.width(), 8);
        assert_eq!(padded.height(), 8);
        assert_eq!(
            padded.to_rgba8().get_pixel(0, 0),
            &image::Rgba([0, 0, 0, 0])
        );
        assert_eq!(
            padded.to_rgba8().get_pixel(0, 2),
            &image::Rgba([1, 2, 3, 255])
        );
    }

    #[test]
    fn fit_pixels_column_store_fills_dest_width() {
        // Stored overlay bitmaps cap at 256 px; a 40-col preview is ~280×320.
        let src = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            256,
            192,
            image::Rgba([9, 8, 7, 255]),
        ));
        let out = fit_pixels(&src, 280, 320);
        assert_eq!(out.width(), 280);
        assert_eq!(out.height(), 320);
        // 256×192 into 280×320 is width-limited → 280×210, then vertical pad.
        assert_eq!(out.to_rgba8().get_pixel(0, 0), &image::Rgba([0, 0, 0, 0]));
        assert_eq!(
            out.to_rgba8().get_pixel(0, 55),
            &image::Rgba([9, 8, 7, 255])
        );
    }

    #[test]
    fn overlay_bitmap_bytes_clamps_and_checks() {
        assert_eq!(overlay_bitmap_bytes(0, 0), Some(0));
        assert_eq!(overlay_bitmap_bytes(2, 3), Some(24));
        let huge = overlay_bitmap_bytes(u32::MAX, u32::MAX).unwrap();
        assert_eq!(
            huge,
            (MAX_OVERLAY_PX as usize) * (MAX_OVERLAY_PX as usize) * 4
        );
    }

    #[test]
    fn clamp_cell_caps_absurd_metrics() {
        let c = clamp_cell(CellPx {
            width: 4000,
            height: 4000,
        });
        assert_eq!(c.width, MAX_CELL_PX_W);
        assert_eq!(c.height, MAX_CELL_PX_H);
    }

    #[test]
    fn hide_is_idempotent() {
        let mut overlay = Overlay::new();
        overlay.hide();
        overlay.hide();
        overlay.suspend_all();
    }

    #[test]
    fn suspend_clears_current_but_keeps_content() {
        let mut overlay = Overlay::new();
        overlay.mark_current(0, 11, 22);
        overlay.suspend_all();
        assert!(!overlay.is_current(0, 11));
        assert!(overlay.same_content(0, 22));
        assert!(!overlay.is_visible(0));
    }

    #[test]
    fn snap_cell_grows_undersized_height() {
        let window = WindowGeom {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        };
        // 10×16 over 80×25 leaves 200 px — more than MAX_CHROME.
        let small = CellPx {
            width: 10,
            height: 16,
        };
        let snapped = snap_cell_to_window(window, 80, 25, small);
        assert_eq!(snapped.width, 10);
        let leftover = 600u32.saturating_sub(25 * u32::from(snapped.height));
        assert!(leftover <= MAX_CHROME, "leftover {leftover}");
    }

    #[test]
    fn snap_cell_keeps_plausible_chrome() {
        let window = WindowGeom {
            x: 0,
            y: 0,
            width: 800,
            height: 534,
        };
        let cell = CellPx {
            width: 10,
            height: 20,
        };
        // 25×20 = 500; leftover 34 is inside MAX_CHROME.
        let snapped = snap_cell_to_window(window, 80, 25, cell);
        assert_eq!(snapped.height, 20);
    }

    #[test]
    fn snap_trusted_cell_keeps_measured_height() {
        let window = WindowGeom {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
        };
        // 25×16 = 400; leftover 200 would grow under snap_cell_to_window.
        let measured = CellPx {
            width: 10,
            height: 16,
        };
        let trusted = snap_trusted_cell(window, 80, 25, measured);
        assert_eq!(trusted.height, 16);
        let grown = snap_cell_to_window(window, 80, 25, measured);
        assert!(grown.height > 16);
        // Overflow still shrinks.
        let tall = CellPx {
            width: 10,
            height: 40,
        };
        let shrunk = snap_trusted_cell(window, 80, 25, tall);
        assert!(u32::from(shrunk.height) * 25 <= 600);
    }

    #[test]
    fn stale_geom_sample_is_hidden_pending_resume_is_not() {
        let ready = FrontWindow::Ready(WindowGeom {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        });
        assert_eq!(
            classify_geom_sample(ready, Duration::from_secs(3)),
            FrontWindow::Hidden
        );
        assert_eq!(
            classify_geom_sample(ready, Duration::from_millis(100)),
            ready
        );
        assert_eq!(
            classify_geom_sample(FrontWindow::Pending, Duration::ZERO),
            FrontWindow::Pending
        );
        assert_eq!(
            classify_geom_sample(FrontWindow::Pending, Duration::from_secs(3)),
            FrontWindow::Hidden
        );

        let mut state = ready;
        let mut at = std::time::Instant::now() - Duration::from_secs(5);
        apply_geom_wanted_transition(false, true, &mut state, &mut at);
        assert_eq!(state, FrontWindow::Pending);
        assert_eq!(
            classify_geom_sample(state, at.elapsed()),
            FrontWindow::Pending
        );
        // Already-wanted keeps the aged sample (dead-thread → Hidden).
        let mut keep = ready;
        let mut keep_at = std::time::Instant::now() - Duration::from_secs(5);
        apply_geom_wanted_transition(true, true, &mut keep, &mut keep_at);
        assert_eq!(keep, ready);
        assert_eq!(
            classify_geom_sample(keep, keep_at.elapsed()),
            FrontWindow::Hidden
        );

        let last = WindowGeom {
            x: 10,
            y: 20,
            width: 800,
            height: 600,
        };
        assert_eq!(
            pending_uses_last_ready(
                FrontWindow::Pending,
                Some((last, std::time::Instant::now()))
            ),
            FrontWindow::Ready(last)
        );
        assert_eq!(
            pending_uses_last_ready(FrontWindow::Pending, None),
            FrontWindow::Pending
        );
        assert_eq!(
            pending_uses_last_ready(FrontWindow::Hidden, Some((last, std::time::Instant::now()))),
            FrontWindow::Hidden
        );
        // A geometry older than GEOM_STALE is never reused: the window may
        // have moved while the overlay was suspended.
        assert_eq!(
            pending_uses_last_ready(
                FrontWindow::Pending,
                Some((last, std::time::Instant::now() - Duration::from_secs(30)))
            ),
            FrontWindow::Pending
        );

        // ...and promoting a sample must keep its observation time: stamping
        // `now` here would make an ancient geometry look fresh and defeat the
        // bound above (the exact bug this helper replaced).
        let mut promoted = None;
        let old = std::time::Instant::now() - Duration::from_secs(30);
        promote_last_ready(&FrontWindow::Ready(last), old, &mut promoted);
        assert_eq!(
            pending_uses_last_ready(FrontWindow::Pending, promoted),
            FrontWindow::Pending,
            "a stale Ready must not resume the overlay at the old rect"
        );
        // A fresh sample is promoted as before.
        let fresh = std::time::Instant::now();
        promote_last_ready(&FrontWindow::Ready(last), fresh, &mut promoted);
        assert_eq!(
            pending_uses_last_ready(FrontWindow::Pending, promoted),
            FrontWindow::Ready(last)
        );
    }

    #[test]
    fn composite_grid_reuses_exact_size_tiles() {
        let tile = ratatui::layout::Rect::new(0, 0, 2, 2);
        let area = ratatui::layout::Rect::new(0, 0, 2, 2);
        let cell = CellPx {
            width: 10,
            height: 10,
        };
        let (fw, fh) = grid_tile_px(tile, cell, GRID_FILL_PERCENT);
        let src = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            4,
            4,
            image::Rgba([1, 2, 3, 255]),
        ));
        let fitted = fit_pixels(&src, fw, fh);
        assert_eq!(fitted.width(), fw);
        assert_eq!(fitted.height(), fh);
        let again = fit_pixels(&fitted, fw, fh);
        assert_eq!(again.width(), fw);
        assert_eq!(again.height(), fh);
        let composed = composite_grid(area, cell, &[(tile, &fitted)], GRID_FILL_PERCENT).unwrap();
        assert_eq!(composed.width(), 20);
        assert_eq!(composed.height(), 20);
    }
}
