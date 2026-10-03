//! Earlier native termination decision for pinned GPUI 0.2.2.
//!
//! Install after GPUI installs its delegate. Keep this main-thread guard outside
//! `Application::run` until process exit. Poll `take_request` from the existing
//! coordinator; native callbacks never borrow the GPUI App. Only reply Approve
//! after actual persistence acknowledgments. Failed acknowledgments keep the
//! request pending; Cancel requires a functioning/recovered runtime.
#[cfg(any(target_os = "macos", test))]
use std::cell::Cell;

// Runtime metadata only; fixed byte/output bounds and ASCII escaping keep each
// rejection diagnostic one line. Never pass an object/pointer/address to this.
#[cfg(any(target_os = "macos", test))]
const STRUCTURE_BYTES: usize = 64;
#[cfg(any(target_os = "macos", test))]
fn bounded_structure(value: &[u8]) -> String {
    let mut output: String = value
        .iter()
        .take(STRUCTURE_BYTES)
        .flat_map(|byte| std::ascii::escape_default(*byte))
        .map(char::from)
        .collect();
    if value.len() > STRUCTURE_BYTES {
        output.push_str("...");
    }
    output
}

#[cfg(any(target_os = "macos", test))]
fn write_structure(output: &mut impl std::io::Write, arguments: std::fmt::Arguments<'_>) {
    // Diagnostics cannot replace a safety rejection or unwind from gate Drop.
    // A closed/broken stderr is deliberately ignored; guard results remain authoritative.
    let _ = writeln!(output, "{arguments}");
}
#[cfg(target_os = "macos")]
fn emit_structure(arguments: std::fmt::Arguments<'_>) {
    write_structure(&mut std::io::stderr().lock(), arguments);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuitRequest {
    gate: u64,
    generation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuitDecision {
    Approve,
    Cancel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuitError {
    UnsupportedPlatform,
    WrongThread,
    MissingDelegate,
    UnsupportedDelegate,
    ExistingTerminationHook,
    ClassRegistration,
    DelegateChanged,
    StaleRequest,
    RequestExhausted,
}
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Pending { generation: u64, delivered: bool },
    Approving,
    Approved,
    Cancelling,
    Exhausted,
}
#[cfg(any(target_os = "macos", test))]
struct Requests {
    gate: u64,
    generation: Cell<u64>,
    phase: Cell<Phase>,
}
#[cfg(any(target_os = "macos", test))]
impl Requests {
    fn new(gate: u64) -> Self {
        Self {
            gate,
            generation: Cell::new(0),
            phase: Cell::new(Phase::Idle),
        }
    }
    // Native reply values are Cancel=0, Now=1, Later=2.
    fn should_terminate(&self) -> usize {
        match self.phase.get() {
            Phase::Approving | Phase::Approved => 1,
            Phase::Cancelling | Phase::Exhausted => 0,
            Phase::Pending { .. } => 2,
            Phase::Idle => {
                let Some(generation) = self.generation.get().checked_add(1) else {
                    self.phase.set(Phase::Exhausted);
                    return 0;
                };
                self.generation.set(generation);
                self.phase.set(Phase::Pending {
                    generation,
                    delivered: false,
                });
                2
            }
        }
    }
    fn take(&self) -> Option<QuitRequest> {
        if let Phase::Pending {
            generation,
            delivered: false,
        } = self.phase.get()
        {
            self.phase.set(Phase::Pending {
                generation,
                delivered: true,
            });
            Some(QuitRequest {
                gate: self.gate,
                generation,
            })
        } else {
            None
        }
    }
    fn begin_reply(&self, request: QuitRequest, decision: QuitDecision) -> Result<(), QuitError> {
        if request.gate != self.gate
            || self.phase.get()
                != (Phase::Pending {
                    generation: request.generation,
                    delivered: true,
                })
        {
            return Err(QuitError::StaleRequest);
        }
        // Set before the synchronous AppKit reply, which can reenter delegates.
        self.phase.set(match decision {
            QuitDecision::Approve => Phase::Approving,
            QuitDecision::Cancel => Phase::Cancelling,
        });
        Ok(())
    }
    fn finish_reply(&self, decision: QuitDecision) {
        self.phase.set(match decision {
            QuitDecision::Approve => Phase::Approved,
            QuitDecision::Cancel => Phase::Idle,
        });
    }
    fn pending(&self) -> bool {
        matches!(self.phase.get(), Phase::Pending { .. })
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use objc2::{
        ClassType, DefinedClass, Encode, MainThreadOnly, define_class, msg_send,
        rc::{Allocated, Retained},
        runtime::{AnyClass, AnyObject, AnyProtocol, ClassBuilder, Ivar, ProtocolObject, Sel},
        sel,
    };
    use objc2_app_kit::{NSApplication, NSApplicationDelegate};
    use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol};
    use std::{
        ffi::c_void,
        sync::{
            OnceLock,
            atomic::{AtomicU64, Ordering},
        },
    };

    pub(super) struct ProxyIvars {
        pub(super) original: Retained<ProtocolObject<dyn NSApplicationDelegate>>,
        pub(super) requests: Requests,
    }
    define_class!(
        // SAFETY: NSObject has no additional subclass requirements. Every
        // callback runs on AppKit's main thread and all Rust ivars are retained.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "IRAApplicationQuitProxy"]
        #[ivars = ProxyIvars]
        pub(super) struct Proxy;
        unsafe impl NSObjectProtocol for Proxy {
            #[unsafe(method(conformsToProtocol:))]
            fn conforms(&self, protocol: &AnyProtocol) -> bool {
                // SAFETY: call NSObject's implementation, never recurse.
                let own: bool = unsafe { msg_send![super(self), conformsToProtocol: protocol] };
                own || self.ivars().original.conformsToProtocol(protocol)
            }
            #[unsafe(method(respondsToSelector:))]
            fn responds(&self, selector: Sel) -> bool {
                // SAFETY: querying NSObject avoids recursively calling us.
                let own: bool = unsafe { msg_send![super(self), respondsToSelector: selector] };
                own || self.ivars().original.respondsToSelector(selector)
            }
        }
        unsafe impl NSApplicationDelegate for Proxy {}
        impl Proxy {
            #[unsafe(method(applicationShouldTerminate:))]
            fn should_terminate(&self, _sender: &AnyObject) -> objc2_app_kit::NSApplicationTerminateReply {
                objc2_app_kit::NSApplicationTerminateReply(self.ivars().requests.should_terminate())
            }
        }
        impl Proxy {
            #[unsafe(method(forwardingTargetForSelector:))]
            fn forwarding_target(&self, selector: Sel) -> Option<&AnyObject> {
                // NSObject's own methods are dispatched normally. Unknown
                // GPUI selectors run on the ORIGINAL receiver and its ivars.
                if self.ivars().original.respondsToSelector(selector) {
                    Some(original_object(&self.ivars().original))
                } else { None }
            }
        }
    );
    fn original_object(original: &ProtocolObject<dyn NSApplicationDelegate>) -> &AnyObject {
        original.as_ref()
    }
    pub(super) fn proxy_class() -> Result<&'static AnyClass, QuitError> {
        static CLASS: OnceLock<Option<&'static AnyClass>> = OnceLock::new();
        CLASS
            .get_or_init(|| {
                let mut builder = ClassBuilder::new(c"IRAApplicationQuitDelegate", Proxy::class())?;
                // GPUI mac/platform.rs499 nulls this exact ivar on the CURRENT
                // delegate after app.run returns. No GPUI handler runs on us.
                builder.add_ivar::<*mut c_void>(c"platform");
                Some(builder.register())
            })
            .as_ref()
            .copied()
            .ok_or(QuitError::ClassRegistration)
    }
    const APPLICATION_CLASS: &std::ffi::CStr = c"GPUIApplication";
    const APPLICATION_COMPANION: &std::ffi::CStr = c"NSKVONotifying_GPUIApplication";

    fn platform_layout(offset: isize, base_size: usize, actual_size: usize) -> bool {
        usize::try_from(offset)
            .ok()
            .is_some_and(|offset| pointer_slot_fits(offset, base_size, actual_size))
    }
    fn pointer_slot_fits(offset: usize, base_size: usize, actual_size: usize) -> bool {
        offset % std::mem::align_of::<*mut c_void>() == 0
            && offset
                .checked_add(std::mem::size_of::<*mut c_void>())
                .is_some_and(|end| end <= base_size)
            && actual_size >= base_size
    }

    // Metadata only: no object reads, messages, allocation or class mutation.
    // The parameterized seam is private; production always supplies fixed names
    // and the runtime's registered GPUI base identity.
    fn application_slot<'a>(
        actual: &AnyClass,
        base: &'a AnyClass,
        companion: &std::ffi::CStr,
    ) -> Result<&'a Ivar, QuitError> {
        let registered_base = std::ptr::eq(actual, base);
        let permitted_name = actual.name() == companion;
        let parent = actual.superclass();
        let direct_base = parent.is_some_and(|parent| std::ptr::eq(parent, base));
        let base_slot = base.instance_variable(c"platform");
        let actual_slot = actual.instance_variable(c"platform");
        let declared = base_slot.is_some_and(|slot| {
            !base
                .superclass()
                .and_then(|parent| parent.instance_variable(c"platform"))
                .is_some_and(|inherited| std::ptr::eq(slot, inherited))
        });
        let inherited_slot = base_slot
            .zip(actual_slot)
            .is_some_and(|(base, actual)| std::ptr::eq(base, actual));
        let encoding_match = base_slot.is_some_and(|slot| {
            slot.type_encoding().to_bytes() == <*mut c_void>::ENCODING.to_string().as_bytes()
        });
        let offset_match = base_slot
            .zip(actual_slot)
            .is_some_and(|(base, actual)| base.offset() == actual.offset());
        let layout_ok = base_slot.is_some_and(|slot| {
            platform_layout(slot.offset(), base.instance_size(), actual.instance_size())
        });
        let accepted = !actual.is_metaclass()
            && !base.is_metaclass()
            && (registered_base || (permitted_name && direct_base))
            && declared
            && inherited_slot
            && encoding_match
            && offset_match
            && layout_ok;
        emit_structure(format_args!(
            "Native deferred quit structure: stage=application_layout actual={} direct_super={} registered_base={} permitted_name={} direct_base={} base_declared={} inherited_slot={} encoding_match={} offset_match={} layout_ok={} accepted={}",
            bounded_structure(actual.name().to_bytes()),
            parent
                .map(|parent| bounded_structure(parent.name().to_bytes()))
                .unwrap_or_else(|| "none".into()),
            registered_base,
            permitted_name,
            direct_base,
            declared,
            inherited_slot,
            encoding_match,
            offset_match,
            layout_ok,
            accepted,
        ));
        if !accepted {
            return Err(QuitError::UnsupportedDelegate);
        }
        base_slot.ok_or(QuitError::UnsupportedDelegate)
    }

    fn validated_application_slot(actual: &AnyClass) -> Result<&'static Ivar, QuitError> {
        let Some(base) = AnyClass::get(APPLICATION_CLASS) else {
            emit_structure(format_args!(
                "Native deferred quit structure: stage=application_layout actual={} registered_base_present=false accepted=false",
                bounded_structure(actual.name().to_bytes()),
            ));
            return Err(QuitError::UnsupportedDelegate);
        };
        application_slot(actual, base, APPLICATION_COMPANION)
    }

    fn application_platform(object: &AnyObject) -> Result<*mut c_void, QuitError> {
        // AnyObject::class reads actual runtime isa, not Objective-C -class.
        let slot = validated_application_slot(object.class())?;
        // SAFETY: declaring-base metadata, exact encoding, non-shadowed ivar
        // identity and complete pointer bounds/alignment were just validated.
        // No native callback occurs between validation and this main-thread read.
        Ok(unsafe { *slot.load_ptr::<*mut c_void>(object) })
    }

    #[cfg(test)]
    mod class_tests {
        include!("application_quit_class_tests.rs");
    }

    pub(super) fn platform(object: &AnyObject) -> Result<*mut c_void, QuitError> {
        let Some(ivar) = object.class().instance_variable(c"platform") else {
            emit_structure(format_args!(
                "Native deferred quit structure: stage=platform_ivar class={} ivar_present=false",
                bounded_structure(object.class().name().to_bytes()),
            ));
            return Err(QuitError::UnsupportedDelegate);
        };
        if ivar.type_encoding().to_bytes() != <*mut c_void>::ENCODING.to_string().as_bytes() {
            emit_structure(format_args!(
                "Native deferred quit structure: stage=platform_encoding class={} ivar_present=true actual={} expected={}",
                bounded_structure(object.class().name().to_bytes()),
                bounded_structure(ivar.type_encoding().to_bytes()),
                bounded_structure(<*mut c_void>::ENCODING.to_string().as_bytes()),
            ));
            return Err(QuitError::UnsupportedDelegate);
        }
        // SAFETY: exact pinned class and encoding are checked by install;
        // proxy's field is created above. Access is main-thread-only.
        Ok(unsafe { *ivar.load_ptr::<*mut c_void>(object) })
    }
    pub(super) fn set_platform(object: &AnyObject, value: *mut c_void) -> Result<(), QuitError> {
        platform(object)?;
        let ivar = object
            .class()
            .instance_variable(c"platform")
            .ok_or(QuitError::UnsupportedDelegate)?;
        // SAFETY: class layout/encoding was validated; only main-thread use.
        unsafe {
            *ivar.load_ptr::<*mut c_void>(object) = value;
        }
        Ok(())
    }
    pub(super) fn new_proxy(
        original: Retained<ProtocolObject<dyn NSApplicationDelegate>>,
        gate: u64,
        pointer: *mut c_void,
    ) -> Result<Retained<Proxy>, QuitError> {
        MainThreadMarker::new().ok_or(QuitError::WrongThread)?;
        // SAFETY: dynamic class inherits Proxy, including Rust ivar layout.
        let allocated: Allocated<Proxy> = unsafe { msg_send![proxy_class()?, alloc] };
        let allocated = allocated.set_ivars(ProxyIvars {
            original,
            requests: Requests::new(gate),
        });
        // SAFETY: NSObject init preserves dynamic subclass and Rust ivars.
        let proxy: Retained<Proxy> = unsafe { msg_send![super(allocated), init] };
        set_platform(proxy.as_ref(), pointer)?;
        Ok(proxy)
    }
    // Only restore layouts whose pointers still match our source-pinned lease.
    #[cfg(test)]
    pub(super) fn synchronize_platform_alias(
        app: &AnyObject,
        proxy: &AnyObject,
        original: &AnyObject,
        expected: *mut c_void,
    ) -> Result<(), QuitError> {
        synchronize_platform_pointers(platform(app)?, proxy, original, expected)
    }
    fn synchronize_platform_pointers(
        app_pointer: *mut c_void,
        proxy: &AnyObject,
        original: &AnyObject,
        expected: *mut c_void,
    ) -> Result<(), QuitError> {
        let proxy_pointer = platform(proxy)?;
        let original_pointer = platform(original)?;
        if app_pointer == expected && proxy_pointer == expected && original_pointer == expected {
            return Ok(());
        }
        if app_pointer.is_null() && proxy_pointer.is_null() {
            if original_pointer == expected {
                set_platform(original, std::ptr::null_mut())?;
                return Ok(());
            }
            if original_pointer.is_null() {
                return Ok(());
            }
        }
        Err(QuitError::UnsupportedDelegate)
    }
    // Both variants send the same native selectors. The test variant is a
    // real initialized NSObject, never an NSApplication cast or global app.
    enum NativeHost {
        Application(Retained<NSApplication>),
        #[cfg(test)]
        Fixture(Retained<AnyObject>),
    }
    impl NativeHost {
        fn object(&self) -> &AnyObject {
            match self {
                Self::Application(app) => app.as_ref(),
                #[cfg(test)]
                Self::Fixture(object) => object,
            }
        }
        fn platform(&self) -> Result<*mut c_void, QuitError> {
            match self {
                Self::Application(app) => application_platform(app.as_ref()),
                #[cfg(test)]
                Self::Fixture(object) => platform(object),
            }
        }
        fn delegate(&self) -> Option<Retained<ProtocolObject<dyn NSApplicationDelegate>>> {
            // SAFETY: NSApplication and the initialized test fixture implement
            // this exact object-returning selector and weak-delegate contract.
            objc2::rc::autoreleasepool(|_| unsafe { msg_send![self.object(), delegate] })
        }
        fn set_delegate(&self, delegate: &ProtocolObject<dyn NSApplicationDelegate>) {
            // SAFETY: both host variants implement the same object argument.
            let _: () = unsafe { msg_send![self.object(), setDelegate: delegate] };
        }
        fn reply(&self, approved: bool) {
            // SAFETY: both hosts implement AppKit's BOOL argument/void result.
            let _: () =
                unsafe { msg_send![self.object(), replyToApplicationShouldTerminate: approved] };
        }
    }
    pub struct ApplicationQuitGate {
        app: NativeHost,
        proxy: Retained<Proxy>,
        original_platform: *mut c_void,
    }
    impl ApplicationQuitGate {
        pub fn install() -> Result<Self, QuitError> {
            let mtm = MainThreadMarker::new().ok_or(QuitError::WrongThread)?;
            let app = NSApplication::sharedApplication(mtm);
            let application_object: &AnyObject = app.as_ref();
            validated_application_slot(application_object.class())?;
            let Some(original) = app.delegate() else {
                emit_structure(format_args!(
                    "Native deferred quit structure: stage=delegate_present app_class=GPUIApplication delegate_present=false"
                ));
                return Err(QuitError::MissingDelegate);
            };
            if original.respondsToSelector(sel!(applicationShouldTerminate:)) {
                emit_structure(format_args!(
                    "Native deferred quit structure: stage=termination_hook delegate_class={} existing_hook=true",
                    bounded_structure(original_object(&original).class().name().to_bytes()),
                ));
                return Err(QuitError::ExistingTerminationHook);
            }
            if original_object(&original).class().name() != c"GPUIApplicationDelegate" {
                emit_structure(format_args!(
                    "Native deferred quit structure: stage=delegate_class actual={} expected=GPUIApplicationDelegate",
                    bounded_structure(original_object(&original).class().name().to_bytes()),
                ));
                return Err(QuitError::UnsupportedDelegate);
            }
            let original_platform = platform(original_object(&original))?;
            if original_platform.is_null() {
                emit_structure(format_args!(
                    "Native deferred quit structure: stage=platform_pointer delegate_class=GPUIApplicationDelegate delegate_platform_null=true app_platform_checked=false"
                ));
                return Err(QuitError::UnsupportedDelegate);
            }
            let app_platform = application_platform(app.as_ref())?;
            if app_platform != original_platform {
                emit_structure(format_args!(
                    "Native deferred quit structure: stage=platform_pointer app_class=GPUIApplication delegate_class=GPUIApplicationDelegate delegate_platform_null=false app_platform_null={} platform_equal=false",
                    app_platform.is_null(),
                ));
                return Err(QuitError::UnsupportedDelegate);
            }
            static NEXT_GATE: AtomicU64 = AtomicU64::new(1);
            let gate = NEXT_GATE
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .map_err(|_| QuitError::RequestExhausted)?;
            Self::attach(
                NativeHost::Application(app),
                original,
                gate,
                original_platform,
            )
        }
        fn attach(
            app: NativeHost,
            original: Retained<ProtocolObject<dyn NSApplicationDelegate>>,
            gate: u64,
            original_platform: *mut c_void,
        ) -> Result<Self, QuitError> {
            let original_identity = Retained::as_ptr(&original);
            let proxy = new_proxy(original, gate, original_platform)?;
            if !app
                .delegate()
                .is_some_and(|current| Retained::as_ptr(&current) == original_identity)
            {
                return Err(QuitError::DelegateChanged);
            }
            app.set_delegate(ProtocolObject::from_ref(&*proxy));
            if matches!(&app, NativeHost::Application(_)) {
                emit_structure(format_args!(
                    "Native deferred quit structure: stage=proxy_attach retained_proxy=true delegate_replacement_sent=true"
                ));
            }
            Ok(Self {
                app,
                proxy,
                original_platform,
            })
        }
        #[cfg(test)]
        pub(super) fn fixture(app: Retained<AnyObject>, gate: u64) -> Result<Self, QuitError> {
            MainThreadMarker::new().ok_or(QuitError::WrongThread)?;
            let app = NativeHost::Fixture(app);
            let original = app.delegate().ok_or(QuitError::MissingDelegate)?;
            let pointer = platform(original_object(&original))?;
            if pointer.is_null() || platform(app.object())? != pointer {
                return Err(QuitError::UnsupportedDelegate);
            }
            Self::attach(app, original, gate, pointer)
        }
        fn check_delegate(&self) -> Result<(), QuitError> {
            if MainThreadMarker::new().is_none() {
                return Err(QuitError::WrongThread);
            }
            if self.app.delegate().is_some_and(|d| {
                std::ptr::eq(
                    Retained::as_ptr(&d).cast::<AnyObject>(),
                    Retained::as_ptr(&self.proxy).cast::<AnyObject>(),
                )
            }) {
                Ok(())
            } else {
                Err(QuitError::DelegateChanged)
            }
        }
        pub fn take_request(&self) -> Result<Option<QuitRequest>, QuitError> {
            self.check_delegate()?;
            Ok(self.proxy.ivars().requests.take())
        }
        pub fn is_pending(&self) -> bool {
            self.proxy.ivars().requests.pending()
        }
        pub fn reply(&self, request: QuitRequest, decision: QuitDecision) -> Result<(), QuitError> {
            self.check_delegate()?;
            self.proxy.ivars().requests.begin_reply(request, decision)?;
            self.app.reply(decision == QuitDecision::Approve);
            self.proxy.ivars().requests.finish_reply(decision);
            Ok(())
        }
        fn synchronize_returned_loop(&self) -> Result<(), QuitError> {
            synchronize_platform_pointers(
                self.app.platform()?,
                self.proxy.as_ref(),
                original_object(&self.proxy.ivars().original),
                self.original_platform,
            )
        }
    }

    impl Drop for ApplicationQuitGate {
        fn drop(&mut self) {
            // Caller must keep this guard through irreversible runtime stop;
            // dropping a pending gate is cancellation, never success.
            if self.check_delegate().is_ok() && self.synchronize_returned_loop().is_ok() {
                if self.is_pending() {
                    self.proxy.ivars().requests.phase.set(Phase::Cancelling);
                    self.app.reply(false);
                }
                // Cancellation can change delegate OR platform ownership.
                if self.check_delegate().is_ok() && self.synchronize_returned_loop().is_ok() {
                    self.app.set_delegate(&self.proxy.ivars().original);
                }
            }
        }
    }
}
#[cfg(target_os = "macos")]
pub use mac::ApplicationQuitGate;

#[cfg(not(target_os = "macos"))]
pub struct ApplicationQuitGate(std::marker::PhantomData<std::rc::Rc<()>>);
#[cfg(not(target_os = "macos"))]
impl ApplicationQuitGate {
    pub fn install() -> Result<Self, QuitError> {
        Err(QuitError::UnsupportedPlatform)
    }
    pub fn take_request(&self) -> Result<Option<QuitRequest>, QuitError> {
        Err(QuitError::UnsupportedPlatform)
    }
    pub fn is_pending(&self) -> bool {
        false
    }
    pub fn reply(&self, _request: QuitRequest, _decision: QuitDecision) -> Result<(), QuitError> {
        Err(QuitError::UnsupportedPlatform)
    }
}
#[cfg(test)]
#[path = "application_quit_tests.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "application_quit_diagnostic_tests.rs"]
mod diagnostic_tests;
