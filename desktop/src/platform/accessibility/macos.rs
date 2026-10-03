//! AppKit accessibility elements attached to GPUI's actual NSView. All AppKit use is main-thread-only.
use super::{
    AccessibilityIntent, ActionSink, PublishedOutcome, Rejection, RetiredPublication,
    model::{
        Action, Capability, FrameKey, MaterializedNodes, Node, NodeId, PreparedFrame, Rect, Role,
        SemanticTree,
    },
};
use objc2::{
    DefinedClass, MainThreadOnly, Message, define_class, msg_send, rc::Retained, runtime::AnyObject,
};
use objc2_app_kit::*;
use objc2_foundation::{
    MainThreadMarker, NSArray, NSObjectProtocol, NSPoint, NSRange, NSRect, NSSize, NSString,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::{Rc, Weak},
    sync::Arc,
};
#[derive(Debug)]
pub enum BridgeError {
    WrongThread,
    WrongHandle,
    InvalidView,
    AlreadyAttached,
    NoNativeWindow,
    Dispatch(Rejection),
}
struct MacState {
    tree: RefCell<Arc<SemanticTree>>,
    prepared: RefCell<Option<Arc<PreparedFrame>>>,
    materialized: Arc<MaterializedNodes>,
    cleanup_cursor: Cell<Option<NodeId>>,
    frames: RefCell<BTreeMap<NodeId, NSRect>>,
    elements: RefCell<BTreeMap<NodeId, Retained<AxElement>>>,
    view: Retained<NSView>,
    sink: ActionSink,
    attached: Cell<bool>,
}
struct ElementIvars {
    id: NodeId,
    state: Weak<MacState>,
}
define_class!(
    // SAFETY: NSAccessibilityElement has no documented extra subclassing requirements.
    #[unsafe(super(NSAccessibilityElement))]
    #[thread_kind = MainThreadOnly]
    #[name = "IRAAccessibilityElement"]
    #[ivars = ElementIvars]
    struct AxElement;
    unsafe impl NSObjectProtocol for AxElement {}
    unsafe impl NSAccessibility for AxElement {}
    impl AxElement {
        #[unsafe(method(isAccessibilityElement))]
        fn ax_element(&self)->bool { self.node().is_some() }
        #[unsafe(method_id(accessibilityRole))]
        fn ax_role(&self)->Option<Retained<NSString>> { self.node().map(|n|role(n.role).retain()) }
        #[unsafe(method_id(accessibilityLabel))]
        fn ax_label(&self)->Option<Retained<NSString>> { self.node().map(|n|NSString::from_str(&n.name)) }
        #[unsafe(method_id(accessibilityIdentifier))]
        fn ax_identifier(&self)->Option<Retained<NSString>> { self.node().map(|n|NSString::from_str(&format!("ira-{}-{}",n.id.window,n.id.serial))) }
        #[unsafe(method_id(accessibilityHelp))]
        fn ax_help(&self)->Option<Retained<NSString>> { self.node().and_then(|n|n.help.map(|h|NSString::from_str(&h))) }
        #[unsafe(method_id(accessibilityValue))]
        fn ax_value(&self)->Option<Retained<AnyObject>> { self.node().and_then(|n|n.value.map(|v|NSString::from_str(&v).into_super().into_super())) }
        #[unsafe(method(setAccessibilityValue:))]
        fn ax_set_value(&self,value:Option<&AnyObject>) {if let Some(value)=value.and_then(|v|v.downcast_ref::<NSString>()) {self.dispatch(Action::SetValue(value.to_string()));}}
        #[unsafe(method(isAccessibilityEnabled))]
        fn ax_enabled(&self)->bool {self.node().is_some_and(|n|n.enabled)}
        #[unsafe(method(isAccessibilitySelected))]
        fn ax_selected(&self)->bool {self.node().is_some_and(|n|n.selected)}
        #[unsafe(method(setAccessibilitySelected:))]
        fn ax_set_selected(&self,selected:bool) {self.dispatch(Action::SetSelected(selected));}
        #[unsafe(method(isAccessibilityFocused))]
        fn ax_focused(&self)->bool {self.ivars().state.upgrade().is_some_and(|s|s.is_attached() && s.tree.borrow().focused==Some(self.ivars().id))}
        #[unsafe(method(setAccessibilityFocused:))]
        fn ax_set_focused(&self,focused:bool) {if focused {self.dispatch(Action::Focus);}}
        #[unsafe(method(isAccessibilityModal))]
        fn ax_modal(&self)->bool {self.node().is_some_and(|n|n.role==Role::Dialog)}
        #[unsafe(method(accessibilityFrame))]
        fn ax_frame(&self)->NSRect {
            // A non-materialized/offscreen element has no hit target. AppKit requires a rect return;
            // an empty rect denotes unavailable geometry and is never inserted into the hit map.
            self.node().and_then(|_|self.ivars().state.upgrade()).and_then(|s|s.frames.borrow().get(&self.ivars().id).copied()).unwrap_or_default()
        }
        #[unsafe(method(isAccessibilityHidden))]
        fn ax_hidden(&self)->bool {self.ivars().state.upgrade().is_none_or(|s|!s.is_attached() || s.tree.borrow().query_node(self.ivars().id).is_none())}
        #[unsafe(method_id(accessibilityParent))]
        fn ax_parent(&self)->Option<Retained<AnyObject>> { (|| {
            let state=self.ivars().state.upgrade()?; let node=self.node()?;
            if let Some(parent)=node.parent {state.element(parent).map(|e|e.into_super().into_super().into_super())} else {Some(state.view.clone().into_super().into_super().into_super())}
        })() }
        #[unsafe(method_id(accessibilityChildren))]
        fn ax_children(&self)->Option<Retained<NSArray>> { (|| {
            let state=self.ivars().state.upgrade()?; if !state.is_attached() {return None;} let node=state.tree.borrow().query_node(self.ivars().id)?.clone();
            let elements:Vec<Retained<AnyObject>>=node.children.iter().filter_map(|id|state.element(*id).map(|e|e.into_super().into_super().into_super())).collect();
            Some(NSArray::from_retained_slice(&elements))
        })() }
        #[unsafe(method_id(accessibilitySelectedChildren))]
        fn ax_selected_children(&self)->Option<Retained<NSArray>> { (|| {
            let state=self.ivars().state.upgrade()?; if !state.is_attached() {return None;} let node=state.tree.borrow().query_node(self.ivars().id)?.clone();
            let ids:Vec<_>=node.children.iter().copied().filter(|id|state.tree.borrow().nodes.get(id).is_some_and(|n|n.selected)).collect();
            let elements:Vec<Retained<AnyObject>>=ids.into_iter().filter_map(|id|state.element(id).map(|e|e.into_super().into_super().into_super())).collect();
            Some(NSArray::from_retained_slice(&elements))
        })() }
        #[unsafe(method_id(accessibilityFocusedUIElement))]
        fn ax_focused_element(&self)->Option<Retained<AnyObject>> { (|| {
            let state=self.ivars().state.upgrade()?; self.node()?; let id=state.tree.borrow().focused?;
            state.element(id).map(|e|e.into_super().into_super().into_super())
        })() }
        #[unsafe(method_id(accessibilityHitTest:))]
        fn ax_hit_test(&self,point:NSPoint)->Option<Retained<AnyObject>> { (|| {
            let state=self.ivars().state.upgrade()?; self.node()?; if !state.is_attached() {return None;}
            if let Some(frame)=state.prepared.borrow().as_ref() {
                let frames=state.frames.borrow();
                let id=frame.geometry.ordered_ids().find(|id|frames.get(id).is_some_and(|r|point.x>=r.origin.x && point.x<r.origin.x+r.size.width && point.y>=r.origin.y && point.y<r.origin.y+r.size.height))?;
                drop(frames); return state.element(id).map(|e|e.into_super().into_super().into_super());
            }
            let tree=state.tree.borrow();
            fn visit(state:&MacState,tree:&SemanticTree,id:NodeId,p:NSPoint)->Option<NodeId> {
                let node=tree.nodes.get(&id)?;
                for child in node.children.iter().rev() {if let Some(found)=visit(state,tree,*child,p) {return Some(found);}}
                state.frames.borrow().get(&id).filter(|r|p.x>=r.origin.x && p.x<r.origin.x+r.size.width && p.y>=r.origin.y && p.y<r.origin.y+r.size.height).map(|_|id)
            }
            let id=visit(&state,&tree,tree.active_modal.unwrap_or(tree.root),point)?;
            drop(tree); state.element(id).map(|e|e.into_super().into_super().into_super())
        })() }
        #[unsafe(method(accessibilityPerformPress))]
        fn ax_press(&self)->bool {self.node().is_some_and(|n| {
            let action=if n.capabilities.contains(&Capability::Activate){Action::Activate}else if n.capabilities.contains(&Capability::Pause){Action::Pause}else if n.capabilities.contains(&Capability::Cancel){Action::Cancel}else if n.capabilities.contains(&Capability::Dismiss){Action::Dismiss}else{return false};
            self.dispatch(action)
        })}
        #[unsafe(method(accessibilityPerformCancel))]
        fn ax_cancel(&self)->bool {
            if self.node().is_some_and(|n|n.capabilities.contains(&Capability::Dismiss)) {self.dispatch(Action::Dismiss)}else{self.dispatch(Action::Cancel)}
        }
        #[unsafe(method(accessibilityNumberOfCharacters))]
        fn ax_text_count(&self)->isize {self.node().and_then(|n|n.value).map(|v|v.encode_utf16().count().min(isize::MAX as usize) as isize).unwrap_or(0)}
        #[unsafe(method(accessibilitySelectedTextRange))]
        fn ax_selection(&self)->NSRange {self.node().and_then(|n|n.text_selection).map(|r|NSRange::new(r.start,r.end-r.start)).unwrap_or(NSRange::new(0,0))}
        #[unsafe(method(setAccessibilitySelectedTextRange:))]
        fn ax_set_selection(&self,r:NSRange) {if let Some(end)=r.location.checked_add(r.length) {self.dispatch(Action::SetSelection(r.location..end));}}
        #[unsafe(method_id(accessibilitySelectedText))]
        fn ax_selected_text(&self)->Option<Retained<NSString>> { (|| {
            let n=self.node()?; let r=n.text_selection?; let text=n.value?;
            let units:Vec<u16>=text.encode_utf16().collect();
            units.get(r).and_then(|slice|String::from_utf16(slice).ok()).map(|s|NSString::from_str(&s))
        })() }
        #[unsafe(method_id(accessibilityStringForRange:))]
        fn ax_text_range(&self,r:NSRange)->Option<Retained<NSString>> { (|| {
            let text=self.node()?.value?; let end=r.location.checked_add(r.length)?;
            if !super::model::valid_text_range(&text,&(r.location..end)){return None;}
            let units:Vec<u16>=text.encode_utf16().collect();
            String::from_utf16(units.get(r.location..end)?).ok().map(|s|NSString::from_str(&s))
        })() }
        #[unsafe(method(accessibilityLineForIndex:))]
        fn ax_line_for_index(&self,index:isize)->isize {
            self.node().and_then(|n|n.value).and_then(|text| {
                let index=usize::try_from(index).ok()?; if !super::model::valid_text_range(&text,&(index..index)){return None;}
                Some(text.encode_utf16().take(index).filter(|c|*c==10).count() as isize)
            }).unwrap_or(-1)
        }
        #[unsafe(method(accessibilityRangeForLine:))]
        fn ax_range_for_line(&self,line:isize)->NSRange {
            self.node().and_then(|n|n.value).and_then(|text| {
                let line=usize::try_from(line).ok()?;let mut offset=0;
                for (index,value) in text.split_inclusive('\n').enumerate() {let length=value.encode_utf16().count();if index==line{return Some(NSRange::new(offset,length));}offset+=length;}
                if text.is_empty() && line==0 || text.ends_with('\n') && line==text.chars().filter(|c|*c=='\n').count(){Some(NSRange::new(offset,0))}else{None}
            }).unwrap_or(NSRange::new(usize::MAX,0))
        }
        #[unsafe(method(isAccessibilitySelectorAllowed:))]
        fn ax_selector_allowed(&self,selector:objc2::runtime::Sel)->bool {
            let Some(n)=self.node() else{return false.into()};
            let name=selector.name().to_string_lossy();
            match name.as_ref() {
                "accessibilityPerformPress"=>n.capabilities.iter().any(|c|matches!(c,Capability::Activate|Capability::Pause|Capability::Cancel|Capability::Dismiss)),
                "setAccessibilityValue:"=>n.capabilities.contains(&Capability::Value),
                "setAccessibilitySelected:"=>n.capabilities.contains(&Capability::Selection),
                "setAccessibilityFocused:"=>n.capabilities.contains(&Capability::Focus),
                "setAccessibilitySelectedTextRange:"=>n.capabilities.contains(&Capability::TextSelection),
                "accessibilityFrameForRange:"|"accessibilityRangeForPosition:"=>false,
                _=>true,
            }
        }
    }
);
impl AxElement {
    fn new(id: NodeId, state: Weak<MacState>, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ElementIvars { id, state });
        // SAFETY: inherited NSObject initializer returns this newly allocated subclass.
        unsafe { msg_send![super(this), init] }
    }
    fn node(&self) -> Option<Node> {
        let state = self.ivars().state.upgrade()?;
        if !state.is_attached() {
            return None;
        };

        state
            .tree
            .borrow()
            .query_node(self.ivars().id)
            .map(Node::clone_metadata)
    }
    fn dispatch(&self, action: Action) -> bool {
        let Some(state) = self.ivars().state.upgrade() else {
            return false;
        };
        if !state.is_attached() {
            return false;
        }
        let stamp = state.tree.borrow().stamp;
        let intent = AccessibilityIntent {
            node: self.ivars().id,
            stamp,
            action,
        };
        let key = state.prepared.borrow().as_ref().map(|f| f.key.request);
        match key {
            Some(key) => state.sink.try_dispatch_prepared(intent, key),
            None => state.sink.try_dispatch(intent),
        }
        .is_ok()
    }
}
impl MacState {
    fn is_attached(&self) -> bool {
        self.attached.get() && !self.sink.is_closing()
    }
    fn element(self: &Rc<Self>, id: NodeId) -> Option<Retained<AxElement>> {
        if !self.is_attached() || self.tree.borrow().query_node(id).is_none() {
            return None;
        }
        if let Some(e) = self.elements.borrow().get(&id) {
            return Some(e.clone());
        }
        self.materialized.record(id).ok()?;
        let mtm = MainThreadMarker::new()?;
        let e = AxElement::new(id, Rc::downgrade(self), mtm);
        self.elements.borrow_mut().insert(id, e.clone());
        Some(e)
    }
}
fn role(r: Role) -> &'static NSString {
    // SAFETY: AppKit exported role constants are immutable process-lifetime NSString objects.
    unsafe {
        match r {
            Role::Window | Role::Group => NSAccessibilityGroupRole,
            Role::List => NSAccessibilityListRole,
            Role::Row => NSAccessibilityRowRole,
            Role::Button => NSAccessibilityButtonRole,
            Role::Status => NSAccessibilityStaticTextRole,
            Role::Dialog => NSAccessibilityGroupRole,
            Role::TextField => NSAccessibilityTextFieldRole,
            Role::TextArea => NSAccessibilityTextAreaRole,
        }
    }
}
fn native_rect(view: &NSView, r: Rect) -> Result<NSRect, BridgeError> {
    let window = view.window().ok_or(BridgeError::NoNativeWindow)?;
    let y = if view.isFlipped() {
        r.y
    } else {
        view.bounds().size.height - r.y - r.height
    };
    let local = NSRect::new(NSPoint::new(r.x, y), NSSize::new(r.width, r.height));
    Ok(window.convertRectToScreen(view.convertRect_toView(local, None)))
}
/// !Send through its retained MainThreadOnly NSView and Rc state. Drop restores only our attachment.
pub struct NativeBridge {
    state: Rc<MacState>,
    root: Retained<AxElement>,
    previous_children: Option<Retained<NSArray>>,
    previous_element: bool,
    detached: bool,
}
impl NativeBridge {
    pub fn attach(window: &impl HasWindowHandle, sink: ActionSink) -> Result<Self, BridgeError> {
        Self::attach_impl(window, sink, true)
    }
    /// Attach the actual immutable sink tree without a foreground logical-node scan.
    /// Geometry starts empty; install the first ACK-based prepared frame separately.
    pub fn attach_unpublished(
        window: &impl HasWindowHandle,
        sink: ActionSink,
    ) -> Result<Self, BridgeError> {
        Self::attach_impl(window, sink, false)
    }
    fn attach_impl(
        window: &impl HasWindowHandle,
        sink: ActionSink,
        publish_initial: bool,
    ) -> Result<Self, BridgeError> {
        let mtm = MainThreadMarker::new().ok_or(BridgeError::WrongThread)?;
        let tree = sink.attachment_tree().map_err(BridgeError::Dispatch)?;
        let handle =
            HasWindowHandle::window_handle(window).map_err(|_| BridgeError::WrongHandle)?;
        let RawWindowHandle::AppKit(raw) = handle.as_raw() else {
            return Err(BridgeError::WrongHandle);
        };
        // SAFETY: the borrowed raw handle is live for window; retaining extends NSView ownership.
        // Unlike from_raw, retain does not claim ownership of GPUI's existing +1 reference.
        let view = unsafe { Retained::<NSView>::retain(raw.ns_view.as_ptr().cast()) }
            .ok_or(BridgeError::InvalidView)?;
        if view.window().is_none() {
            return Err(BridgeError::NoNativeWindow);
        }
        let previous_children = view.accessibilityChildren();
        if previous_children.as_ref().is_some_and(|a| {
            (0..a.len()).any(|i| a.objectAtIndex(i).downcast_ref::<AxElement>().is_some())
        }) {
            return Err(BridgeError::AlreadyAttached);
        }
        let previous_element = view.isAccessibilityElement();
        let state = Rc::new(MacState {
            tree: RefCell::new(tree.clone()),
            prepared: RefCell::new(None),
            materialized: Arc::new(MaterializedNodes::default()),
            cleanup_cursor: Cell::new(None),
            frames: RefCell::new(BTreeMap::new()),
            elements: RefCell::new(BTreeMap::new()),
            view,
            sink,
            attached: Cell::new(true),
        });
        let root = AxElement::new(tree.root, Rc::downgrade(&state), mtm);
        state.elements.borrow_mut().insert(tree.root, root.clone());
        state
            .materialized
            .record(tree.root)
            .map_err(BridgeError::Dispatch)?;
        let children = NSArray::from_slice(&[&*root as &AnyObject]);
        // SAFETY: the array contains valid NSAccessibility-conforming objects retained by state.
        unsafe {
            state.view.setAccessibilityChildren(Some(&children));
        }
        state.view.setAccessibilityElement(false);
        let mut bridge = Self {
            state,
            root,
            previous_children,
            previous_element,
            detached: false,
        };
        if publish_initial {
            bridge.publish(tree)?;
        }
        // A close racing native binding invalidates queries immediately and rolls back attachment.
        if bridge.state.sink.is_closing() {
            return Err(BridgeError::Dispatch(Rejection::Closing));
        }
        Ok(bridge)
    }
    pub fn publish(&mut self, tree: Arc<SemanticTree>) -> Result<(), BridgeError> {
        if MainThreadMarker::new().is_none() {
            return Err(BridgeError::WrongThread);
        }
        if self.detached {
            return Err(BridgeError::Dispatch(Rejection::Closing));
        }
        let mut frames = BTreeMap::new();
        for n in tree.nodes.values() {
            if let Some(g) = n.geometry {
                frames.insert(n.id, native_rect(&self.state.view, g.visible)?);
            }
        }
        self.state
            .sink
            .publish(tree.clone())
            .map_err(BridgeError::Dispatch)?;
        let old = self.state.tree.replace(tree.clone());
        self.state.frames.replace(frames);
        self.state
            .elements
            .borrow_mut()
            .retain(|id, _| tree.nodes.contains_key(id));
        // SAFETY: notification recipient is our live, retained NSAccessibility object on main thread.
        unsafe {
            if old.focused != tree.focused
                && let Some(id) = tree.focused
                && let Some(e) = self.state.element(id)
            {
                NSAccessibilityPostNotification(
                    &e,
                    NSAccessibilityFocusedUIElementChangedNotification,
                );
            }
            if old.layout_revision != tree.layout_revision || old.nodes.len() != tree.nodes.len() {
                NSAccessibilityPostNotification(
                    &self.root,
                    NSAccessibilityLayoutChangedNotification,
                );
            }
            for (id, n) in &tree.nodes {
                if let Some(before) = old.nodes.get(id) {
                    if before.selected != n.selected
                        && let Some(e) = self.state.element(n.parent.unwrap_or(tree.root))
                    {
                        NSAccessibilityPostNotification(
                            &e,
                            NSAccessibilitySelectedChildrenChangedNotification,
                        );
                    }
                    if before.value != n.value {
                        let e = self.state.elements.borrow().get(id).cloned();
                        if let Some(e) = e {
                            NSAccessibilityPostNotification(
                                &e,
                                NSAccessibilityValueChangedNotification,
                            );
                        }
                    }
                    if before.text_selection != n.text_selection {
                        let e = self.state.elements.borrow().get(id).cloned();
                        if let Some(e) = e {
                            NSAccessibilityPostNotification(
                                &e,
                                NSAccessibilitySelectedTextChangedNotification,
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }
    pub fn invalidate_geometry(&mut self) -> Result<(), BridgeError> {
        if MainThreadMarker::new().is_none() {
            return Err(BridgeError::WrongThread);
        }
        self.state.frames.borrow_mut().clear();
        Ok(())
    }
    pub fn materialized_nodes(&self) -> Arc<MaterializedNodes> {
        self.state.materialized.clone()
    }
    /// Prepared production path: no logical-tree enumeration or native-cache retain scan.
    pub fn publish_prepared(
        &mut self,
        frame: Arc<PreparedFrame>,
        expected: FrameKey,
    ) -> Result<PublishedOutcome<BridgeError>, BridgeError> {
        if MainThreadMarker::new().is_none() {
            return Err(BridgeError::WrongThread);
        }
        if self.detached {
            return Err(BridgeError::Dispatch(Rejection::Closing));
        }
        if frame.key != expected || !frame.notifications.materialized_only {
            return Err(BridgeError::Dispatch(Rejection::Stale));
        }
        let mut frames = BTreeMap::new();
        for (id, geometry) in &frame.geometry.nodes {
            frames.insert(*id, native_rect(&self.state.view, geometry.visible)?);
        }
        let converted_frames = frames.len();
        let retired = self
            .state
            .materialized
            .with_revision(frame.notifications.materialization_revision, || {
                self.state.sink.install_prepared(&frame, expected, || {
                    self.state.tree.replace(frame.semantic.tree.clone());
                    self.state.prepared.replace(Some(frame.clone()));
                    self.state.frames.replace(frames);
                })
            })
            .map_err(BridgeError::Dispatch)?;
        // Native objects are lazily unavailable immediately, then reclaimed in bounded batches.
        let cursor = self.state.cleanup_cursor.get();
        let ids: Vec<_> = self
            .state
            .elements
            .borrow()
            .range((
                cursor.map_or(std::ops::Bound::Unbounded, std::ops::Bound::Excluded),
                std::ops::Bound::Unbounded,
            ))
            .take(64)
            .map(|(id, _)| *id)
            .collect();
        let cleanup_visits = ids.len();
        self.state.cleanup_cursor.set(if ids.len() == 64 {
            ids.last().copied()
        } else {
            None
        });
        for id in ids {
            if !frame.semantic.tree.nodes.contains_key(&id) {
                self.state.elements.borrow_mut().remove(&id);
            }
        }
        let plan = &frame.notifications;
        let mut notification_visits = 0;
        // SAFETY: every recipient is a live main-thread NSAccessibility object; state is installed.
        unsafe {
            if let Some(id) = plan.focused {
                notification_visits += 1;
                if let Some(e) = self.state.element(id) {
                    NSAccessibilityPostNotification(
                        &e,
                        NSAccessibilityFocusedUIElementChangedNotification,
                    );
                }
            }
            if plan.layout_changed || plan.structure_changed {
                notification_visits += 1;
                NSAccessibilityPostNotification(
                    &self.root,
                    NSAccessibilityLayoutChangedNotification,
                );
            }
            for id in &plan.selected_parents {
                notification_visits += 1;
                if let Some(e) = self.state.element(*id) {
                    NSAccessibilityPostNotification(
                        &e,
                        NSAccessibilitySelectedChildrenChangedNotification,
                    );
                }
            }
            for id in plan.values.keys() {
                notification_visits += 1;
                if let Some(e) = self.state.elements.borrow().get(id).cloned() {
                    NSAccessibilityPostNotification(&e, NSAccessibilityValueChangedNotification);
                }
            }
            for id in &plan.text_selections {
                notification_visits += 1;
                if let Some(e) = self.state.elements.borrow().get(id).cloned() {
                    NSAccessibilityPostNotification(
                        &e,
                        NSAccessibilitySelectedTextChangedNotification,
                    );
                }
            }
        }
        Ok(PublishedOutcome {
            publication_seq: frame.publication_seq,
            retired,
            notification_error: None,
            converted_frames,
            notification_visits,
            cleanup_visits,
        })
    }
    fn detach_attachment(&mut self) {
        if self.detached {
            return;
        }
        self.detached = true;
        self.state.attached.set(false);
        self.state.sink.close();
        let children = self.state.view.accessibilityChildren();
        let owned = children.as_ref().is_some_and(|a| {
            a.len() == 1
                && a.firstObject()
                    .is_some_and(|object| std::ptr::eq(&*object, &*self.root as &AnyObject))
        });
        if owned {
            // SAFETY: this restores the exact array borrowed before attaching; no foreign attachment is overwritten.
            unsafe {
                self.state
                    .view
                    .setAccessibilityChildren(self.previous_children.as_deref());
            }
            self.state
                .view
                .setAccessibilityElement(self.previous_element);
        }
        self.state.frames.borrow_mut().clear();
    }
    /// Keep this bridge on the native thread until native_retirement_complete().
    /// Keep materialized_nodes() on the worker until after the bridge is dropped.
    pub fn detach_prepared(&mut self) -> Result<RetiredPublication, BridgeError> {
        if MainThreadMarker::new().is_none() {
            return Err(BridgeError::WrongThread);
        }
        self.detach_attachment();
        let retired = self
            .state
            .sink
            .close_and_retire()
            .map_err(BridgeError::Dispatch)?;
        let empty = self.state.sink.current().map_err(BridgeError::Dispatch)?;
        self.state.tree.replace(empty);
        self.state.prepared.take();
        Ok(retired)
    }
    pub fn drain_native_retirement(&mut self, budget: usize) -> Result<usize, BridgeError> {
        if MainThreadMarker::new().is_none() {
            return Err(BridgeError::WrongThread);
        }
        if !self.detached {
            return Err(BridgeError::Dispatch(Rejection::Closing));
        }
        let mut elements = self.state.elements.borrow_mut();
        let mut visits = 0;
        while visits < budget && elements.pop_first().is_some() {
            visits += 1;
        }
        Ok(visits)
    }
    pub fn native_retirement_complete(&self) -> bool {
        self.detached && self.state.elements.borrow().is_empty()
    }
    pub fn detach(&mut self) {
        // Compatibility path may reclaim all client-enumerated objects; production
        // uses detach_prepared plus bounded native retirement on subsequent UI turns.
        if self.detached {
            return;
        }
        self.detach_attachment();
        self.state.elements.borrow_mut().clear();
    }
}
impl Drop for NativeBridge {
    fn drop(&mut self) {
        self.detach();
    }
}
