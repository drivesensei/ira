//! Scoped HWND UI Automation fragment provider. Queries read a cached immutable tree;
//! actions enqueue generation-checked commands and never enter the actor or GPUI entities.
#![allow(non_snake_case)]
use super::{
    AccessibilityIntent, ActionSink, PublishedOutcome, Rejection, RetiredPublication,
    model::{
        Action, Capability, FrameKey, MaterializedNodes, Node, NodeId, PreparedFrame, Rect, Role,
        SemanticTree,
    },
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    collections::BTreeMap,
    marker::PhantomData,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::ClientToScreen,
        System::{
            Com::SAFEARRAY,
            Ole::{SafeArrayCreateVector, SafeArrayDestroy, SafeArrayPutElement},
            Threading::GetCurrentThreadId,
            Variant::*,
        },
        UI::{
            Accessibility::*,
            HiDpi::GetDpiForWindow,
            Shell::{DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{GetWindowThreadProcessId, WM_GETOBJECT, WM_NCDESTROY},
        },
    },
    core::{BOOL, BSTR, Error, HRESULT, IUnknown, Interface, PCWSTR, Result, implement},
};
const SUBCLASS_ID: usize = 0x4952414158;
struct Cached {
    tree: Arc<SemanticTree>,
    prepared: Option<Arc<PreparedFrame>>,
    frames: BTreeMap<NodeId, Rect>,
}
struct State {
    cache: Mutex<Cached>,
    materialized: Arc<MaterializedNodes>,
    sink: ActionSink,
    hwnd: isize,
    closing: AtomicBool,
    hook_owned: AtomicBool,
    thread: u32,
}
fn unavailable() -> Error {
    Error::from_hresult(HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32))
}
fn unsupported() -> Error {
    Error::from_hresult(E_NOINTERFACE)
}
fn dispatch_error(e: Rejection) -> Error {
    Error::from_hresult(match e {
        Rejection::Closing | Rejection::Stale => HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32),
        Rejection::Disabled => HRESULT(UIA_E_ELEMENTNOTENABLED as i32),
        Rejection::InvalidRange => E_INVALIDARG,
        _ => HRESULT(UIA_E_NOTSUPPORTED as i32),
    })
}
impl State {
    fn tree(&self) -> Result<Arc<SemanticTree>> {
        if self.closing.load(Ordering::Acquire) {
            return Err(unavailable());
        };
        self.cache
            .try_lock()
            .map(|c| c.tree.clone())
            .map_err(|_| unavailable())
    }
    fn publication(&self) -> Result<(Arc<SemanticTree>, Option<Arc<PreparedFrame>>)> {
        let cache = self.cache.try_lock().map_err(|_| unavailable())?;
        if self.closing.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        Ok((cache.tree.clone(), cache.prepared.clone()))
    }
    fn send_observed(
        &self,
        id: NodeId,
        action: Action,
        tree: &SemanticTree,
        prepared: Option<&PreparedFrame>,
    ) -> Result<()> {
        let intent = AccessibilityIntent {
            node: id,
            stamp: tree.stamp,
            action,
        };
        match prepared {
            Some(frame) => self.sink.try_dispatch_prepared(intent, frame.key.request),
            None => self.sink.try_dispatch(intent),
        }
        .map_err(dispatch_error)
    }
    fn simple(self: &Arc<Self>, id: NodeId) -> Result<IRawElementProviderSimple> {
        self.materialized.record(id).map_err(dispatch_error)?;
        Ok(new_provider(self.clone(), id))
    }
    fn fragment(self: &Arc<Self>, id: NodeId) -> Result<IRawElementProviderFragment> {
        self.simple(id)?.cast()
    }
    fn send(&self, id: NodeId, action: Action) -> Result<()> {
        if self.closing.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        let (stamp, key) = {
            let cache = self.cache.try_lock().map_err(|_| unavailable())?;
            (
                cache.tree.stamp,
                cache.prepared.as_ref().map(|f| f.key.request),
            )
        };
        let intent = AccessibilityIntent {
            node: id,
            stamp,
            action,
        };
        match key {
            Some(key) => self.sink.try_dispatch_prepared(intent, key),
            None => self.sink.try_dispatch(intent),
        }
        .map_err(dispatch_error)
    }
    fn frame(&self, id: NodeId) -> Result<Rect> {
        if self.closing.load(Ordering::Acquire) {
            return Err(unavailable());
        };
        let cache = self.cache.try_lock().map_err(|_| unavailable())?;
        if self.closing.load(Ordering::Acquire) || cache.tree.query_node(id).is_none() {
            return Err(unavailable());
        }
        cache
            .frames
            .get(&id)
            .copied()
            .ok_or_else(|| Error::from_hresult(HRESULT(UIA_E_NOTSUPPORTED as i32)))
    }
}
#[implement(
    IRawElementProviderSimple,
    IRawElementProviderFragment,
    IRawElementProviderFragmentRoot,
    IInvokeProvider,
    ISelectionProvider,
    ISelectionItemProvider,
    IScrollItemProvider,
    IValueProvider
)]
struct Provider {
    state: Arc<State>,
    id: NodeId,
}
impl Provider_Impl {
    fn node(&self) -> Result<Node> {
        let tree = self.state.tree()?;
        tree.query_node(self.id)
            .map(Node::clone_metadata)
            .ok_or_else(unavailable)
    }
    fn unknown<I: Interface>(&self) -> Result<IUnknown> {
        let p = self.state.simple(self.id)?;
        p.cast::<I>()?.cast()
    }
}
#[allow(non_upper_case_globals)] // Exact pinned Win32 constant names in patterns.
impl IRawElementProviderSimple_Impl for Provider_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        self.node()?;
        Ok(ProviderOptions_ServerSideProvider | ProviderOptions_UseComThreading)
    }
    fn GetPatternProvider(&self, id: UIA_PATTERN_ID) -> Result<IUnknown> {
        let (tree, prepared) = self.state.publication()?;
        let n = tree.query_node(self.id).ok_or_else(unavailable)?;
        match id {
            UIA_InvokePatternId
                if n.capabilities.iter().any(|c| {
                    matches!(
                        c,
                        Capability::Activate
                            | Capability::Pause
                            | Capability::Cancel
                            | Capability::Dismiss
                    )
                }) =>
            {
                self.unknown::<IInvokeProvider>()
            }
            UIA_SelectionPatternId
                if n.role == Role::List && {
                    prepared.as_ref().map_or_else(
                        || {
                            n.children.iter().any(|id| {
                                tree.query_node(*id).is_some_and(|n| {
                                    n.capabilities.contains(&Capability::Selection)
                                })
                            })
                        },
                        |frame| frame.semantic.selection_containers.contains(&self.id),
                    )
                } =>
            {
                self.unknown::<ISelectionProvider>()
            }
            UIA_SelectionItemPatternId if n.capabilities.contains(&Capability::Selection) => {
                self.unknown::<ISelectionItemProvider>()
            }
            UIA_ScrollItemPatternId if n.capabilities.contains(&Capability::Focus) => {
                self.unknown::<IScrollItemProvider>()
            }
            UIA_ValuePatternId if matches!(n.role, Role::TextField | Role::TextArea) => {
                self.unknown::<IValueProvider>()
            }
            // Full Text/TextRange is deliberately not advertised until real glyph-range geometry exists.
            _ => Err(unsupported()),
        }
    }
    fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> {
        let n = self.node()?;
        Ok(match id {
            UIA_NamePropertyId => VARIANT::from(n.name.as_str()),
            UIA_AutomationIdPropertyId => {
                VARIANT::from(format!("ira-{}-{}", n.id.window, n.id.serial).as_str())
            }
            UIA_ControlTypePropertyId => VARIANT::from(
                match n.role {
                    Role::Window => UIA_WindowControlTypeId,
                    Role::Group => UIA_GroupControlTypeId,
                    Role::List => UIA_ListControlTypeId,
                    Role::Row => UIA_ListItemControlTypeId,
                    Role::Button => UIA_ButtonControlTypeId,
                    Role::Status => UIA_TextControlTypeId,
                    Role::Dialog => UIA_PaneControlTypeId,
                    Role::TextField | Role::TextArea => UIA_EditControlTypeId,
                }
                .0,
            ),
            UIA_HelpTextPropertyId => VARIANT::from(n.help.as_deref().unwrap_or("")),
            UIA_IsEnabledPropertyId => VARIANT::from(n.enabled),
            UIA_IsKeyboardFocusablePropertyId => VARIANT::from(n.focusable),
            UIA_HasKeyboardFocusPropertyId => {
                VARIANT::from(self.state.tree()?.focused == Some(self.id))
            }
            UIA_IsOffscreenPropertyId => VARIANT::from(self.state.frame(self.id).is_err()),
            UIA_IsControlElementPropertyId | UIA_IsContentElementPropertyId => VARIANT::from(true),
            UIA_SelectionItemIsSelectedPropertyId => VARIANT::from(n.selected),
            UIA_ValueIsReadOnlyPropertyId => VARIANT::from(n.read_only),
            UIA_ValueValuePropertyId => VARIANT::from(n.value.as_deref().unwrap_or("")),
            _ => VARIANT::default(),
        })
    }
    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        let tree = self.state.tree()?;
        if self.id != tree.root {
            return Err(unsupported());
        };
        // SAFETY: bridge stores its actual live HWND and tears down on WM_NCDESTROY.
        unsafe { UiaHostProviderFromHwnd(HWND(self.state.hwnd as *mut _)) }
    }
}
#[allow(non_upper_case_globals)] // Exact pinned Win32 constant names in patterns.
impl IRawElementProviderFragment_Impl for Provider_Impl {
    fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        let cache = self.state.cache.try_lock().map_err(|_| unavailable())?;
        if self.state.closing.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        let direction = match direction {
            NavigateDirection_Parent => super::model::NavigationDirection::Parent,
            NavigateDirection_FirstChild => super::model::NavigationDirection::FirstChild,
            NavigateDirection_LastChild => super::model::NavigationDirection::LastChild,
            NavigateDirection_NextSibling => super::model::NavigationDirection::NextSibling,
            NavigateDirection_PreviousSibling => super::model::NavigationDirection::PreviousSibling,
            _ => return Err(Error::from_hresult(E_INVALIDARG)),
        };
        let id = super::model::navigation_destination(
            &cache.tree,
            cache.prepared.as_ref().map(|frame| &*frame.semantic),
            self.id,
            direction,
        )
        .map_err(|error| {
            if error == Rejection::Unsupported {
                unsupported()
            } else {
                dispatch_error(error)
            }
        })?;
        drop(cache);
        self.state.fragment(id)
    }
    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        self.node()?;
        ints(&[
            UiaAppendRuntimeId as i32,
            self.id.window as i32,
            (self.id.window >> 32) as i32,
            self.id.serial as i32,
            (self.id.serial >> 32) as i32,
        ])
    }
    fn BoundingRectangle(&self) -> Result<UiaRect> {
        self.node()?;
        let r = self.state.frame(self.id)?;
        Ok(UiaRect {
            left: r.x,
            top: r.y,
            width: r.width,
            height: r.height,
        })
    }
    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        self.node()?;
        Ok(std::ptr::null_mut())
    }
    fn SetFocus(&self) -> Result<()> {
        self.state.send(self.id, Action::Focus)
    }
    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        let t = self.state.tree()?;
        t.query_node(self.id).ok_or_else(unavailable)?;
        self.state.simple(t.root)?.cast()
    }
}
impl IRawElementProviderFragmentRoot_Impl for Provider_Impl {
    fn ElementProviderFromPoint(&self, x: f64, y: f64) -> Result<IRawElementProviderFragment> {
        let cache = self.state.cache.try_lock().map_err(|_| unavailable())?;
        if self.state.closing.load(Ordering::Acquire) || cache.tree.query_node(self.id).is_none() {
            return Err(unavailable());
        }
        if let Some(frame) = &cache.prepared {
            let id = frame
                .geometry
                .ordered_ids()
                .find(|id| cache.frames.get(id).is_some_and(|r| r.contains(x, y)))
                .ok_or_else(unsupported)?;
            drop(cache);
            return self.state.fragment(id);
        }
        fn hit(cache: &Cached, id: NodeId, x: f64, y: f64) -> Option<NodeId> {
            let n = cache.tree.nodes.get(&id)?;
            for c in n.children.iter().rev() {
                if let Some(id) = hit(cache, *c, x, y) {
                    return Some(id);
                }
            }
            cache
                .frames
                .get(&id)
                .filter(|r| r.contains(x, y))
                .map(|_| id)
        }
        let id = hit(
            &cache,
            cache.tree.active_modal.unwrap_or(cache.tree.root),
            x,
            y,
        )
        .ok_or_else(unsupported)?;
        drop(cache);
        self.state.fragment(id)
    }
    fn GetFocus(&self) -> Result<IRawElementProviderFragment> {
        let t = self.state.tree()?;
        t.query_node(self.id).ok_or_else(unavailable)?;
        let focused = t.focused.ok_or_else(unsupported)?;
        t.query_node(focused).ok_or_else(unavailable)?;
        self.state.fragment(focused)
    }
}
impl IInvokeProvider_Impl for Provider_Impl {
    fn Invoke(&self) -> Result<()> {
        let (tree, prepared) = self.state.publication()?;
        let n = tree.query_node(self.id).ok_or_else(unavailable)?;
        let action = if n.capabilities.contains(&Capability::Activate) {
            Action::Activate
        } else if n.capabilities.contains(&Capability::Pause) {
            Action::Pause
        } else if n.capabilities.contains(&Capability::Cancel) {
            Action::Cancel
        } else if n.capabilities.contains(&Capability::Dismiss) {
            Action::Dismiss
        } else {
            return Err(unsupported());
        };
        self.state
            .send_observed(self.id, action, &tree, prepared.as_deref())
    }
}
impl ISelectionProvider_Impl for Provider_Impl {
    fn GetSelection(&self) -> Result<*mut SAFEARRAY> {
        let t = self.state.tree()?;
        let n = t.query_node(self.id).ok_or_else(unavailable)?;
        let values: Vec<IUnknown> = n
            .children
            .iter()
            .filter(|id| t.query_node(**id).is_some_and(|n| n.selected))
            .map(|id| self.state.simple(*id)?.cast())
            .collect::<Result<_>>()?;
        interfaces(&values)
    }
    fn CanSelectMultiple(&self) -> Result<BOOL> {
        self.node()?;
        Ok(true.into())
    }
    fn IsSelectionRequired(&self) -> Result<BOOL> {
        self.node()?;
        Ok(false.into())
    }
}
impl ISelectionItemProvider_Impl for Provider_Impl {
    fn Select(&self) -> Result<()> {
        self.state.send(self.id, Action::SelectOnly)
    }
    fn AddToSelection(&self) -> Result<()> {
        self.state.send(self.id, Action::SetSelected(true))
    }
    fn RemoveFromSelection(&self) -> Result<()> {
        self.state.send(self.id, Action::SetSelected(false))
    }
    fn IsSelected(&self) -> Result<BOOL> {
        Ok(self.node()?.selected.into())
    }
    fn SelectionContainer(&self) -> Result<IRawElementProviderSimple> {
        let n = self.node()?;
        self.state.simple(n.parent.ok_or_else(unavailable)?)
    }
}
impl IScrollItemProvider_Impl for Provider_Impl {
    fn ScrollIntoView(&self) -> Result<()> {
        self.state.send(self.id, Action::Reveal)
    }
}
impl IValueProvider_Impl for Provider_Impl {
    fn SetValue(&self, value: &PCWSTR) -> Result<()> {
        // SAFETY: COM supplies a NUL-terminated string valid for this callback.
        let value = unsafe { value.to_string() }?;
        self.state.send(self.id, Action::SetValue(value))
    }
    fn Value(&self) -> Result<BSTR> {
        let n = self.node()?;
        Ok(BSTR::from(n.value.as_deref().unwrap_or("")))
    }
    fn IsReadOnly(&self) -> Result<BOOL> {
        Ok(self.node()?.read_only.into())
    }
}
fn ints(values: &[i32]) -> Result<*mut SAFEARRAY> {
    // SAFETY: array owns copies of each scalar; output ownership passes to UI Automation.
    unsafe {
        let array = SafeArrayCreateVector(VT_I4, 0, values.len() as u32);
        if array.is_null() {
            return Err(Error::from_hresult(E_OUTOFMEMORY));
        };
        for (i, value) in values.iter().enumerate() {
            if let Err(e) = SafeArrayPutElement(array, &(i as i32), value as *const i32 as *const _)
            {
                let _ = SafeArrayDestroy(array);
                return Err(e);
            }
        }
        Ok(array)
    }
}
fn interfaces(values: &[IUnknown]) -> Result<*mut SAFEARRAY> {
    // SAFETY: VT_UNKNOWN insertion AddRefs each interface; destroy releases on failure.
    unsafe {
        let array = SafeArrayCreateVector(VT_UNKNOWN, 0, values.len() as u32);
        if array.is_null() {
            return Err(Error::from_hresult(E_OUTOFMEMORY));
        };
        for (i, value) in values.iter().enumerate() {
            if let Err(e) = SafeArrayPutElement(array, &(i as i32), value.as_raw()) {
                let _ = SafeArrayDestroy(array);
                return Err(e);
            }
        }
        Ok(array)
    }
}
unsafe extern "system" fn subclass(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let pointer = data as *const State;
    // SAFETY: installed subclass holds one Arc reference; callback temporarily retains another.
    let state = unsafe {
        Arc::increment_strong_count(pointer);
        Arc::from_raw(pointer)
    };
    if message == WM_GETOBJECT
        && lparam.0 as i32 == UiaRootObjectId
        && !state.closing.load(Ordering::Acquire)
    {
        if let Ok(t) = state.tree()
            && let Ok(provider) = state.simple(t.root)
        {
            // SAFETY: provider lifetime is transferred by UIA's marshaling; actual HWND belongs to this callback.
            return unsafe { UiaReturnRawElementProvider(hwnd, wparam, lparam, &provider) };
        }
    }
    if message == WM_NCDESTROY {
        state.closing.store(true, Ordering::Release);
        state.sink.close();
        if state.hook_owned.swap(false, Ordering::AcqRel) {
            // SAFETY: removing the exact installed hook then releasing its single Arc reference.
            unsafe {
                let _ = RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID);
                drop(Arc::from_raw(pointer));
            }
        }
    }
    // SAFETY: all unrelated messages preserve the original chain/responder behavior.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}
