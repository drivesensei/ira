//! AppKit floating panel that sits over the terminal's preview cells.

use std::io::Cursor;
use std::process::Command;

use image::{DynamicImage, ImageFormat};
use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSEventMask,
    NSFloatingWindowLevel, NSImage, NSImageFrameStyle, NSImageScaling, NSImageView, NSPanel,
    NSScreen, NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{NSData, NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize};

use super::{ScreenRect, WindowGeom};

pub struct MacOverlay {
    panel: Option<Retained<NSPanel>>,
    image_view: Option<Retained<NSImageView>>,
    /// Decoded overlay image with the content fingerprint it was built from.
    /// PNG encoding plus the AppKit decode are the expensive half of a show,
    /// and a re-show of the same content (after `order_out`, or a re-layout)
    /// must not repeat them — nor copy the bytes twice to get there.
    image: Option<(u64, Retained<NSImage>)>,
    pub visible: bool,
}

impl MacOverlay {
    pub fn new() -> Self {
        Self {
            panel: None,
            image_view: None,
            image: None,
            visible: false,
        }
    }

    fn ensure(&mut self) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };

        if self.panel.is_none() {
            let app = NSApplication::sharedApplication(mtm);
            app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

            let style = NSWindowStyleMask::Borderless.union(NSWindowStyleMask::NonactivatingPanel);
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(8.0, 8.0));
            let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
                NSPanel::alloc(mtm),
                frame,
                style,
                NSBackingStoreType::Buffered,
                false,
            );
            panel.setFloatingPanel(true);
            // `ira` is a child CLI process inside the terminal with
            // `ActivationPolicy::Accessory`, so `NSApp` is never the active
            // application (the terminal is). `hidesOnDeactivate = true` would
            // hide the panel immediately on every frame; visibility when the
            // terminal loses focus is managed by `geom_tracker_loop` via
            // `order_out`.
            panel.setHidesOnDeactivate(false);
            panel.setOpaque(false);
            panel.setBackgroundColor(Some(&NSColor::clearColor()));
            panel.setIgnoresMouseEvents(true);
            panel.setHasShadow(false);
            // Above Terminal only while it is front; we hide when it is not.
            panel.setLevel(NSFloatingWindowLevel);
            panel.setCollectionBehavior(
                NSWindowCollectionBehavior::Transient
                    .union(NSWindowCollectionBehavior::IgnoresCycle)
                    .union(NSWindowCollectionBehavior::Stationary),
            );
            // We own the Retained; do not let AppKit release on close.
            unsafe {
                panel.setReleasedWhenClosed(false);
            }

            let view = NSImageView::initWithFrame(NSImageView::alloc(mtm), frame);
            view.setImageScaling(NSImageScaling::ScaleAxesIndependently);
            view.setImageFrameStyle(NSImageFrameStyle::None);
            view.setWantsLayer(true);
            panel.setContentView(Some(&view));

            self.image_view = Some(view);
            self.panel = Some(panel);
        }
        true
    }

    /// Show the overlay for `img` (content fingerprint `fp`). Returns whether
    /// the panel is on screen afterwards: a caller must not record a
    /// placement that never appeared.
    pub fn show_image(&mut self, fp: u64, img: &DynamicImage, rect: ScreenRect) -> bool {
        if rect.width == 0 || rect.height == 0 {
            self.hide();
            return false;
        }
        if !self.ensure() {
            // No panel: never report the previous content as current.
            self.order_out();
            return false;
        }
        let cached = self
            .image
            .as_ref()
            .filter(|(cached_fp, _)| *cached_fp == fp)
            .map(|(_, nsimg)| nsimg.clone());
        let nsimg = match cached {
            Some(nsimg) => nsimg,
            None => {
                let png = encode_png(img);
                let data = NSData::with_bytes(&png);
                let Some(nsimg) = NSImage::initWithData(NSImage::alloc(), &data) else {
                    self.order_out();
                    return false;
                };
                self.image = Some((fp, nsimg.clone()));
                nsimg
            }
        };
        // Express the NSImage logical size in Cocoa points while keeping the
        // full-resolution (2x Retina) bitmap representation intact.
        nsimg.setSize(NSSize::new(rect.width as f64, rect.height as f64));
        let Some(panel) = self.panel.as_ref() else {
            self.order_out();
            return false;
        };
        if let Some(view) = &self.image_view {
            view.setImage(Some(&nsimg));
        }
        let cocoa = to_cocoa_rect(rect);
        panel.setFrame_display(cocoa, true);
        panel.orderFront(None);
        self.pump();
        self.visible = true;
        true
    }

    /// Reposition, creating the panel if an earlier show never managed to.
    /// Returns whether the panel is on screen afterwards.
    pub fn move_to(&mut self, rect: ScreenRect) -> bool {
        if rect.width == 0 || rect.height == 0 {
            self.order_out();
            return false;
        }
        if !self.ensure() {
            self.order_out();
            return false;
        }
        if let Some((_, nsimg)) = &self.image {
            nsimg.setSize(NSSize::new(rect.width as f64, rect.height as f64));
        }
        let Some(panel) = self.panel.as_ref() else {
            self.order_out();
            return false;
        };
        panel.setFrame_display(to_cocoa_rect(rect), true);
        if !self.visible {
            panel.orderFront(None);
            self.visible = true;
            self.pump();
        }
        self.visible
    }

    /// Hide the panel without dropping the image (modal / wrong-window).
    pub fn order_out(&mut self) {
        if !self.visible {
            return;
        }
        if let Some(panel) = &self.panel {
            panel.orderOut(None);
        }
        self.visible = false;
    }

    pub fn hide(&mut self) {
        self.order_out();
        if let Some(view) = &self.image_view {
            view.setImage(None);
        }
        // The bytes are gone with the view; the decoded image would only
        // pin memory.
        self.image = None;
    }

    pub fn close(&mut self) {
        self.hide();
        if let Some(panel) = self.panel.take() {
            panel.close();
        }
        self.image_view = None;
    }

    pub fn pump(&mut self) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        // Non-blocking drain: nil untilDate can wait forever. Bound the
        // loop so a busy queue cannot stall the render thread.
        const MAX_EVENTS: usize = 8;
        let until = NSDate::distantPast();
        for _ in 0..MAX_EVENTS {
            let event = unsafe {
                app.nextEventMatchingMask_untilDate_inMode_dequeue(
                    NSEventMask::Any,
                    Some(&until),
                    NSDefaultRunLoopMode,
                    true,
                )
            };
            match event {
                Some(ev) => app.sendEvent(&ev),
                None => break,
            }
        }
    }
}

