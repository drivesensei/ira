//! AppKit floating panel that sits over the terminal's preview cells.

use std::io::Cursor;
use std::process::Command;

use image::{DynamicImage, ImageFormat};
use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSEventMask,
    NSFloatingWindowLevel, NSImage, NSImageFrameStyle, NSImageScaling, NSImageView, NSPanel,
    NSScreen, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSData, NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize};

use super::{ScreenRect, WindowGeom};

pub struct MacOverlay {
    panel: Option<Retained<NSPanel>>,
    image_view: Option<Retained<NSImageView>>,
    pub visible: bool,
}

impl MacOverlay {
    pub fn new() -> Self {
        Self {
            panel: None,
            image_view: None,
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
            panel.setHidesOnDeactivate(true);
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

    pub fn show_png(&mut self, png: &[u8], rect: ScreenRect) {
        if rect.width == 0 || rect.height == 0 || png.is_empty() {
            self.hide();
            return;
        }
        if !self.ensure() {
            return;
        }
        let data = NSData::from_vec(png.to_vec());
        let Some(nsimg) = NSImage::initWithData(NSImage::alloc(), &data) else {
            return;
        };
        if let Some(view) = &self.image_view {
            view.setImage(Some(&nsimg));
        }
        let cocoa = to_cocoa_rect(rect);
        if let Some(panel) = &self.panel {
            panel.setFrame_display(cocoa, true);
            panel.orderFront(None);
        }
        self.visible = true;
        self.pump();
    }

    pub fn move_to(&mut self, rect: ScreenRect) {
        if rect.width == 0 || rect.height == 0 {
            self.order_out();
            return;
        }
        if let Some(panel) = &self.panel {
            panel.setFrame_display(to_cocoa_rect(rect), true);
            if !self.visible {
                panel.orderFront(None);
                self.visible = true;
                self.pump();
            }
        }
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
    let screen_h = main_screen_height();
    let y = screen_h - rect.y as f64 - rect.height as f64;
    NSRect::new(
        NSPoint::new(rect.x as f64, y),
        NSSize::new(rect.width as f64, rect.height as f64),
    )
}

fn main_screen_height() -> f64 {
    let Some(mtm) = MainThreadMarker::new() else {
        return 1080.0;
    };
    NSScreen::mainScreen(mtm)
        .map(|s| s.frame().size.height)
        .unwrap_or(1080.0)
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

pub fn query_front_window() -> Option<WindowGeom> {
    let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    let expected = if program.contains("iTerm") {
        "iTerm2"
    } else if program.contains("ghostty") || program.contains("Ghostty") {
        "Ghostty"
    } else if program == "Apple_Terminal" || program.contains("Terminal") {
        "Terminal"
    } else {
        ""
    };
    let tty = stdin_tty().filter(|t| !t.is_empty() && !t.contains('"') && !t.contains('\\'));
    let script = if expected.is_empty() {
        r#"tell application "System Events"
            set n to name of first process whose frontmost is true
            if n is not in {"Terminal", "iTerm2", "iTerm", "Ghostty", "kitty", "WezTerm", "Alacritty", "Warp"} then
                return "HIDE"
            end if
            tell (first process whose frontmost is true)
                set p to position of first window
                set s to size of first window
                return (item 1 of p as text) & "," & (item 2 of p as text) & "," & (item 1 of s as text) & "," & (item 2 of s as text)
            end tell
        end tell"#
            .to_string()
    } else if expected == "iTerm2" {
        iterm_bounds_script(tty.as_deref())
    } else if expected == "Ghostty" {
        r#"tell application "System Events" to set n to name of first process whose frontmost is true
if n does not contain "Ghostty" and n is not "ghostty" then return "HIDE"
tell application "System Events"
    tell (first process whose frontmost is true)
        set p to position of first window
        set s to size of first window
        return (item 1 of p as text) & "," & (item 2 of p as text) & "," & (item 1 of s as text) & "," & (item 2 of s as text)
    end tell
end tell"#
            .to_string()
    } else {
        terminal_app_bounds_script(tty.as_deref())
    };
    let raw = osascript(&script)?;
    if raw.trim() == "HIDE" {
        return None;
    }
    parse_bounds(&Some(raw))
}

fn terminal_app_bounds_script(tty: Option<&str>) -> String {
    let Some(tty) = tty else {
        return r#"tell application "System Events" to set n to name of first process whose frontmost is true
if n is not "Terminal" then return "HIDE"
tell application "Terminal" to get the bounds of the front window"#
            .to_string();
    };
    format!(
        r#"tell application "System Events" to set n to name of first process whose frontmost is true
if n is not "Terminal" then return "HIDE"
tell application "Terminal"
    set targetTty to "{tty}"
    repeat with w in windows
        try
            if tty of selected tab of w is targetTty then
                return bounds of w
            end if
        end try
    end repeat
    return "HIDE"
end tell"#
    )
}

fn iterm_bounds_script(tty: Option<&str>) -> String {
    let Some(tty) = tty else {
        return r#"tell application "System Events" to set n to name of first process whose frontmost is true
if n does not contain "iTerm" then return "HIDE"
tell application "iTerm" to get the bounds of the first window"#
            .to_string();
    };
    format!(
        r#"tell application "System Events" to set n to name of first process whose frontmost is true
if n does not contain "iTerm" then return "HIDE"
tell application "iTerm"
    set targetTty to "{tty}"
    repeat with w in windows
        repeat with t in tabs of w
            repeat with s in sessions of t
                try
                    if tty of s is targetTty then return bounds of w
                end try
            end repeat
        end repeat
    end repeat
    return "HIDE"
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
