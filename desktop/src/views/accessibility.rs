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

pub struct Host {
    model: AccessibilityModel,
    pub tree: Option<Arc<SemanticTree>>,
    pub frame: Rc<RefCell<LayoutSnapshot>>,
    sink: Option<ActionSink>,
    pub receiver: Option<ActionReceiver>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    bridge: Option<crate::platform::accessibility::NativeBridge>,
    frame_revision: u64,
    native: bool,
}
impl Default for Host {
    fn default() -> Self {
        Self {
            model: AccessibilityModel::default(),
            tree: None,
            frame: Rc::new(RefCell::new(LayoutSnapshot::default())),
            sink: None,
            receiver: None,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            bridge: None,
            frame_revision: 0,
            native: true,
        }
    }
}
impl Host {
    /// Test-platform frames never attach an AppKit/Win32 provider.
    pub fn headless() -> Self {
        let mut host = Self::default();
        host.native = false;
        host
    }
    pub fn begin(&mut self, s: &Snapshot, text: Option<&NativeTextSnapshot>) {
        self.frame_revision = self.frame_revision.wrapping_add(1);
        self.frame = Rc::new(RefCell::new(LayoutSnapshot {
            window_generation: s.window_generation,
            semantic_revision: s.revision,
            revision: self.frame_revision,
            nodes: Default::default(),
        }));
        self.tree = Some(Arc::new(self.model.project(s, text, &self.frame.borrow())));
    }
    pub fn id(
        &self,
        target: &Target,
        role: Option<Role>,
        capability: Option<Capability>,
    ) -> Option<NodeId> {
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
        element
            .relative()
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
        if self.frame.borrow().semantic_revision != s.revision
            || self.frame.borrow().window_generation != s.window_generation
        {
            return Ok(());
        }
        let tree = Arc::new(self.model.project(s, text, &self.frame.borrow()));
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
        self.sink.as_ref().unwrap()
    }
    pub fn close(&mut self) {
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
        (Target::Place { kind, path }, Action::Activate) => Command::Place {
            kind: match kind {
                crate::platform::accessibility::model::PlaceKind::Drive => PlaceKind::Drive,
                crate::platform::accessibility::model::PlaceKind::Common => PlaceKind::Common,
                crate::platform::accessibility::model::PlaceKind::Bookmark => PlaceKind::Bookmark,
            },
            path: path.to_string_lossy().into_owned(),
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
