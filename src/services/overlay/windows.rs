//! Win32 layered popup that sits over the terminal's preview cells.

use std::ffi::OsStr;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::sync::{Once, OnceLock};

use image::DynamicImage;
use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::ntdef::HANDLE;
use winapi::shared::windef::{HWND, POINT, RECT, SIZE};
use winapi::um::handleapi::INVALID_HANDLE_VALUE;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::processenv::GetStdHandle;
use winapi::um::winbase::STD_OUTPUT_HANDLE;
use winapi::um::wincon::{
    GetConsoleFontSize, GetConsoleWindow, GetCurrentConsoleFont, CONSOLE_FONT_INFO,
};
use winapi::um::wingdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDeviceCaps, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    LOGPIXELSY,
};
use winapi::um::winuser::{
    ClientToScreen, CreateWindowExW, DefWindowProcW, DestroyWindow, GetAncestor, GetClassNameW,
    GetClientRect, GetDC, GetDpiForWindow, GetForegroundWindow, GetWindow, IsWindow,
    IsWindowVisible, RegisterClassW, ReleaseDC, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    UpdateLayeredWindow, CS_HREDRAW, CS_VREDRAW, GA_ROOT, GWLP_HWNDPARENT, GW_OWNER, HWND_TOP,
    SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::{overlay_bitmap_bytes, CellPx, ScreenRect, WindowGeom, MAX_OVERLAY_PX};

const CLASS: &str = "IraPreviewOverlay";

pub struct WinOverlay {
    hwnd: Option<HWND>,
    pub visible: bool,
}

impl WinOverlay {
    pub fn new() -> Self {
        Self {
            hwnd: None,
            visible: false,
        }
    }

    fn ensure(&mut self) -> Option<HWND> {
        if let Some(hwnd) = self.hwnd {
            // The handle is only good while the window exists. Our popup is
            // *owned* by the terminal, so Windows destroys it with its owner —
            // and handle values are recycled, so a stale one can name an
            // unrelated window of another process.
            if unsafe { IsWindow(hwnd) } != 0 {
                return Some(hwnd);
            }
            self.hwnd = None;
            self.visible = false;
        }
        register_class();
        let title = wide("");
        let hwnd = unsafe {
            let instance = GetModuleHandleW(ptr::null());
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                class_wide().as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                0,
                0,
                8,
                8,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null_mut(),
            )
        };
        if hwnd.is_null() {
            return None;
        }
        self.hwnd = Some(hwnd);
        Some(hwnd)
    }

    /// Show `img`. Returns whether the popup is on screen afterwards, so the
    /// caller never records a placement it did not get.
    pub fn show(&mut self, img: &DynamicImage, rect: ScreenRect) -> bool {
        if rect.width == 0 || rect.height == 0 {
            self.hide();
            return false;
        }
        // Re-assert frontness where the side effect happens: the geometry
        // sample this rect came from can be up to GEOM_STALE old, and an
        // alt-tab inside that window must not leave the popup over the app
        // that is now in front.
        let Some(host) = foreground_host() else {
            self.order_out();
            return false;
        };
        let Some(hwnd) = self.ensure() else {
            self.order_out();
            return false;
        };
        if !blit(hwnd, img, rect) {
            self.hide();
            return false;
        }
        unsafe {
            attach_to_terminal(hwnd, host);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        self.visible = true;
        true
    }

    /// Reposition, creating the popup if an earlier show never managed to.
    /// Returns whether the popup is on screen afterwards.
    pub fn move_to(&mut self, rect: ScreenRect) -> bool {
        if rect.width == 0 || rect.height == 0 {
            self.order_out();
            return false;
        }
        let Some(host) = foreground_host() else {
            self.order_out();
            return false;
        };
        let Some(hwnd) = self.ensure() else {
            self.order_out();
            return false;
        };
        unsafe {
            attach_to_terminal(hwnd, host);
            SetWindowPos(
                hwnd,
                HWND_TOP,
                rect.x,
                rect.y,
                rect.width.min(MAX_OVERLAY_PX) as i32,
                rect.height.min(MAX_OVERLAY_PX) as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
            if !self.visible {
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                self.visible = true;
            }
        }
        self.visible
    }

    pub fn order_out(&mut self) {
        if !self.visible {
            return;
        }
        match self.hwnd {
            Some(hwnd) if unsafe { IsWindow(hwnd) } != 0 => unsafe {
                ShowWindow(hwnd, SW_HIDE);
            },
            // Owner died: our popup went with it.
            Some(_) => self.hwnd = None,
            None => {}
        }
        self.visible = false;
    }

    pub fn hide(&mut self) {
        self.order_out();
    }

    pub fn close(&mut self) {
        self.hide();
        if let Some(hwnd) = self.hwnd.take() {
            if unsafe { IsWindow(hwnd) } != 0 {
                unsafe {
                    DestroyWindow(hwnd);
                }
            }
        }
    }
}

fn is_terminal_hwnd(hwnd: HWND) -> bool {
    if hwnd.is_null() {
        return false;
    }
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if n <= 0 {
        return false;
    }
    let class = String::from_utf16_lossy(&buf[..n as usize]);
    class == "CASCADIA_HOSTING_WINDOW_CLASS" || class == "ConsoleWindowClass"
}

fn is_visible_terminal(hwnd: HWND) -> bool {
    unsafe { is_terminal_hwnd(hwnd) && IsWindowVisible(hwnd) != 0 }
}

/// Prefer the visible terminal that owns our console. Under ConPTY the
/// console HWND is *owned* by Windows Terminal, not parented, and is often
/// hidden — GA_ROOT of that window is the wrong host.
fn our_host_hwnd() -> HWND {
    unsafe {
        let fg = GetForegroundWindow();
        let con = GetConsoleWindow();
        let owned = if !con.is_null() {
            let owner = GetWindow(con, GW_OWNER);
            if is_visible_terminal(owner) {
                owner
            } else {
                let root = GetAncestor(con, GA_ROOT);
                if is_visible_terminal(root) {
                    root
                } else {
                    ptr::null_mut()
                }
            }
        } else {
            ptr::null_mut()
        };
        if !owned.is_null() {
            return if fg == owned { owned } else { ptr::null_mut() };
        }
        // Could not correlate (ConPTY): accept a foreground terminal, which
        // is the 0.1.15 behaviour that actually painted on Windows Terminal.
        if is_visible_terminal(fg) {
            fg
        } else {
            ptr::null_mut()
        }
    }
}

unsafe fn attach_to_terminal(overlay: HWND, host: HWND) {
    if host.is_null() {
        return;
    }
    SetWindowLongPtrW(overlay, GWLP_HWNDPARENT, host as isize);
}

/// Our host terminal window, but only while it is actually frontmost.
/// `None` means "do not paint": the terminal is not in front, or its console
/// cannot be correlated with a terminal window at all.
fn foreground_host() -> Option<HWND> {
    unsafe {
        let host = our_host_hwnd();
        if host.is_null() || GetForegroundWindow() != host {
            return None;
        }
        Some(host)
    }
}

pub fn query_front_window() -> Option<WindowGeom> {
    unsafe {
        // Only paint while this host is actually front — otherwise the
        // overlay floats over whatever replaced the terminal.
        let hwnd = foreground_host()?;
        let mut rc = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetClientRect(hwnd, &mut rc) == 0 {
            return None;
        }
        let mut pt = POINT {
            x: rc.left,
            y: rc.top,
        };
        if ClientToScreen(hwnd, &mut pt) == 0 {
            return None;
        }
        Some(WindowGeom {
            x: pt.x,
            y: pt.y,
            width: (rc.right - rc.left).max(0) as u32,
            height: (rc.bottom - rc.top).max(0) as u32,
        })
    }
}

pub fn console_font_cell_px() -> Option<CellPx> {
    unsafe {
        let h: HANDLE = GetStdHandle(STD_OUTPUT_HANDLE);
        if h.is_null() || h == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut info: CONSOLE_FONT_INFO = mem::zeroed();
        if GetCurrentConsoleFont(h, 0, &mut info) == 0 {
            return None;
        }
        let size = GetConsoleFontSize(h, info.nFont);
        if size.X <= 0 || size.Y <= 0 {
            return None;
        }
        Some(CellPx {
            width: size.X as u16,
            height: size.Y as u16,
        })
    }
}

fn host_dpi() -> u32 {
    unsafe {
        let hwnd = our_host_hwnd();
        if !hwnd.is_null() {
            let dpi = GetDpiForWindow(hwnd);
            if dpi > 0 {
                return dpi;
            }
        }
        let hdc = GetDC(ptr::null_mut());
        if hdc.is_null() {
            return 96;
        }
        let dpi = GetDeviceCaps(hdc, LOGPIXELSY);
        ReleaseDC(ptr::null_mut(), hdc);
        if dpi > 0 {
            dpi as u32
        } else {
            96
        }
    }
}

pub fn wt_settings_cell_px() -> Option<CellPx> {
    let profile = std::env::var("WT_PROFILE_ID").ok();
    let size = crate::theme::wt_font::windows_terminal_font_size(profile.as_deref())?;
    if size <= 0.0 {
        return None;
    }
    // `font.size` is the em. Default line box ~1.2 em; `font.lineHeight` /
    // `font.cellHeight` override when they are a simple number or percent.
    // Advance ~0.6 em. snap_cell_to_window then bounds leftover chrome.
    let line =
        crate::theme::wt_font::windows_terminal_line_height(profile.as_deref()).unwrap_or(1.2);
    let em = size * f64::from(host_dpi()) / 72.0;
    let height = (em * line).round() as u32;
    let width = (em * 0.6).round() as u32;
    Some(CellPx {
        width: width.clamp(1, 256) as u16,
        height: height.clamp(1, 512) as u16,
    })
}

fn register_class() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: ptr::null_mut(),
            hCursor: ptr::null_mut(),
            hbrBackground: ptr::null_mut(),
            lpszMenuName: ptr::null(),
            lpszClassName: class_wide().as_ptr(),
        };
        RegisterClassW(&wc);
    });
}