pub(super) fn encode_png(img: &DynamicImage) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png);
    buf
}

/// AppleScript / our ScreenRect is top-left y-down; Cocoa is bottom-left y-up.
fn to_cocoa_rect(rect: ScreenRect) -> NSRect {
    let screen_h = primary_screen_height();
    let y = screen_h - rect.y as f64 - rect.height as f64;
    NSRect::new(
        NSPoint::new(rect.x as f64, y),
        NSSize::new(rect.width as f64, rect.height as f64),
    )
}

fn primary_screen_height() -> f64 {
    let Some(mtm) = MainThreadMarker::new() else {
        return 1080.0;
    };
    NSScreen::screens(mtm)
        .iter()
        .next()
        .map(|s| s.frame().size.height)
        .unwrap_or(1080.0)
}

/// Backing pixel scale factor of the primary screen (`2` on Retina Macs, `1` on
/// standard displays). Used to size overlay bitmaps at native Retina density
/// while keeping window/panel placement in 1x Cocoa points.
pub fn backing_scale() -> u16 {
    let Some(mtm) = MainThreadMarker::new() else {
        return 2;
    };
    NSScreen::screens(mtm)
        .iter()
        .next()
        .map(|s| s.backingScaleFactor().round() as u16)
        .unwrap_or(2)
        .clamp(1, 2)
}

fn stdin_tty() -> Option<String> {
    unsafe {
        let p = libc::ttyname(libc::STDIN_FILENO);
        if p.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(p)
            .to_str()
            .ok()
            .map(str::to_string)
    }
}

