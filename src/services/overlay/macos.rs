//! AppKit floating panel that sits over the terminal's preview cells.

use std::io::Cursor;
use std::process::Command;

use image::{DynamicImage, ImageFormat};
use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSEventMask,
    NSImage, NSImageScaling, NSImageView, NSPanel, NSScreen, NSStatusWindowLevel,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSData, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize};

use super::{ScreenRect, WindowGeom};

pub struct MacOverlay {
    panel: Option<Retained<NSPanel>>,
    image_view: Option<Retained<NSImageView>>,
    visible: bool,
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
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

        if self.panel.is_none() {
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
            panel.setHidesOnDeactivate(false);
            panel.setOpaque(false);
            panel.setBackgroundColor(Some(&NSColor::clearColor()));
            panel.setIgnoresMouseEvents(true);
            panel.setHasShadow(false);
            panel.setLevel(NSStatusWindowLevel);
            panel.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    .union(NSWindowCollectionBehavior::Transient)
                    .union(NSWindowCollectionBehavior::IgnoresCycle)
                    .union(NSWindowCollectionBehavior::Stationary),
            );
            // We own the Retained; do not let AppKit release on close.
            unsafe {
                panel.setReleasedWhenClosed(false);
            }

            let view = NSImageView::initWithFrame(NSImageView::alloc(mtm), frame);
            view.setImageScaling(NSImageScaling::ScaleAxesIndependently);
            panel.setContentView(Some(&view));

            self.image_view = Some(view);
            self.panel = Some(panel);
        }
        true
    }

    pub fn show(&mut self, img: &DynamicImage, rect: ScreenRect) {
        if rect.width == 0 || rect.height == 0 {
            self.hide();
            return;
        }
        let png = encode_png(img);
        if !self.ensure() {
            return;
        }
        let data = NSData::from_vec(png);
        let Some(nsimg) = NSImage::initWithData(NSImage::alloc(), &data) else {
            return;
        };
        if let Some(view) = &self.image_view {
            view.setImage(Some(&nsimg));
        }
        let cocoa = to_cocoa_rect(rect);
        if let Some(panel) = &self.panel {
            panel.setFrame_display(cocoa, true);
            panel.orderFrontRegardless();
        }
        self.visible = true;
        self.pump();
    }

    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        if let Some(panel) = &self.panel {
            panel.orderOut(None);
        }
        self.visible = false;
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
        // Drain pending AppKit events so the panel actually paints without
        // taking focus from the terminal.
        loop {
            let event = unsafe {
                app.nextEventMatchingMask_untilDate_inMode_dequeue(
                    NSEventMask::Any,
                    None,
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

fn encode_png(img: &DynamicImage) -> Vec<u8> {
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

pub fn front_window() -> Option<WindowGeom> {
    let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    if program.contains("iTerm") {
        return parse_bounds(&osascript(
            r#"tell application "iTerm" to get the bounds of the first window"#,
        ));
    }
    if program == "Apple_Terminal" || program.contains("Terminal") {
        return parse_bounds(&osascript(
            r#"tell application "Terminal" to get the bounds of the front window"#,
        ));
    }
    parse_system_events()
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

fn parse_system_events() -> Option<WindowGeom> {
    let raw = osascript(
        r#"tell application "System Events"
            tell (first process whose frontmost is true)
                set p to position of first window
                set s to size of first window
                return (item 1 of p as text) & "," & (item 2 of p as text) & "," & (item 1 of s as text) & "," & (item 2 of s as text)
            end tell
        end tell"#,
    )?;
    let parts: Vec<i32> = raw
        .split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect();
    if parts.len() != 4 {
        return None;
    }
    Some(WindowGeom {
        x: parts[0],
        y: parts[1],
        width: parts[2].max(0) as u32,
        height: parts[3].max(0) as u32,
    })
}
