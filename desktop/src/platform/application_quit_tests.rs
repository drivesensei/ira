use super::*;

#[test]
fn os_request_defers_and_repeated_quit_delivers_only_one_token() {
    let gate = Requests::new(7);
    assert_eq!(gate.should_terminate(), 2);
    let request = gate.take().unwrap();
    for _ in 0..32 {
        assert_eq!(gate.should_terminate(), 2);
        assert_eq!(gate.take(), None);
    }
    assert!(gate.pending());
    assert_eq!(request.generation, 1);
}
#[test]
fn no_ack_or_failed_ack_keeps_native_request_pending_without_deadline_success() {
    let gate = Requests::new(7);
    gate.should_terminate();
    gate.take().unwrap();
    // The coordinator does not call reply on failure/timeout. Arbitrarily many
    // pump turns and OS requests must never manufacture approval.
    for _ in 0..1000 {
        assert!(gate.pending());
        assert_eq!(gate.should_terminate(), 2);
    }
}
#[test]
fn success_is_explicit_and_reentrant_request_sees_approval() {
    let gate = Requests::new(7);
    gate.should_terminate();
    let request = gate.take().unwrap();
    gate.begin_reply(request, QuitDecision::Approve).unwrap();
    assert_eq!(gate.should_terminate(), 1);
    assert_eq!(
        gate.begin_reply(request, QuitDecision::Approve),
        Err(QuitError::StaleRequest)
    );
    gate.finish_reply(QuitDecision::Approve);
    assert_eq!(gate.should_terminate(), 1);
    assert!(!gate.pending());
}
#[test]
fn cancellation_is_reentrant_safe_and_future_attempt_gets_new_generation() {
    let gate = Requests::new(7);
    gate.should_terminate();
    let old = gate.take().unwrap();
    gate.begin_reply(old, QuitDecision::Cancel).unwrap();
    assert_eq!(gate.should_terminate(), 0);
    assert_eq!(gate.take(), None);
    gate.finish_reply(QuitDecision::Cancel);
    assert_eq!(gate.should_terminate(), 2);
    let new = gate.take().unwrap();
    assert_ne!(old, new);
    assert_eq!(
        gate.begin_reply(old, QuitDecision::Approve),
        Err(QuitError::StaleRequest)
    );
    assert!(gate.pending());
    gate.begin_reply(new, QuitDecision::Approve).unwrap();
}
#[test]
fn foreign_gate_token_never_approves_matching_generation() {
    let first = Requests::new(7);
    let second = Requests::new(8);
    first.should_terminate();
    second.should_terminate();
    let request = first.take().unwrap();
    second.take().unwrap();
    assert_eq!(
        second.begin_reply(request, QuitDecision::Approve),
        Err(QuitError::StaleRequest)
    );
    assert!(second.pending());
}
#[test]
fn token_exhaustion_refuses_quit_without_wrapping_or_false_ack() {
    let gate = Requests::new(7);
    gate.generation.set(u64::MAX);
    assert_eq!(gate.should_terminate(), 0);
    assert_eq!(gate.take(), None);
    assert!(!gate.pending());
}
#[test]
fn pinned_gpui_late_observer_is_not_an_earlier_termination_gate() {
    // Source-grounded negative proof. The fixture is the pinned local registry
    // implementation, rather than an invented callback-timeout expectation.
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cargo")
        });
    let gpui = cargo_home.join("registry/src");
    let package = std::fs::read_dir(gpui)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("gpui-0.2.2/src"))
        .find(|path| path.is_dir())
        .unwrap();
    let app = std::fs::read_to_string(package.join("app.rs")).unwrap();
    let mac = std::fs::read_to_string(package.join("platform/mac/platform.rs")).unwrap();
    assert!(app.contains("const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(100)"));
    assert!(mac.contains("sel!(applicationWillTerminate:)"));
    assert!(!mac.contains("sel!(applicationShouldTerminate:)"));
    assert!(mac.contains("(*NSWindow::delegate(app)).set_ivar(MAC_PLATFORM_IVAR"));
}

