//! Synthetic NSObject host driving the actual public gate lifecycle.
use super::super::*;

pub(super) fn native_gate_lifecycle_probe() {
    use super::super::mac;
    use objc2::{
        ClassType, DefinedClass, MainThreadOnly, define_class, msg_send,
        rc::{Allocated, Retained, Weak},
        runtime::{AnyObject, ClassBuilder, ProtocolObject},
    };
    use objc2_app_kit::NSApplicationDelegate;
    use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol};
    use std::{cell::RefCell, ffi::c_void, rc::Rc};
    struct Lifetime(Rc<Cell<usize>>);
    impl Drop for Lifetime {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "IRAQuitGateOriginalBase"]
        #[ivars = Lifetime]
        struct Original;
        unsafe impl NSObjectProtocol for Original {}
        unsafe impl NSApplicationDelegate for Original {}
    );
    type ReplyHook = Box<dyn FnOnce(&Host, bool)>;
    struct HostIvars {
        delegate: RefCell<Option<Weak<ProtocolObject<dyn NSApplicationDelegate>>>>,
        replies: Rc<RefCell<Vec<bool>>>,
        hook: RefCell<Option<ReplyHook>>,
    }
    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "IRAQuitGateHostBase"]
        #[ivars = HostIvars]
        struct Host;
        unsafe impl NSObjectProtocol for Host {}
        impl Host {
            #[unsafe(method_id(delegate))]
            fn delegate(&self) -> Option<Retained<ProtocolObject<dyn NSApplicationDelegate>>> {
                self.ivars().delegate.borrow().as_ref().and_then(Weak::load)
            }
            #[unsafe(method(setDelegate:))]
            fn set_delegate(&self, delegate: Option<&ProtocolObject<dyn NSApplicationDelegate>>) {
                *self.ivars().delegate.borrow_mut() = delegate.map(Weak::new);
            }
            #[unsafe(method(replyToApplicationShouldTerminate:))]
            fn reply(&self, approved: bool) {
                self.ivars().replies.borrow_mut().push(approved);
                let hook = self.ivars().hook.borrow_mut().take();
                if let Some(hook) = hook { hook(self, approved); }
            }
        }
    );
    impl Host {
        fn read_delegate(&self) -> Option<Retained<ProtocolObject<dyn NSApplicationDelegate>>> {
            objc2::rc::autoreleasepool(|_| unsafe { msg_send![self, delegate] })
        }
        fn replace_delegate(&self, delegate: Option<&ProtocolObject<dyn NSApplicationDelegate>>) {
            let _: () = unsafe { msg_send![self, setDelegate: delegate] };
        }
        fn ask(&self) -> usize {
            let delegate = self.read_delegate().unwrap();
            // The proxy receives a valid NSObject sender, with native @ ABI.
            unsafe { msg_send![&*delegate, applicationShouldTerminate: self] }
        }
        fn points_to(&self, object: &AnyObject) -> bool {
            self.read_delegate()
                .is_some_and(|d| std::ptr::eq(Retained::as_ptr(&d).cast::<AnyObject>(), object))
        }
        fn current_object(&self) -> Retained<AnyObject> {
            // ProtocolObject's upcast preserves a real object's dynamic class.
            // SAFETY: universal Objective-C root upcast preserves the valid
            // retained dynamic class; never casts to NSApplication.
            unsafe { Retained::cast_unchecked::<AnyObject>(self.read_delegate().unwrap()) }
        }
    }
    MainThreadMarker::new().expect("public gate probe requires main thread");
    let mut original_class = ClassBuilder::new(c"IRAQuitGateOriginal", Original::class()).unwrap();
    original_class.add_ivar::<*mut c_void>(c"platform");
    let original_class = original_class.register();
    let mut host_class = ClassBuilder::new(c"IRAQuitGateHost", Host::class()).unwrap();
    host_class.add_ivar::<*mut c_void>(c"platform");
    let host_class = host_class.register();
    let expected = std::ptr::NonNull::<c_void>::dangling().as_ptr();
    let epoch = Cell::new(100);
    let new_original = |drops: Rc<Cell<usize>>| {
        let allocated: Allocated<Original> = unsafe { msg_send![original_class, alloc] };
        let allocated = allocated.set_ivars(Lifetime(drops));
        let original: Retained<Original> = unsafe { msg_send![super(allocated), init] };
        mac::set_platform(original.as_ref(), expected).unwrap();
        original
    };
    let fixture = || {
        let drops = Rc::new(Cell::new(0));
        let original = new_original(drops.clone());
        let replies = Rc::new(RefCell::new(Vec::new()));
        let allocated: Allocated<Host> = unsafe { msg_send![host_class, alloc] };
        let allocated = allocated.set_ivars(HostIvars {
            delegate: RefCell::new(None),
            replies: replies.clone(),
            hook: RefCell::new(None),
        });
        let host: Retained<Host> = unsafe { msg_send![super(allocated), init] };
        mac::set_platform(host.as_ref(), expected).unwrap();
        host.replace_delegate(Some(ProtocolObject::from_ref(&*original)));
        epoch.set(epoch.get() + 1);
        let gate =
            mac::ApplicationQuitGate::fixture(host.clone().into_super().into_super(), epoch.get())
                .unwrap();
        (gate, host, original, drops, replies)
    };
    // Public once-only approval and synchronous native callback reentry.
    let (gate, host, original, _, replies) = fixture();
    assert_eq!(host.ask(), 2);
    let request = gate.take_request().unwrap().unwrap();
    for _ in 0..8 {
        assert_eq!(host.ask(), 2);
        assert_eq!(gate.take_request(), Ok(None));
    }
    let reentered = Rc::new(Cell::new(false));
    let observed = reentered.clone();
    *host.ivars().hook.borrow_mut() = Some(Box::new(move |host, approved| {
        assert!(approved);
        assert_eq!(host.ask(), 1);
        observed.set(true);
    }));
    gate.reply(request, QuitDecision::Approve).unwrap();
    assert!(reentered.get());
    assert_eq!(
        gate.reply(request, QuitDecision::Approve),
        Err(QuitError::StaleRequest)
    );
    assert_eq!(&*replies.borrow(), &[true]);
    drop(gate);
    assert!(host.points_to(original.as_ref()));
    assert_eq!(
        &*replies.borrow(),
        &[true],
        "Drop never cancels committed approval"
    );

    // Public cancellation observes Cancelling during dispatch then permits retry.
    let (gate, host, original, _, replies) = fixture();
    host.ask();
    let old = gate.take_request().unwrap().unwrap();
    *host.ivars().hook.borrow_mut() = Some(Box::new(|host, approved| {
        assert!(!approved);
        assert_eq!(host.ask(), 0);
    }));
    gate.reply(old, QuitDecision::Cancel).unwrap();
    assert_eq!(host.ask(), 2);
    let new = gate.take_request().unwrap().unwrap();
    assert_ne!(old, new);
    assert_eq!(
        gate.reply(old, QuitDecision::Approve),
        Err(QuitError::StaleRequest)
    );
    gate.reply(new, QuitDecision::Approve).unwrap();
    assert_eq!(&*replies.borrow(), &[false, true]);
    drop(gate);
    assert!(host.points_to(original.as_ref()));

    // Foreign replacement: no take, YES, NO, cancellation or ownership overwrite.
    let (gate, host, original, drops, replies) = fixture();
    host.ask();
    let request = gate.take_request().unwrap().unwrap();
    let external_proxy = host.current_object();
    let foreign = new_original(Rc::new(Cell::new(0)));
    host.replace_delegate(Some(ProtocolObject::from_ref(&*foreign)));
    drop(original);
    assert!(gate.is_pending());
    assert_eq!(gate.take_request(), Err(QuitError::DelegateChanged));
    assert_eq!(
        gate.reply(request, QuitDecision::Approve),
        Err(QuitError::DelegateChanged)
    );
    assert_eq!(
        gate.reply(request, QuitDecision::Cancel),
        Err(QuitError::DelegateChanged)
    );
    assert!(replies.borrow().is_empty());
    assert_eq!(drops.get(), 0);
    drop(gate);
    assert!(host.points_to(foreign.as_ref()));
    assert!(replies.borrow().is_empty());
    assert_eq!(
        drops.get(),
        0,
        "foreign replacement must not kill retained original"
    );
    drop(external_proxy);
    assert_eq!(drops.get(), 1);

    // Drop owns pending gate: native NO, reentrant Cancelling, restore original.
    let (gate, host, original, _, replies) = fixture();
    host.ask();
    *host.ivars().hook.borrow_mut() = Some(Box::new(|host, approved| {
        assert!(!approved);
        assert_eq!(host.ask(), 0);
    }));
    drop(gate);
    assert_eq!(&*replies.borrow(), &[false]);
    assert!(host.points_to(original.as_ref()));

    // Synchronous cancellation replaces delegate; post-reply check preserves it.
    let (gate, host, original, drops, replies) = fixture();
    host.ask();
    let external_proxy = host.current_object();
    let foreign = new_original(Rc::new(Cell::new(0)));
    let foreign_callback = ProtocolObject::from_retained(foreign.clone());
    *host.ivars().hook.borrow_mut() = Some(Box::new(move |host, approved| {
        assert!(!approved);
        assert_eq!(host.ask(), 0);
        host.replace_delegate(Some(&foreign_callback));
    }));
    drop(original);
    drop(gate);
    assert_eq!(&*replies.borrow(), &[false]);
    assert!(host.points_to(foreign.as_ref()));
    assert_eq!(drops.get(), 0);
    drop(external_proxy);
    assert_eq!(drops.get(), 1);

    // A synchronous NO can change platform ownership without replacing the
    // delegate; final restoration must revalidate pointers as well as identity.
    let (gate, host, original, _, replies) = fixture();
    host.ask();
    let external_proxy = host.current_object();
    let callback_original = original.clone();
    let foreign_pointer = std::ptr::NonNull::<u64>::dangling().as_ptr().cast();
    *host.ivars().hook.borrow_mut() = Some(Box::new(move |host, approved| {
        assert!(!approved);
        assert_eq!(host.ask(), 0);
        mac::set_platform(callback_original.as_ref(), foreign_pointer).unwrap();
    }));
    drop(gate);
    assert_eq!(&*replies.borrow(), &[false]);
    assert!(
        host.points_to(&external_proxy),
        "Drop must not restore an original whose pointer changed during NO"
    );
    assert_eq!(mac::platform(original.as_ref()), Ok(foreign_pointer));
    drop(external_proxy);

    // Public Drop mirrors GPUI's exact app/proxy null sequence before restore.
    let (gate, host, original, _, replies) = fixture();
    let external_proxy = host.current_object();
    mac::set_platform(host.as_ref(), std::ptr::null_mut()).unwrap();
    mac::set_platform(&external_proxy, std::ptr::null_mut()).unwrap();
    drop(gate);
    assert_eq!(mac::platform(original.as_ref()), Ok(std::ptr::null_mut()));
    assert!(host.points_to(original.as_ref()));
    assert!(replies.borrow().is_empty());
    drop(external_proxy);

    // Unexpected original pointer suppresses Drop cancellation and restoration.
    let (gate, host, original, _, replies) = fixture();
    host.ask();
    let external_proxy = host.current_object();
    let foreign_pointer = std::ptr::NonNull::<u64>::dangling().as_ptr().cast();
    mac::set_platform(original.as_ref(), foreign_pointer).unwrap();
    drop(gate);
    assert!(host.points_to(&external_proxy));
    assert!(replies.borrow().is_empty());
    assert_eq!(mac::platform(original.as_ref()), Ok(foreign_pointer));
    println!(
        "public gate lifecycle: take/reply, actual native reentry, stale/once/cancel-retry, foreign rejection/noYESorNO, owningDrop/cancellation, foreign-after-cancel preservation, strong lifetime, actualDrop null-alias/foreign-pointer PASS"
    );
}