/// UI-thread-only lifecycle owner; COM query objects remain independently reference counted.
pub struct NativeBridge {
    state: Arc<State>,
    _main_thread: PhantomData<Rc<()>>,
}
impl NativeBridge {
    pub fn attach(window: &impl HasWindowHandle, sink: ActionSink) -> Result<Self> {
        Self::attach_impl(window, sink, true)
    }
    /// Attach the actual immutable sink tree without a foreground logical-node scan.
    /// Geometry starts empty; install the first ACK-based prepared frame separately.
    pub fn attach_unpublished(window: &impl HasWindowHandle, sink: ActionSink) -> Result<Self> {
        Self::attach_impl(window, sink, false)
    }
    fn attach_impl(
        window: &impl HasWindowHandle,
        sink: ActionSink,
        publish_initial: bool,
    ) -> Result<Self> {
        let handle =
            HasWindowHandle::window_handle(window).map_err(|_| Error::from_hresult(E_HANDLE))?;
        let RawWindowHandle::Win32(raw) = handle.as_raw() else {
            return Err(Error::from_hresult(E_HANDLE));
        };
        let hwnd = HWND(raw.hwnd.get() as *mut _);
        // SAFETY: this validates ownership of the actual borrowed HWND; no handles are synthesized.
        let thread = unsafe { GetCurrentThreadId() };
        if unsafe { GetWindowThreadProcessId(hwnd, None) } != thread {
            return Err(Error::from_hresult(RPC_E_WRONG_THREAD));
        };
        let mut existing = 0usize;
        if unsafe { GetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, Some(&mut existing)) }
            .as_bool()
        {
            return Err(Error::from_hresult(E_FAIL));
        };
        let tree = sink.current().map_err(dispatch_error)?;
        let state = Arc::new(State {
            cache: Mutex::new(Cached {
                tree,
                prepared: None,
                frames: BTreeMap::new(),
            }),
            sink,
            materialized: Arc::new(MaterializedNodes::default()),
            hwnd: raw.hwnd.get(),
            closing: AtomicBool::new(false),
            hook_owned: AtomicBool::new(true),
            thread,
        });
        let pointer = Arc::into_raw(state.clone());
        // SAFETY: the hook keeps its Arc until exact removal or WM_NCDESTROY.
        if !unsafe { SetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, pointer as usize) }
            .as_bool()
        {
            unsafe { drop(Arc::from_raw(pointer)) };
            state.hook_owned.store(false, Ordering::Release);
            return Err(Error::from_win32());
        };
        let mut bridge = Self {
            state,
            _main_thread: PhantomData,
        };
        if publish_initial {
            let initial = bridge.state.tree()?;
            bridge.publish(initial)?;
        }
        Ok(bridge)
    }
    pub fn publish(&mut self, tree: Arc<SemanticTree>) -> Result<()> {
        if unsafe { GetCurrentThreadId() } != self.state.thread {
            return Err(Error::from_hresult(RPC_E_WRONG_THREAD));
        };
        if self.state.closing.load(Ordering::Acquire) {
            return Err(unavailable());
        };
        let hwnd = HWND(self.state.hwnd as *mut _);
        let mut origin = POINT::default();
        // SAFETY: only query the live window on its creating thread. UIA rectangles are physical screen pixels.
        if !unsafe { ClientToScreen(hwnd, &mut origin) }.as_bool() {
            return Err(Error::from_win32());
        };
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        if dpi == 0 {
            return Err(Error::from_win32());
        };
        let scale = dpi as f64 / 96.0;
        let mut frames = BTreeMap::new();
        for n in tree.nodes.values() {
            if let Some(g) = n.geometry {
                let r = g.visible;
                frames.insert(
                    n.id,
                    Rect {
                        x: origin.x as f64 + r.x * scale,
                        y: origin.y as f64 + r.y * scale,
                        width: r.width * scale,
                        height: r.height * scale,
                    },
                );
            }
        }
        let old = {
            let mut slot = self.state.cache.try_lock().map_err(|_| unavailable())?;
            self.state
                .sink
                .publish(tree.clone())
                .map_err(dispatch_error)?;
            let old = slot.tree.clone();
            *slot = Cached {
                tree: tree.clone(),
                prepared: None,
                frames,
            };
            old
        };
        // Notifications describe installed coherent state; failed native delivery is surfaced to caller.
        if old.focused != tree.focused {
            if let Some(id) = tree.focused {
                unsafe {
                    UiaRaiseAutomationEvent(
                        &self.state.simple(id)?,
                        UIA_AutomationFocusChangedEventId,
                    )
                }?;
            }
        }
        for (id, n) in &tree.nodes {
            if let Some(before) = old.nodes.get(id) {
                let provider = self.state.simple(*id)?;
                // SAFETY: provider is independently retained; no cache/sink lock is held across UIA calls.
                if before.selected != n.selected {
                    unsafe {
                        UiaRaiseAutomationPropertyChangedEvent(
                            &provider,
                            UIA_SelectionItemIsSelectedPropertyId,
                            &VARIANT::from(before.selected),
                            &VARIANT::from(n.selected),
                        )
                    }?;
                }
                if before.value != n.value {
                    unsafe {
                        UiaRaiseAutomationPropertyChangedEvent(
                            &provider,
                            UIA_ValueValuePropertyId,
                            &VARIANT::from(before.value.as_deref().unwrap_or("")),
                            &VARIANT::from(n.value.as_deref().unwrap_or("")),
                        )
                    }?;
                }
            }
        }
        if old.nodes.len() != tree.nodes.len()
            || old.active_modal != tree.active_modal
            || old.nodes.iter().any(|(id, n)| {
                tree.nodes
                    .get(id)
                    .is_none_or(|after| after.children != n.children)
            })
        {
            // SAFETY: ChildrenInvalidated has no required runtime-id payload.
            unsafe {
                UiaRaiseStructureChangedEvent(
                    &self.state.simple(tree.root)?,
                    StructureChangeType_ChildrenInvalidated,
                    std::ptr::null_mut(),
                    0,
                )
            }?;
        }
        Ok(())
    }
    pub fn invalidate_geometry(&mut self) -> Result<()> {
        if unsafe { GetCurrentThreadId() } != self.state.thread {
            return Err(Error::from_hresult(RPC_E_WRONG_THREAD));
        }
        self.state
            .cache
            .try_lock()
            .map_err(|_| unavailable())?
            .frames
            .clear();
        Ok(())
    }
    pub fn materialized_nodes(&self) -> Arc<MaterializedNodes> {
        self.state.materialized.clone()
    }
    /// UI-thread conversion and coherent sparse commit; immutable callbacks use try_lock.
    pub fn publish_prepared(
        &mut self,
        frame: Arc<PreparedFrame>,
        expected: FrameKey,
    ) -> Result<PublishedOutcome<Error>> {
        if unsafe { GetCurrentThreadId() } != self.state.thread {
            return Err(Error::from_hresult(RPC_E_WRONG_THREAD));
        }
        if self.state.closing.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        if frame.key != expected || !frame.notifications.materialized_only {
            return Err(dispatch_error(Rejection::Stale));
        }
        let hwnd = HWND(self.state.hwnd as *mut _);
        let mut origin = POINT::default();
        // SAFETY: live HWND conversion runs on its creating thread, never on the worker.
        if !unsafe { ClientToScreen(hwnd, &mut origin) }.as_bool() {
            return Err(Error::from_win32());
        }
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        if dpi == 0 {
            return Err(Error::from_win32());
        }
        let scale = dpi as f64 / 96.0;
        let frames: BTreeMap<_, _> = frame
            .geometry
            .nodes
            .iter()
            .map(|(id, g)| {
                (
                    *id,
                    Rect {
                        x: origin.x as f64 + g.visible.x * scale,
                        y: origin.y as f64 + g.visible.y * scale,
                        width: g.visible.width * scale,
                        height: g.visible.height * scale,
                    },
                )
            })
            .collect();
        let converted_frames = frames.len();
        let retired = {
            let mut cache = self.state.cache.try_lock().map_err(|_| unavailable())?;
            self.state
                .materialized
                .with_revision(frame.notifications.materialization_revision, || {
                    self.state.sink.install_prepared(&frame, expected, || {
                        *cache = Cached {
                            tree: frame.semantic.tree.clone(),
                            prepared: Some(frame.clone()),
                            frames,
                        };
                    })
                })
                .map_err(dispatch_error)?
        };
        let mut notification_visits = 0;
        // Delivery failure follows commit: return installed outcome so runtime still ACKs it.
        let notification_error = (|| -> Result<()> {
            let plan = &frame.notifications;
            if let Some(id) = plan.focused {
                notification_visits += 1;
                unsafe {
                    UiaRaiseAutomationEvent(
                        &self.state.simple(id)?,
                        UIA_AutomationFocusChangedEventId,
                    )
                }?;
            }
            for (id, (before, after)) in &plan.selected_items {
                notification_visits += 1;
                unsafe {
                    UiaRaiseAutomationPropertyChangedEvent(
                        &self.state.simple(*id)?,
                        UIA_SelectionItemIsSelectedPropertyId,
                        &VARIANT::from(*before),
                        &VARIANT::from(*after),
                    )
                }?;
            }
            for (id, (before, after)) in &plan.values {
                notification_visits += 1;
                unsafe {
                    UiaRaiseAutomationPropertyChangedEvent(
                        &self.state.simple(*id)?,
                        UIA_ValueValuePropertyId,
                        &VARIANT::from(before.as_deref().unwrap_or("")),
                        &VARIANT::from(after.as_deref().unwrap_or("")),
                    )
                }?;
            }
            if plan.structure_changed {
                notification_visits += 1;
                unsafe {
                    UiaRaiseStructureChangedEvent(
                        &self.state.simple(frame.semantic.tree.root)?,
                        StructureChangeType_ChildrenInvalidated,
                        std::ptr::null_mut(),
                        0,
                    )
                }?;
            }
            Ok(())
        })()
        .err();
        Ok(PublishedOutcome {
            publication_seq: frame.publication_seq,
            retired,
            notification_error,
            converted_frames,
            notification_visits,
            cleanup_visits: 0,
        })
    }
    /// Windows has no retained native-element map; pure ownership still retires on worker.
    pub fn detach_prepared(&mut self) -> Result<RetiredPublication> {
        self.detach()?;
        let mut cache = self.state.cache.try_lock().map_err(|_| unavailable())?;
        let retired = self.state.sink.close_and_retire().map_err(dispatch_error)?;
        cache.tree = self.state.sink.current().map_err(dispatch_error)?;
        cache.prepared = None;
        cache.frames.clear();
        Ok(retired)
    }
    pub fn drain_native_retirement(&mut self, _budget: usize) -> Result<usize> {
        if unsafe { GetCurrentThreadId() } != self.state.thread {
            return Err(Error::from_hresult(RPC_E_WRONG_THREAD));
        }
        Ok(0)
    }
    pub fn native_retirement_complete(&self) -> bool {
        self.state.closing.load(Ordering::Acquire)
    }
    pub fn detach(&mut self) -> Result<()> {
        if unsafe { GetCurrentThreadId() } != self.state.thread {
            return Err(Error::from_hresult(RPC_E_WRONG_THREAD));
        };
        self.state.closing.store(true, Ordering::Release);
        self.state.sink.close();
        if self.state.hook_owned.load(Ordering::Acquire) {
            let hwnd = HWND(self.state.hwnd as *mut _);
            // SAFETY: exact scoped hook and ID, UI-thread removal; no unrelated WNDPROC replacement.
            if !unsafe { RemoveWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID) }.as_bool() {
                return Err(Error::from_win32());
            };
            if self.state.hook_owned.swap(false, Ordering::AcqRel) {
                unsafe { drop(Arc::from_raw(Arc::as_ptr(&self.state))) };
            }
        }
        Ok(())
    }
}
impl Drop for NativeBridge {
    fn drop(&mut self) {
        let _ = self.detach();
    }
}

