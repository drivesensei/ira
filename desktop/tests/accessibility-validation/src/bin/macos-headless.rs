//! No NSApplication, NSWindow, run loop, screen permission or visible UI is created.
#[cfg(target_os = "macos")]
fn main() {
    use ira_accessibility_validation::accessibility::{
        ActionSink,
        macos::{BridgeError, NativeBridge},
        model::{AccessibilityModel, LayoutSnapshot},
    };
    use objc2::{MainThreadOnly, msg_send};
    use objc2_app_kit::NSView;
    use objc2_foundation::{MainThreadMarker, NSRect};
    use raw_window_handle::{
        AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
    };
    use std::{ptr::NonNull, sync::Arc};
    struct BorrowedView<'a>(&'a NSView);
    impl HasWindowHandle for BorrowedView<'_> {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let raw = AppKitWindowHandle::new(NonNull::from(self.0).cast());
            // SAFETY: live borrowed retained NSView outlives this WindowHandle.
            Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::AppKit(raw)) })
        }
    }
    let mtm = MainThreadMarker::new().expect("binary main is AppKit main thread");
    let view = NSView::initWithFrame(NSView::alloc(mtm), NSRect::default());
    assert!(view.window().is_none());
    // SAFETY: NSObject retainCount exists; only compare before/after this isolated failed attachment.
    let before: usize = unsafe { msg_send![&*view, retainCount] };
    let snapshot = ira_core::application::App::default().snapshot();
    let tree = Arc::new(AccessibilityModel::default().project(
        &snapshot,
        None,
        &LayoutSnapshot::default(),
    ));
    let (sink, _receiver) = ActionSink::channel(tree, 2);
    assert!(matches!(
        NativeBridge::attach(&BorrowedView(&view), sink.clone()),
        Err(BridgeError::NoNativeWindow)
    ));
    let after: usize = unsafe { msg_send![&*view, retainCount] };
    assert_eq!(
        before, after,
        "failed attachment releases its temporary native retain"
    );
    assert!(
        !sink.is_closing(),
        "failed pre-attachment must not steal ownership of sink"
    );
    assert!(view.window().is_none());
    println!("PASS: main-thread view-only failed attachment balances retain; no NSWindow created");
}
#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("macos-headless validation requires macOS");
}