fn class_wide() -> &'static [u16] {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    NAME.get_or_init(|| wide(CLASS))
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn blit(hwnd: HWND, img: &DynamicImage, rect: ScreenRect) -> bool {
    let w = rect.width.min(MAX_OVERLAY_PX);
    let h = rect.height.min(MAX_OVERLAY_PX);
    // The surface must be exactly the rect it was placed for: the caller fits
    // or composites at that size (`fit_pixels` / `composite_grid`) and clamps
    // both the same way. Rescaling here would draw an aspect-correct image
    // that no longer lines up with the text cells, so a mismatch means the
    // pane is too large for one surface and braille stays as the underlay.
    if w == 0 || h == 0 || img.width() != w || img.height() != h {
        return false;
    }
    let Some(byte_len) = overlay_bitmap_bytes(w, h) else {
        return false;
    };
    if byte_len == 0 {
        return false;
    }
    // Borrow when the image is already RGBA8 instead of cloning the canvas.
    let owned: image::RgbaImage;
    let rgba: &image::RgbaImage = match img.as_rgba8() {
        Some(rgba) => rgba,
        None => {
            owned = img.to_rgba8();
            &owned
        }
    };
    unsafe {
        let hdc_screen = GetDC(ptr::null_mut());
        if hdc_screen.is_null() {
            return false;
        }
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        if hdc_mem.is_null() {
            ReleaseDC(ptr::null_mut(), hdc_screen);
            return false;
        }
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };
        let mut bits: *mut winapi::ctypes::c_void = ptr::null_mut();
        let hbmp = CreateDIBSection(
            hdc_screen,
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            ptr::null_mut(),
            0,
        );
        if hbmp.is_null() || bits.is_null() {
            DeleteDC(hdc_mem);
            ReleaseDC(ptr::null_mut(), hdc_screen);
            return false;
        }
        let src = rgba.as_raw();
        let dest = std::slice::from_raw_parts_mut(bits as *mut u8, byte_len);
        let pixels = (w as usize).saturating_mul(h as usize);
        if src.len() < pixels.saturating_mul(4) || dest.len() < pixels.saturating_mul(4) {
            DeleteObject(hbmp as _);
            DeleteDC(hdc_mem);
            ReleaseDC(ptr::null_mut(), hdc_screen);
            return false;
        }
        for i in 0..pixels {
            let r = src[i * 4];
            let g = src[i * 4 + 1];
            let b = src[i * 4 + 2];
            let a = src[i * 4 + 3];
            dest[i * 4] = ((b as u16 * a as u16) / 255) as u8;
            dest[i * 4 + 1] = ((g as u16 * a as u16) / 255) as u8;
            dest[i * 4 + 2] = ((r as u16 * a as u16) / 255) as u8;
            dest[i * 4 + 3] = a;
        }
        let old = SelectObject(hdc_mem, hbmp as _);
        let mut dest_pt = POINT {
            x: rect.x,
            y: rect.y,
        };
        let mut size = SIZE {
            cx: w as i32,
            cy: h as i32,
        };
        let mut src_pt = POINT { x: 0, y: 0 };
        let mut blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA,
        };
        let ok = UpdateLayeredWindow(
            hwnd,
            hdc_screen,
            &mut dest_pt,
            &mut size,
            hdc_mem,
            &mut src_pt,
            0,
            &mut blend,
            ULW_ALPHA,
        ) != 0;
        SelectObject(hdc_mem, old);
        DeleteObject(hbmp as _);
        DeleteDC(hdc_mem);
        ReleaseDC(ptr::null_mut(), hdc_screen);
        ok
    }
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
