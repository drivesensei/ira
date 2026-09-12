//! Win32 layered popup that sits over the terminal's preview cells.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::sync::{Once, OnceLock};

use image::DynamicImage;
use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HWND, POINT, RECT, SIZE};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::wingdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
};
use winapi::um::winuser::{
    ClientToScreen, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetDC,
    GetForegroundWindow, RegisterClassW, ReleaseDC, ShowWindow, UpdateLayeredWindow, CS_HREDRAW,
    CS_VREDRAW, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::{ScreenRect, WindowGeom};

const CLASS: &str = "IraPreviewOverlay";

pub struct WinOverlay {
    hwnd: Option<HWND>,
    visible: bool,
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
            return Some(hwnd);
        }
        register_class();
        let title = wide("");
        let hwnd = unsafe {
            let instance = GetModuleHandleW(ptr::null());
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST,
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

    pub fn show(&mut self, img: &DynamicImage, rect: ScreenRect) {
        if rect.width == 0 || rect.height == 0 {
            self.hide();
            return;
        }
        let Some(hwnd) = self.ensure() else {
            return;
        };
        if !blit(hwnd, img, rect) {
            self.hide();
            return;
        }
        unsafe {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        self.visible = true;
    }

    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        if let Some(hwnd) = self.hwnd {
            unsafe {
                ShowWindow(hwnd, SW_HIDE);
            }
        }
        self.visible = false;
    }

    pub fn close(&mut self) {
        self.hide();
        if let Some(hwnd) = self.hwnd.take() {
            unsafe {
                DestroyWindow(hwnd);
            }
        }
    }
}

pub fn front_window() -> Option<WindowGeom> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
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
    let w = rect.width;
    let h = rect.height;
    if w == 0 || h == 0 {
        return false;
    }
    let rgba = if img.width() != w || img.height() != h {
        img.resize(w, h, image::imageops::FilterType::Triangle)
            .to_rgba8()
    } else {
        img.to_rgba8()
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
        let dest = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
        for i in 0..(w * h) as usize {
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