/// Called by the separately compiled tiny main-thread evidence harness. This
/// is a real Objective-C proxy/lifetime probe, without NSApp or a native window.
#[cfg(target_os = "macos")]
#[allow(dead_code)] // Invoked by the standalone main-thread evidence harness.
pub(crate) fn native_contract_probe() {
    use super::mac;
    use objc2::{
        ClassType, DefinedClass, MainThreadOnly, define_class, msg_send,
        rc::{Allocated, Retained},
        runtime::{AnyObject, ClassBuilder, ProtocolBuilder, ProtocolObject},
        sel,
    };
    use objc2_app_kit::NSApplicationDelegate;
    use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol};
    use std::{ffi::c_void, rc::Rc};
    struct Lifetime(Rc<Cell<usize>>);
    impl Drop for Lifetime {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    struct Ivars {
        calls: Rc<Cell<usize>>,
        _lifetime: Lifetime,
    }
    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "IRAQuitFixtureBase"]
        #[ivars = Ivars]
        struct Original;
        unsafe impl NSObjectProtocol for Original {}
        unsafe impl NSApplicationDelegate for Original {}
        impl Original {
            #[unsafe(method(handleGPUIMenuItem:))]
            fn menu(&self, _sender: &AnyObject) { self.ivars().calls.set(self.ivars().calls.get()+1); }
            #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
            fn reopen(&self, _sender: &AnyObject, _visible: bool) -> bool { self.ivars().calls.set(self.ivars().calls.get()+1); true }
            #[unsafe(method(applicationWillTerminate:))]
            fn will_terminate(&self, _notification: &AnyObject) { self.ivars().calls.set(self.ivars().calls.get()+1); }
        }
    );
    let mtm = MainThreadMarker::new().expect("probe must execute on actual main thread");
    let extra_protocol = ProtocolBuilder::new(c"IRAQuitFixtureProtocol")
        .unwrap()
        .register();
    let mut class = ClassBuilder::new(c"IRAQuitFixtureDelegate", Original::class()).unwrap();
    class.add_ivar::<*mut c_void>(c"platform");
    class.add_protocol(extra_protocol);
    let class = class.register();
    let calls = Rc::new(Cell::new(0));
    let drops = Rc::new(Cell::new(0));
    let fixture = |calls: Rc<Cell<usize>>, drops: Rc<Cell<usize>>| {
        let allocated: Allocated<Original> = unsafe { msg_send![class, alloc] };
        let allocated = allocated.set_ivars(Ivars {
            calls,
            _lifetime: Lifetime(drops),
        });
        let original: Retained<Original> = unsafe { msg_send![super(allocated), init] };
        original
    };
    let original = fixture(calls.clone(), drops.clone());
    let app = fixture(Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let expected = std::ptr::NonNull::<c_void>::dangling().as_ptr();
    mac::set_platform(original.as_ref(), expected).unwrap();
    mac::set_platform(app.as_ref(), expected).unwrap();
    let proxy = mac::new_proxy(
        ProtocolObject::from_retained(original.clone()),
        42,
        expected,
    )
    .unwrap();
    assert!(proxy.respondsToSelector(sel!(applicationShouldTerminate:)));
    for selector in [
        sel!(handleGPUIMenuItem:),
        sel!(applicationShouldHandleReopen:hasVisibleWindows:),
        sel!(applicationWillTerminate:),
    ] {
        assert!(proxy.respondsToSelector(selector));
    }
    assert!(!proxy.respondsToSelector(sel!(iraMissingSelector)));
    assert!(proxy.conformsToProtocol(extra_protocol));
    assert!(proxy.respondsToSelector(sel!(description))); // NSObject own method.
    // objc2 debug preflight checks only the receiver class method table;
    // native fast forwarding must be exercised through the dispatcher.
    unsafe extern "C" {
        fn objc_msgSend();
    }
    let send_void: unsafe extern "C" fn(*const AnyObject, objc2::runtime::Sel, *const AnyObject) =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    let send_reopen: unsafe extern "C" fn(
        *const AnyObject,
        objc2::runtime::Sel,
        *const AnyObject,
        objc2::runtime::Bool,
    ) -> objc2::runtime::Bool =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    let proxy_pointer = Retained::as_ptr(&proxy).cast::<AnyObject>();
    let app_pointer = Retained::as_ptr(&app).cast::<AnyObject>();
    unsafe {
        send_void(proxy_pointer, sel!(handleGPUIMenuItem:), app_pointer);
    }
    let reopened = unsafe {
        send_reopen(
            proxy_pointer,
            sel!(applicationShouldHandleReopen:hasVisibleWindows:),
            app_pointer,
            objc2::runtime::Bool::NO,
        )
    };
    assert!(reopened.as_bool());
    unsafe {
        send_void(proxy_pointer, sel!(applicationWillTerminate:), app_pointer);
    }
    assert_eq!(
        calls.get(),
        3,
        "real original receiver handled all GPUI selectors"
    );
    let native_decision =
        || -> usize { unsafe { msg_send![&*proxy, applicationShouldTerminate: &*app] } };
    assert_eq!(native_decision(), 2);
    let request = proxy.ivars().requests.take().unwrap();
    assert_eq!(native_decision(), 2);
    proxy
        .ivars()
        .requests
        .begin_reply(request, QuitDecision::Cancel)
        .unwrap();
    assert_eq!(native_decision(), 0);
    proxy.ivars().requests.finish_reply(QuitDecision::Cancel);
    assert_eq!(native_decision(), 2);
    let new_request = proxy.ivars().requests.take().unwrap();
    assert_ne!(request, new_request);
    proxy
        .ivars()
        .requests
        .begin_reply(new_request, QuitDecision::Approve)
        .unwrap();
    assert_eq!(native_decision(), 1);
    mac::synchronize_platform_alias(app.as_ref(), proxy.as_ref(), original.as_ref(), expected)
        .unwrap();
    mac::set_platform(app.as_ref(), std::ptr::null_mut()).unwrap();
    mac::set_platform(proxy.as_ref(), std::ptr::null_mut()).unwrap();
    mac::synchronize_platform_alias(app.as_ref(), proxy.as_ref(), original.as_ref(), expected)
        .unwrap();
    assert_eq!(
        mac::platform(original.as_ref()),
        Ok(std::ptr::null_mut()),
        "GPUI499 alias null mirrored before restore"
    );
    let foreign = std::ptr::NonNull::<u64>::dangling().as_ptr().cast();
    mac::set_platform(original.as_ref(), foreign).unwrap();
    assert_eq!(
        mac::synchronize_platform_alias(app.as_ref(), proxy.as_ref(), original.as_ref(), expected),
        Err(QuitError::UnsupportedDelegate)
    );
    assert_eq!(
        mac::platform(original.as_ref()),
        Ok(foreign),
        "foreign pointer never overwritten"
    );
    let external_proxy = proxy.clone();
    drop(original);
    drop(proxy);
    assert_eq!(
        drops.get(),
        0,
        "external proxy retains original and Rust state"
    );
    unsafe {
        send_void(
            Retained::as_ptr(&external_proxy).cast::<AnyObject>(),
            sel!(handleGPUIMenuItem:),
            app_pointer,
        );
    }
    assert_eq!(calls.get(), 4);
    drop(external_proxy);
    assert_eq!(
        drops.get(),
        1,
        "generated ivar teardown destroys original once"
    );
    assert_eq!(
        std::thread::spawn(|| ApplicationQuitGate::install().err())
            .join()
            .unwrap(),
        Some(QuitError::WrongThread)
    );
    let _ = mtm;
    println!(
        "native synthetic adapter: forwarding, responds, protocol, repeat, reentry, cancellation, null-alias, foreign-pointer, strong lifetime, wrong-thread PASS"
    );
}