fn classify_term_program(program: &str) -> &'static str {
    if program.contains("iTerm") {
        "iTerm2"
    } else if program.eq_ignore_ascii_case("ghostty") {
        "Ghostty"
    } else if program == "Apple_Terminal" || program == "Terminal" {
        "Terminal"
    } else if program.contains("Warp") {
        "Warp"
    } else if program.contains("WezTerm") {
        "WezTerm"
    } else if program.eq_ignore_ascii_case("kitty") {
        "kitty"
    } else if program.eq_ignore_ascii_case("alacritty") {
        "Alacritty"
    } else if program.contains("vscode") {
        "Code"
    } else {
        ""
    }
}

fn owner_matches(owner_name: &str, expected: &str) -> bool {
    if !expected.is_empty()
        && owner_name
            .to_ascii_lowercase()
            .contains(&expected.to_ascii_lowercase())
    {
        return true;
    }
    const KNOWN_TERMINALS: &[&str] = &[
        "terminal", "iterm", "ghostty", "kitty", "wezterm", "alacritty", "warp", "stable",
        "rio", "hyper", "tabby", "code", "cursor", "windsurf", "zed",
    ];
    let lower = owner_name.to_ascii_lowercase();
    KNOWN_TERMINALS.iter().any(|t| lower.contains(t))
}

fn is_terminal_app(name: Option<&str>, bundle: Option<&str>, expected: &str) -> bool {
    if let Some(b) = bundle {
        let b_lower = b.to_ascii_lowercase();
        if b_lower.contains("terminal")
            || b_lower.contains("iterm")
            || b_lower.contains("ghostty")
            || b_lower.contains("kitty")
            || b_lower.contains("wezterm")
            || b_lower.contains("alacritty")
            || b_lower.contains("warp")
        {
            return true;
        }
    }
    if let Some(n) = name {
        if owner_matches(n, expected) {
            return true;
        }
    }
    false
}

pub fn query_front_window() -> Option<WindowGeom> {
    let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let expected = classify_term_program(&program);

    let ws = NSWorkspace::sharedWorkspace();
    let front_app = ws.frontmostApplication();
    let front_pid = front_app.as_ref().map(|a| a.processIdentifier());
    let name = front_app
        .as_ref()
        .and_then(|a| a.localizedName().map(|s| s.to_string()));
    let bundle = front_app
        .as_ref()
        .and_then(|a| a.bundleIdentifier().map(|s| s.to_string()));

    // When NSWorkspace reports a frontmost app that is NOT a terminal (e.g.
    // the user switched to Safari or Finder), hide the overlay immediately.
    if front_app.is_some() && !is_terminal_app(name.as_deref(), bundle.as_deref(), expected) {
        return None;
    }

    // Primary path: in-process CoreGraphics window query.
    // Matching by target_pid needs zero permissions and returns exact bounds.
    match cg::query_front_terminal_window(front_pid, expected) {
        cg::CgFrontResult::Frontmost {
            geom,
            same_owner_windows,
        } => {
            // When multiple windows of Terminal.app or iTerm2 are open on
            // screen at the same time, try tty-matching via AppleScript
            // (talking directly to the terminal app, never System Events).
            if same_owner_windows > 1 && matches!(expected, "Terminal" | "iTerm2") {
                let tty =
                    stdin_tty().filter(|t| !t.is_empty() && !t.contains('"') && !t.contains('\\'));
                if let Some(tty) = tty.as_deref() {
                    if let Some(raw) = osascript(&bounds_script(expected, Some(tty))) {
                        if raw.trim() == "HIDE" {
                            return None;
                        }
                        if raw.trim() != "NOTFOUND" {
                            if let Some(parsed) = parse_bounds(&Some(raw)) {
                                return Some(parsed);
                            }
                        }
                    }
                }
            }
            return Some(geom);
        }
        cg::CgFrontResult::Unavailable => {}
    }

    // Fallback: direct AppleScript query to Terminal / iTerm (no System Events).
    if !matches!(expected, "Terminal" | "iTerm2") {
        return None;
    }
    let tty = stdin_tty().filter(|t| !t.is_empty() && !t.contains('"') && !t.contains('\\'));
    let raw = osascript(&bounds_script(expected, tty.as_deref()))?;
    let raw = if raw.trim() == "NOTFOUND" {
        osascript(&bounds_script(expected, None))?
    } else {
        raw
    };
    if raw.trim() == "HIDE" {
        return None;
    }
    parse_bounds(&Some(raw))
}