// windows 0.61.3's implementation traits require non-null interface Result<T> even for
// COM methods whose documented empty result is S_OK/null. These small binding-vtable
// shims initialize outputs and translate the private E_NOINTERFACE empty-result sentinel.
// They preserve all genuine stale/disabled/error HRESULTs. No nullable interface is forged.
unsafe extern "system" fn nullable_pattern(
    this: *mut std::ffi::c_void,
    id: UIA_PATTERN_ID,
    out: *mut *mut std::ffi::c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe {
        out.write(std::ptr::null_mut());
    }
    let hr = unsafe {
        (Provider_Impl::VTABLE_INTERFACE1_IRAWELEMENTPROVIDERS.GetPatternProvider)(this, id, out)
    };
    if hr == E_NOINTERFACE { S_OK } else { hr }
}
unsafe extern "system" fn nullable_host(
    this: *mut std::ffi::c_void,
    out: *mut *mut std::ffi::c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe {
        out.write(std::ptr::null_mut());
    }
    let hr = unsafe {
        (Provider_Impl::VTABLE_INTERFACE1_IRAWELEMENTPROVIDERS.HostRawElementProvider)(this, out)
    };
    if hr == E_NOINTERFACE { S_OK } else { hr }
}
unsafe extern "system" fn nullable_navigate(
    this: *mut std::ffi::c_void,
    direction: NavigateDirection,
    out: *mut *mut std::ffi::c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe {
        out.write(std::ptr::null_mut());
    }
    let hr = unsafe {
        (Provider_Impl::VTABLE_INTERFACE2_IRAWELEMENTPROVIDERF.Navigate)(this, direction, out)
    };
    if hr == E_NOINTERFACE { S_OK } else { hr }
}
unsafe extern "system" fn nullable_point(
    this: *mut std::ffi::c_void,
    x: f64,
    y: f64,
    out: *mut *mut std::ffi::c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe {
        out.write(std::ptr::null_mut());
    }
    let hr = unsafe {
        (Provider_Impl::VTABLE_INTERFACE3_IRAWELEMENTPROVIDERF.ElementProviderFromPoint)(
            this, x, y, out,
        )
    };
    if hr == E_NOINTERFACE { S_OK } else { hr }
}
unsafe extern "system" fn nullable_focus(
    this: *mut std::ffi::c_void,
    out: *mut *mut std::ffi::c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    unsafe {
        out.write(std::ptr::null_mut());
    }
    let hr = unsafe { (Provider_Impl::VTABLE_INTERFACE3_IRAWELEMENTPROVIDERF.GetFocus)(this, out) };
    if hr == E_NOINTERFACE { S_OK } else { hr }
}
static SIMPLE_VTABLE: IRawElementProviderSimple_Vtbl = {
    let mut v = Provider_Impl::VTABLE_INTERFACE1_IRAWELEMENTPROVIDERS;
    v.GetPatternProvider = nullable_pattern;
    v.HostRawElementProvider = nullable_host;
    v
};
static FRAGMENT_VTABLE: IRawElementProviderFragment_Vtbl = {
    let mut v = Provider_Impl::VTABLE_INTERFACE2_IRAWELEMENTPROVIDERF;
    v.Navigate = nullable_navigate;
    v
};
static ROOT_VTABLE: IRawElementProviderFragmentRoot_Vtbl = {
    let mut v = Provider_Impl::VTABLE_INTERFACE3_IRAWELEMENTPROVIDERF;
    v.ElementProviderFromPoint = nullable_point;
    v.GetFocus = nullable_focus;
    v
};
fn new_provider(state: Arc<State>, id: NodeId) -> IRawElementProviderSimple {
    // Generated pinned ComObjectInner constructor initializes reference count to one.
    let mut outer = Provider { state, id }.into_outer();
    outer.interface1_irawelementproviders = &SIMPLE_VTABLE;
    outer.interface2_irawelementproviderf = &FRAGMENT_VTABLE;
    outer.interface3_irawelementproviderf = &ROOT_VTABLE;
    let pointer = std::ptr::NonNull::from(Box::leak(Box::new(outer)));
    // SAFETY: sole owned boxed generated COM object, refcount=1, static ABI-identical vtables.
    unsafe { windows::core::ComObject::<Provider>::from_raw(pointer) }.into_interface()
}
