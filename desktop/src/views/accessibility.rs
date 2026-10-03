//! Native accessibility crossing: actual frame geometry and the existing actor gateway.
use crate::platform::accessibility::{
    ActionReceiver, ActionSink, ResolvedAction,
    model::{
        AccessibilityModel, Action, Capability, LayoutSnapshot, NativeTextSnapshot, NodeId, Rect,
        Role, SemanticTree, Target,
    },
};
use crate::{
    components::text_input::TextInput,
    runtime::{Command, JobVerb, PlaceKind, TargetVerb},
};
use gpui::{App, Bounds, Entity, Pixels, Window, canvas, prelude::*};
use ira_core::{input::KeyCode, model::EntryTarget, observable::Snapshot};
use std::{cell::RefCell, rc::Rc, sync::Arc};

/// The native caret/marked tuple is independent of core dirty state and text revision.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NativeFocus {
    target: Option<Target>,
    text: Option<NativeTextFocus>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct NativeTextFocus {
    document: u64,
    focus: u64,
    selection: std::ops::Range<usize>,
    marked: Option<std::ops::Range<usize>>,
    read_only: bool,
    disabled: bool,
    multiline: bool,
}
#[derive(Default)]
struct Authority {
    focus: Option<NativeFocus>,
    focus_revision: u64,
    presentation_revision: u64,
    footer: Option<Arc<str>>,
    key: Option<crate::platform::accessibility::model::RequestKey>,
}
impl Authority {
    fn observe(
        &mut self,
        s: &Snapshot,
        text: Option<&NativeTextSnapshot>,
        target: Option<Target>,
        footer: Arc<str>,
    ) -> crate::platform::accessibility::model::RequestKey {
        let focus = NativeFocus {
            target,
            text: text.map(|t| NativeTextFocus {
                document: t.document_generation,
                focus: t.focus_generation,
                selection: t.selection_utf16.clone(),
                marked: t.marked_utf16.clone(),
                read_only: t.read_only,
                disabled: t.disabled,
                multiline: t.multiline,
            }),
        };
        if self.focus.as_ref() != Some(&focus) {
            self.focus_revision = self.focus_revision.wrapping_add(1);
            self.focus = Some(focus);
        }
        if self.footer.as_deref() != Some(footer.as_ref()) {
            self.presentation_revision = self.presentation_revision.wrapping_add(1);
            self.footer = Some(footer);
        }
        let key = crate::platform::accessibility::model::RequestKey {
            window_generation: s.window_generation,
            semantic_revision: s.revision,
            document_generation: s.document_generation,
            focus_generation: s.focus_generation,
            native_text_revision: text.map_or(0, |t| t.revision),
            host_focus_revision: self.focus_revision,
            host_presentation_revision: self.presentation_revision,
        };
        self.key = Some(key);
        key
    }
}
pub struct Host {
    model: AccessibilityModel,
    prepared: Option<super::accessibility_prepared::PreparedHost>,
    compatibility: bool,
    authority: Authority,
    pub tree: Option<Arc<SemanticTree>>,
    pub frame: Rc<RefCell<LayoutSnapshot>>,
    sink: Option<ActionSink>,
    pub receiver: Option<ActionReceiver>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    bridge: Option<crate::platform::accessibility::NativeBridge>,
    frame_revision: u64,
    native: bool,
    pub focused_target: Option<Target>,
}
impl Default for Host {
    fn default() -> Self {
        Self::configured(false, Default::default())
    }
}
impl Host {
    fn configured(native: bool, retirement: super::accessibility_retirement::Retirement) -> Self {
        Self {
            model: AccessibilityModel::default(),
            prepared: Some(super::accessibility_prepared::PreparedHost::new(
                native, retirement,
            )),
            compatibility: false,
            authority: Authority::default(),
            tree: None,
            frame: Rc::new(RefCell::new(LayoutSnapshot::default())),
            sink: None,
            receiver: None,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            bridge: None,
            frame_revision: 0,
            native,
            focused_target: None,
        }
    }
}
impl Host {
    /// Test-platform frames never attach an AppKit/Win32 provider.
    pub fn headless() -> Self {
        Self::configured(false, Default::default())
    }
    /// Legacy crossing fixtures retain their original synchronous projector coverage.
    #[cfg(test)]
    pub fn compatibility_headless() -> Self {
        let mut host = Self::configured(false, Default::default());
        host.prepared.take();
        host.compatibility = true;
        host
    }
    pub fn with_retirement(retirement: super::accessibility_retirement::Retirement) -> Self {
        Self::configured(true, retirement)
    }
    pub fn poll(&mut self) -> bool {
        let changed = self.prepared.as_mut().is_some_and(|host| host.poll());
        if changed && let Some(host) = &mut self.prepared {
            if let Some(old) = self.tree.take() {
                host.retire_pin(old);
            }
            self.tree = host.tree();
        }
        changed
    }
    pub fn begin_prepared(
        &mut self,
        s: Arc<Snapshot>,
        text: Option<NativeTextSnapshot>,
        footer: Arc<str>,
    ) {
        let key = self.observe(&s, text.as_ref(), footer.clone());
        if self.compatibility {
            self.begin(&s, text.as_ref());
            return;
        }
        let focused = self.focused_target.as_ref().and_then(|target| {
            self.prepared
                .as_ref()?
                .focus_id(target, key.window_generation)
        });
        if let Some(host) = &mut self.prepared {
            host.observe_native(
                s.clone(),
                text,
                key,
                (focused, self.focused_target.clone()),
                footer,
            );
        }
        self.poll();
        self.frame = Rc::new(RefCell::new(LayoutSnapshot {
            window_generation: s.window_generation,
            semantic_revision: s.revision,
            revision: 0,
            nodes: Default::default(),
        }));
    }
    pub fn geometry(&self, id: NodeId) -> Option<crate::platform::accessibility::model::Geometry> {
        self.frame.borrow().nodes.get(&id).copied()
    }
    pub fn is_compatibility(&self) -> bool {
        self.compatibility
    }
    pub fn can_accept_snapshot(&self) -> bool {
        self.prepared
            .as_ref()
            .is_none_or(|h| h.can_accept_snapshot())
    }
    pub fn retire_native_text(&mut self, text: NativeTextSnapshot) {
        if let Some(host) = &mut self.prepared {
            host.retire_pin(text);
        }
    }
    pub fn retire_snapshot(&mut self, snapshot: Arc<Snapshot>) {
        if let Some(host) = &mut self.prepared {
            host.retire_pin(snapshot);
        }
    }
    pub fn observe(
        &mut self,
        s: &Snapshot,
        text: Option<&NativeTextSnapshot>,
        footer: Arc<str>,
    ) -> crate::platform::accessibility::model::RequestKey {
        self.authority
            .observe(s, text, self.focused_target.clone(), footer)
    }
    pub fn authoritative_key(&self) -> Option<crate::platform::accessibility::model::RequestKey> {
        self.authority.key
    }
    pub fn begin(&mut self, s: &Snapshot, text: Option<&NativeTextSnapshot>) {
        self.frame_revision = self.frame_revision.wrapping_add(1);
        self.frame = Rc::new(RefCell::new(LayoutSnapshot {
            window_generation: s.window_generation,
            semantic_revision: s.revision,
            revision: self.frame_revision,
            nodes: Default::default(),
        }));
        self.tree = Some(Arc::new(
            self.model.project_with_host_focus(
                s,
                text,
                &self.frame.borrow(),
                self.focused_target
                    .as_ref()
                    .and_then(|target| self.id(target, None, None)),
            ),
        ));
    }
    pub fn id(
        &self,
        target: &Target,
        role: Option<Role>,
        capability: Option<Capability>,
    ) -> Option<NodeId> {
        if !self.compatibility {
            return self.prepared.as_ref()?.id(target, role, capability);
        }
        self.tree
            .as_ref()?
            .nodes
            .values()
            .find(|n| {
                &n.target == target
                    && role.is_none_or(|r| r == n.role)
                    && capability.is_none_or(|c| n.capabilities.contains(&c))
            })
            .map(|n| n.id)
    }
    pub fn measured(
        &self,
        element: impl IntoElement + ParentElement + Styled + 'static,
        target: Target,
        role: Option<Role>,
        capability: Option<Capability>,
    ) -> gpui::AnyElement {
        let Some(id) = self.id(&target, role, capability) else {
            return element.into_any_element();
        };
        let frame = self.frame.clone();
        // Preserve caller positioning: common modals are absolute overlays.
        element
            .child(
                canvas(
                    move |bounds, window, _| {
                        frame.borrow_mut().record(
                            id,
                            rect(bounds),
                            rect(window.content_mask().bounds),
                        );
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .into_any_element()
    }
    pub fn publish(
        &mut self,
        s: &Snapshot,
        text: Option<&NativeTextSnapshot>,
        window: &mut Window,
    ) -> Result<(), String> {
        if !self.compatibility {
            self.poll();
            if let Some(host) = &mut self.prepared {
                host.paint(self.frame.borrow().clone(), window)?;
                if self.receiver.is_none() {
                    self.receiver = host.take_receiver();
                }
            }
            return Ok(());
        }
        if self.frame.borrow().semantic_revision != s.revision
            || self.frame.borrow().window_generation != s.window_generation
        {
            return Ok(());
        }
        let tree = Arc::new(
            self.model.project_with_host_focus(
                s,
                text,
                &self.frame.borrow(),
                self.focused_target
                    .as_ref()
                    .and_then(|target| self.id(target, None, None)),
            ),
        );
        if self.sink.is_none() {
            let (sink, receiver) = ActionSink::channel(tree.clone(), 128);
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            {
                if self.native {
                    self.bridge = Some(
                        crate::platform::accessibility::NativeBridge::attach(window, sink.clone())
                            .map_err(|e| format!("Accessibility attachment: {e:?}"))?,
                    );
                }
            }
            self.sink = Some(sink);
            self.receiver = Some(receiver);
        }
        self.sink
            .as_ref()
            .unwrap()
            .publish(tree.clone())
            .map_err(|e| format!("Accessibility publication: {e:?}"))?;
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(bridge) = &mut self.bridge {
            bridge
                .publish(tree.clone())
                .map_err(|e| format!("Accessibility native publication: {e:?}"))?;
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = window;
        self.tree = Some(tree);
        Ok(())
    }
    #[cfg(test)]
    pub fn sink_for_test(&self) -> &ActionSink {
        if let Some(host) = &self.prepared {
            return host.sink_for_test();
        }
        self.sink.as_ref().unwrap()
    }
    #[cfg(test)]
    pub fn retire_probe<T: Send + 'static>(&mut self, value: T) {
        self.prepared.as_mut().unwrap().retire_pin(value);
    }
    #[cfg(test)]
    pub fn prepared_frame_for_test(
        &self,
    ) -> Option<&Arc<crate::platform::accessibility::model::PreparedFrame>> {
        self.prepared.as_ref()?.installed_for_test()
    }
    pub fn close(&mut self) {
        if let Some(mut host) = self.prepared.take() {
            if let Some(tree) = self.tree.take() {
                host.retire_pin(tree);
            }
            if let Some(receiver) = self.receiver.take() {
                host.retire_pin(receiver);
            }
            host.close();
        }
        if let Some(sink) = &self.sink {
            sink.close();
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(mut bridge) = self.bridge.take() {
            #[cfg(target_os = "macos")]
            bridge.detach();
            #[cfg(target_os = "windows")]
            let _ = bridge.detach();
        }
        self.receiver = None;
        self.sink = None;
        self.tree = None;
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.close();
    }
}
pub fn rect(b: Bounds<Pixels>) -> Rect {
    Rect {
        x: f32::from(b.origin.x) as f64,
        y: f32::from(b.origin.y) as f64,
        width: f32::from(b.size.width) as f64,
        height: f32::from(b.size.height) as f64,
    }
}
pub fn native_text(
    input: &Entity<TextInput>,
    mode: super::InputMode,
    read_only: bool,
    cx: &App,
) -> NativeTextSnapshot {
    let input = input.read(cx);
    let stamp = input.stamp();
    let b = input.snapshot();
    let utf16 = |range: std::ops::Range<usize>| {
        b.text[..range.start].encode_utf16().count()..b.text[..range.end].encode_utf16().count()
    };
    NativeTextSnapshot {
        document_generation: stamp.document_generation,
        focus_generation: stamp.focus_generation,
        revision: stamp.value_revision,
        text: b.text.clone().into(),
        selection_utf16: utf16(b.selection),
        marked_utf16: b.marked.map(utf16),
        read_only,
        disabled: false,
        multiline: mode == super::InputMode::Editor,
    }
}
pub fn actor_command(action: ResolvedAction) -> Result<Command, String> {
    let key = |code| Command::Input(crate::actions::input(code));
    Ok(match (action.target, action.action) {
        (Target::Pane(pane), Action::Focus) => Command::FocusPane(pane),
        (
            Target::Entry {
                pane,
                path,
                listing_generation,
            },
            action,
        ) => Command::Target {
            target: EntryTarget {
                pane,
                path,
                listing_generation,
            },
            verb: match action {
                Action::Focus => TargetVerb::Focus,
                Action::Activate => TargetVerb::Open,
                Action::SetSelected(value) => TargetVerb::Selected(value),
                Action::SelectOnly => TargetVerb::SelectOnly,
                _ => return Err("Unsupported entry accessibility action".into()),
            },
        },
        (
            Target::Place {
                kind,
                path,
                shortcut,
                occurrence,
            },
            Action::Activate,
        ) => Command::PlaceExact {
            kind: match kind {
                crate::platform::accessibility::model::PlaceKind::Drive => PlaceKind::Drive,
                crate::platform::accessibility::model::PlaceKind::Common => PlaceKind::Common,
                crate::platform::accessibility::model::PlaceKind::Bookmark => PlaceKind::Bookmark,
            },
            path: path.to_string_lossy().into_owned(),
            shortcut,
            occurrence,
        },
        (Target::Job(id), action) => Command::Job {
            id,
            verb: match action {
                Action::Focus => JobVerb::Focus,
                Action::Pause => JobVerb::Pause,
                Action::Cancel => JobVerb::Cancel,
                _ => return Err("Unsupported job accessibility action".into()),
            },
        },
        (Target::Modal, Action::Activate) => key(KeyCode::Enter),
        (Target::Modal, Action::Dismiss) => key(KeyCode::Esc),
        _ => return Err("Accessibility action requires the native text or layout adapter".into()),
    })
}

#[cfg(test)]
mod authority_tests {
    use super::*;
    #[test]
    fn caret_marked_and_focus_changes_preserve_core_and_text_revisions() {
        let snapshot = ira_core::application::App::default().snapshot();
        let mut authority = Authority::default();
        let mut text = NativeTextSnapshot {
            document_generation: snapshot.document_generation,
            focus_generation: snapshot.focus_generation,
            revision: 9,
            text: "文😀e\u{301}".into(),
            selection_utf16: 1..1,
            marked_utf16: None,
            read_only: false,
            disabled: false,
            multiline: true,
        };
        let first = authority.observe(&snapshot, Some(&text), None, "footer".into());
        text.selection_utf16 = 3..3; // Whole non-BMP scalar boundary.
        let caret = authority.observe(&snapshot, Some(&text), None, "footer".into());
        text.marked_utf16 = Some(3..5); // Composition unchanged text bytes.
        let marked = authority.observe(&snapshot, Some(&text), None, "footer".into());
        let focused = authority.observe(
            &snapshot,
            Some(&text),
            Some(Target::Text {
                document: snapshot.document_generation,
            }),
            "footer".into(),
        );
        assert_ne!(first.host_focus_revision, caret.host_focus_revision);
        assert_ne!(caret.host_focus_revision, marked.host_focus_revision);
        assert_ne!(marked.host_focus_revision, focused.host_focus_revision);
        for key in [caret, marked, focused] {
            assert_eq!(key.semantic_revision, first.semantic_revision);
            assert_eq!(key.native_text_revision, 9);
            assert_eq!(
                key.host_presentation_revision,
                first.host_presentation_revision
            );
        }
        assert_eq!(
            authority.observe(
                &snapshot,
                Some(&text),
                Some(Target::Text {
                    document: snapshot.document_generation
                }),
                "footer".into()
            ),
            focused
        );
        let footer = authority.observe(
            &snapshot,
            Some(&text),
            Some(Target::Text {
                document: snapshot.document_generation,
            }),
            "feedback".into(),
        );
        assert_ne!(
            footer.host_presentation_revision,
            focused.host_presentation_revision
        );
        assert_eq!(footer.host_focus_revision, focused.host_focus_revision);
    }
}

#[cfg(test)]
mod reviewer_async_focus_probe {
    use super::*;
    use ira_core::{
        application::App as CoreApp, domain::data::Folder, services::list_files::FEntry,
    };
    #[test]
    fn actual_host_retains_observed_place_focus_across_pending_semantics() {
        let mut app = CoreApp::default();
        app.window_generation = 7;
        app.bookmarks = Some(vec![Folder::new(
            "Bookmark".into(),
            "/fixture/bookmark".into(),
            'b',
        )]);
        app.panes[0].files = vec![FEntry {
            path: "/fixture/file".into(),
            label: "file".into(),
            is_dir: false,
            size: 1,
            modified: None,
        }];
        app.panes[0].selected = vec![false];
        app.panes[0].state.select(Some(0));
        let snapshot = Arc::new(app.snapshot());
        let mut host = Host::headless();
        host.begin_prepared(snapshot.clone(), None, "footer".into());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while host.tree.is_none() {
            host.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let target = Target::Place {
            kind: crate::platform::accessibility::model::PlaceKind::Bookmark,
            path: "/fixture/bookmark".into(),
            shortcut: 'b',
            occurrence: 0,
        };
        let expected = host.id(&target, None, None).unwrap();
        let mut next = (*snapshot).clone();
        next.revision += 1;
        let next = Arc::new(next);
        let pending = host.observe(&next, None, "footer".into());
        host.prepared
            .as_mut()
            .unwrap()
            .observe(next.clone(), None, pending, None, "footer".into());
        // A real UI can observe new focus before its previous semantic request completes.
        assert!(host.id(&target, None, None).is_none());
        host.focused_target = Some(target.clone());
        host.begin_prepared(next.clone(), None, "footer".into());
        let key = host.authoritative_key().unwrap();
        loop {
            host.poll();
            if host
                .prepared
                .as_ref()
                .unwrap()
                .semantic
                .as_ref()
                .is_some_and(|s| s.key == key)
            {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(
            host.tree.as_ref().unwrap().focused,
            Some(expected),
            "latest actual bookmark focus must override semantic pane cursor even with an intermediate frame pending"
        );
        host.close();
    }
}

#[cfg(test)]
mod reviewer_novel_focus_probe {
    use super::*;
    use ira_core::{
        application::App as CoreApp, domain::data::Folder, services::list_files::FEntry,
    };
    #[test]
    fn actual_host_resolves_newly_added_place_focus_on_latest_tree() {
        let mut app = CoreApp::default();
        app.window_generation = 7;
        app.bookmarks = Some(vec![Folder::new(
            "Bookmark".into(),
            "/fixture/bookmark".into(),
            'b',
        )]);
        app.panes[0].files = vec![FEntry {
            path: "/fixture/file".into(),
            label: "file".into(),
            is_dir: false,
            size: 1,
            modified: None,
        }];
        app.panes[0].selected = vec![false];
        app.panes[0].state.select(Some(0));
        let snapshot = Arc::new(app.snapshot());
        let mut host = Host::headless();
        host.begin_prepared(snapshot.clone(), None, "footer".into());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while host.tree.is_none() {
            host.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let target = Target::Place {
            kind: crate::platform::accessibility::model::PlaceKind::Bookmark,
            path: "/fixture/bookmark".into(),
            shortcut: 'b',
            occurrence: 0,
        };
        let mut next = (*snapshot).clone();
        next.revision += 1;
        next.bookmarks.as_mut().unwrap().push(Folder::new(
            "New bookmark".into(),
            "/fixture/new-bookmark".into(),
            'c',
        ));
        let next = Arc::new(next);
        let pending = host.observe(&next, None, "footer".into());
        host.prepared
            .as_mut()
            .unwrap()
            .observe(next.clone(), None, pending, None, "footer".into());
        // A real UI can observe new focus before its previous semantic request completes.
        assert!(host.id(&target, None, None).is_none());
        let target = Target::Place {
            kind: crate::platform::accessibility::model::PlaceKind::Bookmark,
            path: "/fixture/new-bookmark".into(),
            shortcut: 'c',
            occurrence: 0,
        };
        host.focused_target = Some(target.clone());
        host.begin_prepared(next.clone(), None, "footer".into());
        let key = host.authoritative_key().unwrap();
        loop {
            host.poll();
            if host
                .prepared
                .as_ref()
                .unwrap()
                .semantic
                .as_ref()
                .is_some_and(|s| s.key == key)
            {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let expected = host.id(&target, None, None).unwrap();
        assert_eq!(
            host.tree.as_ref().unwrap().focused,
            Some(expected),
            "latest actual bookmark focus must override semantic pane cursor even with an intermediate frame pending"
        );
        host.close();
    }
}