/// AppleScript for `Terminal` / `iTerm2`, tty-matched when our stdin tty is
/// known. Uses the application's own `frontmost` property instead of
/// `"System Events"` so no Accessibility/System Events TCC permission is
/// required.
fn bounds_script(expected: &str, tty: Option<&str>) -> String {
    if expected == "iTerm2" {
        return iterm_bounds_script(tty);
    }
    terminal_app_bounds_script(tty)
}

fn terminal_app_bounds_script(tty: Option<&str>) -> String {
    let Some(tty) = tty else {
        return r#"tell application "Terminal"
    if not frontmost then return "HIDE"
    if (count of windows) is 0 then return "HIDE"
    return bounds of front window
end tell"#
            .to_string();
    };
    format!(
        r#"tell application "Terminal"
    if not frontmost then return "HIDE"
    set targetTty to "{tty}"
    repeat with w in windows
        try
            if tty of selected tab of w is targetTty then
                if w is front window then
                    return bounds of w
                else
                    return "HIDE"
                end if
            end if
        end try
    end repeat
    return "NOTFOUND"
end tell"#
    )
}

fn iterm_bounds_script(tty: Option<&str>) -> String {
    let Some(tty) = tty else {
        return r#"tell application "iTerm"
    if not frontmost then return "HIDE"
    if (count of windows) is 0 then return "HIDE"
    return bounds of current window
end tell"#
            .to_string();
    };
    format!(
        r#"tell application "iTerm"
    if not frontmost then return "HIDE"
    set targetTty to "{tty}"
    repeat with w in windows
        repeat with t in tabs of w
            repeat with s in sessions of t
                try
                    if tty of s is targetTty then
                        if w is current window then
                            return bounds of w
                        else
                            return "HIDE"
                        end if
                    end if
                end try
            end repeat
        end repeat
    end repeat
    return "NOTFOUND"
end tell"#
    )
}

fn osascript(src: &str) -> Option<String> {
    let out = Command::new("osascript").arg("-e").arg(src).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout)
        .ok()
        .map(|s| s.trim().to_string())
}

/// Terminal.app / iTerm return "l, t, r, b" (top-left origin).
fn parse_bounds(raw: &Option<String>) -> Option<WindowGeom> {
    let raw = raw.as_ref()?;
    let parts: Vec<i32> = raw
        .split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect();
    if parts.len() != 4 {
        return None;
    }
    let (l, t, r, b) = (parts[0], parts[1], parts[2], parts[3]);
    Some(WindowGeom {
        x: l,
        y: t,
        width: (r - l).max(0) as u32,
        height: (b - t).max(0) as u32,
    })
}

mod cg {
    use std::ffi::{c_char, c_void, CStr};

    use super::WindowGeom;

    pub enum CgFrontResult {
        Frontmost {
            geom: WindowGeom,
            same_owner_windows: usize,
        },
        Unavailable,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGSize {
        width: f64,
        height: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    type CFTypeRef = *const c_void;
    type CFArrayRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFNumberRef = *const c_void;
    type CFIndex = isize;

    // kCGWindowListOptionOnScreenOnly (1 << 0) | kCGWindowListExcludeDesktopElements (1 << 4)
    const WINDOW_LIST_OPTIONS: u32 = (1 << 0) | (1 << 4);
    const K_CG_NULL_WINDOW_ID: u32 = 0;
    const K_CF_NUMBER_SINT32_TYPE: i32 = 3;
    const K_CF_NUMBER_FLOAT64_TYPE: i32 = 6;
    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFArrayRef;
        fn CGRectMakeWithDictionaryRepresentation(dict: CFDictionaryRef, rect: *mut CGRect) -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFArrayGetCount(the_array: CFArrayRef) -> CFIndex;
        fn CFArrayGetValueAtIndex(the_array: CFArrayRef, idx: CFIndex) -> CFTypeRef;
        fn CFDictionaryGetValue(the_dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
        fn CFNumberGetValue(
            number: CFNumberRef,
            the_type: i32,
            value_ptr: *mut c_void,
        ) -> bool;
        fn CFStringCreateWithCString(
            alloc: CFTypeRef,
            c_str: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFRelease(cf: CFTypeRef);
    }

    struct CfKey(CFStringRef);

    impl CfKey {
        fn new(s: &CStr) -> Option<Self> {
            let ptr = unsafe {
                CFStringCreateWithCString(std::ptr::null(), s.as_ptr(), K_CF_STRING_ENCODING_UTF8)
            };
            if ptr.is_null() {
                None
            } else {
                Some(Self(ptr))
            }
        }
    }

    impl Drop for CfKey {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn dict_i32(dict: CFDictionaryRef, key: &CfKey) -> Option<i32> {
        let val = unsafe { CFDictionaryGetValue(dict, key.0) };
        if val.is_null() {
            return None;
        }
        let mut out: i32 = 0;
        let ok = unsafe {
            CFNumberGetValue(val, K_CF_NUMBER_SINT32_TYPE, (&mut out as *mut i32).cast())
        };
        ok.then_some(out)
    }

    fn dict_f64(dict: CFDictionaryRef, key: &CfKey) -> Option<f64> {
        let val = unsafe { CFDictionaryGetValue(dict, key.0) };
        if val.is_null() {
            return None;
        }
        let mut out: f64 = 0.0;
        let ok = unsafe {
            CFNumberGetValue(val, K_CF_NUMBER_FLOAT64_TYPE, (&mut out as *mut f64).cast())
        };
        ok.then_some(out)
    }

    fn dict_rect(dict: CFDictionaryRef, key: &CfKey) -> Option<CGRect> {
        let val = unsafe { CFDictionaryGetValue(dict, key.0) };
        if val.is_null() {
            return None;
        }
        let mut rect = CGRect::default();
        let ok = unsafe { CGRectMakeWithDictionaryRepresentation(val, &mut rect) };
        ok.then_some(rect)
    }

    pub fn query_front_terminal_window(
        target_pid: Option<i32>,
        _expected: &str,
    ) -> CgFrontResult {
        let k_layer = match CfKey::new(c"kCGWindowLayer") {
            Some(k) => k,
            None => return CgFrontResult::Unavailable,
        };
        let k_pid = match CfKey::new(c"kCGWindowOwnerPID") {
            Some(k) => k,
            None => return CgFrontResult::Unavailable,
        };
        let k_bounds = match CfKey::new(c"kCGWindowBounds") {
            Some(k) => k,
            None => return CgFrontResult::Unavailable,
        };
        let k_alpha = match CfKey::new(c"kCGWindowAlpha") {
            Some(k) => k,
            None => return CgFrontResult::Unavailable,
        };

        let list = unsafe { CGWindowListCopyWindowInfo(WINDOW_LIST_OPTIONS, K_CG_NULL_WINDOW_ID) };
        if list.is_null() {
            return CgFrontResult::Unavailable;
        }
        let count = unsafe { CFArrayGetCount(list) };
        let self_pid = std::process::id() as i32;

        let mut front: Option<WindowGeom> = None;
        let mut same_owner_windows = 0usize;

        for idx in 0..count {
            let dict = unsafe { CFArrayGetValueAtIndex(list, idx) };
            if dict.is_null() {
                continue;
            }
            if dict_i32(dict, &k_layer) != Some(0) {
                continue;
            }
            let Some(pid) = dict_i32(dict, &k_pid) else {
                continue;
            };
            if pid == self_pid {
                continue;
            }
            if let Some(target) = target_pid {
                if pid != target {
                    continue;
                }
            }
            if dict_f64(dict, &k_alpha).unwrap_or(1.0) <= 0.01 {
                continue;
            }
            let Some(rect) = dict_rect(dict, &k_bounds) else {
                continue;
            };
            // Skip tiny helper/find/status windows that sit on layer 0.
            if rect.size.width < 160.0 || rect.size.height < 100.0 {
                continue;
            }

            if front.is_some() {
                same_owner_windows += 1;
                continue;
            }

            let geom = WindowGeom {
                x: rect.origin.x.round() as i32,
                y: rect.origin.y.round() as i32,
                width: rect.size.width.round().max(0.0) as u32,
                height: rect.size.height.round().max(0.0) as u32,
            };
            front = Some(geom);
            same_owner_windows = 1;
        }

        unsafe { CFRelease(list) };

        match front {
            Some(geom) => CgFrontResult::Frontmost {
                geom,
                same_owner_windows,
            },
            None => CgFrontResult::Unavailable,
        }
    }
}

