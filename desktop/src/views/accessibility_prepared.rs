//! Foreground adapter: indexed lookup, sparse paint capture and coherent worker ACKs.
use super::accessibility_bridge::NativeBridge;
use super::{
    accessibility_retirement::{Pins, Retirement},
    accessibility_worker::{LayoutRequest, PreparedResult, SemanticRequest, Worker},
};
use crate::platform::accessibility::{
    ActionReceiver, ActionSink,
    model::{
        Capability, FrameKey, HostPresentationSnapshot, LayoutSnapshot, MaterializedNodes,
        NativeTextSnapshot, NodeId, PreparedFrame, PreparedSemantic, RequestKey, Role,
        SemanticTree, Target,
    },
};
use gpui::Window;
use ira_core::observable::Snapshot;
use std::sync::Arc;

pub struct PreparedHost {
    worker: Option<Worker>,
    pub semantic: Option<Arc<PreparedSemantic>>,
    key: Option<RequestKey>,
    sequence: u64,
    submitted_key: Option<RequestKey>,
    pending_request: Option<Box<SemanticRequest>>,
    pending_layout: Option<Box<LayoutRequest>>,
    pending_ack: Option<Arc<PreparedFrame>>,
    pending_seed: Option<(Arc<SemanticTree>, RequestKey)>,
    candidate: Option<Arc<PreparedFrame>>,
    installed: Option<Arc<PreparedFrame>>,
    measured: Option<LayoutSnapshot>,
    measured_key: Option<FrameKey>,
    layout_revision: u64,
    retired: Pins,
    sink: Option<ActionSink>,
    pub receiver: Option<ActionReceiver>,
    bridge: Option<NativeBridge>,
    materialized: Arc<MaterializedNodes>,
    retirement: Retirement,
    native: bool,
    pub index_lookups: std::cell::Cell<usize>,
    pub converted_frames: usize,
}
impl PreparedHost {
    pub fn new(native: bool, retirement: Retirement) -> Self {
        Self {
            worker: Some(Worker::new()),
            semantic: None,
            key: None,
            sequence: 0,
            submitted_key: None,
            pending_request: None,
            pending_layout: None,
            pending_ack: None,
            pending_seed: None,
            candidate: None,
            installed: None,
            measured: None,
            measured_key: None,
            layout_revision: 0,
            retired: Vec::new(),
            sink: None,
            receiver: None,
            bridge: None,
            materialized: Arc::new(MaterializedNodes::default()),
            retirement,
            native: native && cfg!(any(target_os = "macos", target_os = "windows")),
            index_lookups: std::cell::Cell::new(0),
            converted_frames: 0,
        }
    }
    pub fn can_accept_snapshot(&self) -> bool {
        self.retired.len() < 24
    }
    pub fn retire_pin<T: Send + 'static>(&mut self, value: T) {
        self.retire(value);
        self.flush();
    }
    pub fn take_receiver(&mut self) -> Option<ActionReceiver> {
        self.receiver.take()
    }
    #[cfg(test)]
    pub fn installed_for_test(&self) -> Option<&Arc<PreparedFrame>> {
        self.installed.as_ref()
    }
    #[cfg(test)]
    pub fn sink_for_test(&self) -> &ActionSink {
        self.sink.as_ref().unwrap()
    }
    pub fn key(&self) -> Option<RequestKey> {
        self.key
    }
    fn retire<T: Send + 'static>(&mut self, value: T) {
        self.retired.push(Box::new(value));
    }
    fn flush(&mut self) {
        let Some(worker) = &self.worker else { return };
        while let Some(value) = self.retired.pop() {
            if let Err(value) = worker.retire(value) {
                self.retired.push(value);
                break;
            }
        }
        if let Some((tree, key)) = self.pending_seed.take() {
            self.pending_seed = worker.seed_actual_installed(tree, key).err();
        }
        if let Some(ack) = self.pending_ack.take() {
            self.pending_ack = worker.acknowledge(ack).err();
        }
        if let Some(request) = self.pending_request.take() {
            self.pending_request = worker.request(request).err();
        }
        if self.pending_ack.is_none()
            && self.pending_seed.is_none()
            && let Some(layout) = self.pending_layout.take()
        {
            self.pending_layout = worker.layout(layout).err();
        }
    }
    pub fn observe(
        &mut self,
        snapshot: Arc<Snapshot>,
        text: Option<NativeTextSnapshot>,
        key: RequestKey,
        focused: Option<NodeId>,
        footer: Arc<str>,
    ) {
        self.flush();
        if self.key != Some(key) {
            self.key = Some(key);
            if let Some(bridge) = &mut self.bridge {
                let _ = bridge.invalidate_geometry();
            }
        }
        // A busy retirement gate bounds ownership: no new large request/result is taken.
        if self.retired.len() >= 24 || self.worker.is_none() {
            return;
        }
        if self.semantic.as_ref().is_some_and(|s| s.key == key)
            || self.pending_request.as_ref().is_some_and(|r| r.key == key)
        {
            return;
        }
        // A submitted request is tracked separately from its returned payload.
        if self.requested_key() == Some(key) {
            return;
        }
        self.sequence = self.sequence.wrapping_add(1).max(1);
        if let Some(old) = self.pending_request.take() {
            self.retire(old);
        }
        if let Some(old) = self.pending_layout.take() {
            self.retire(old);
        }
        if let Some(old) = self.candidate.take() {
            self.retire(old);
        }
        self.pending_request = Some(Box::new(SemanticRequest {
            sequence: self.sequence,
            key,
            snapshot,
            text,
            focused,
            presentation: HostPresentationSnapshot {
                revision: key.host_presentation_revision,
                footer,
            },
        }));
        self.submitted_key = Some(key);
        self.flush();
    }
    fn requested_key(&self) -> Option<RequestKey> {
        self.submitted_key
    }
    pub fn poll(&mut self) -> bool {
        self.flush();
        if self.retired.len() >= 24 {
            return false;
        }
        let Some(result) = self.worker.as_ref().and_then(Worker::take_result) else {
            return false;
        };
        match result {
            PreparedResult::Semantic {
                sequence,
                value: Ok(semantic),
                ..
            } if sequence == self.sequence && Some(semantic.key) == self.key => {
                if let Some(old) = self.semantic.replace(semantic) {
                    self.retire(old);
                }
                true
            }
            PreparedResult::Frame {
                key,
                value: Ok(frame),
                ..
            } if Some(key) == self.measured_key && Some(key.request) == self.key => {
                if let Some(old) = self.candidate.replace(frame) {
                    self.retire(old);
                }
                true
            }
            other => {
                self.retire(other);
                self.submitted_key = None;
                false
            }
        }
    }
    pub fn id(
        &self,
        target: &Target,
        role: Option<Role>,
        capability: Option<Capability>,
    ) -> Option<NodeId> {
        let semantic = self.semantic.as_ref()?;
        if Some(semantic.key) != self.key {
            return None;
        }
        self.index_lookups.set(self.index_lookups.get() + 1);
        semantic.index.lookup(target, role, capability)
    }
    pub fn tree(&self) -> Option<Arc<SemanticTree>> {
        self.semantic.as_ref().map(|s| s.tree.clone())
    }
    pub fn paint(&mut self, mut layout: LayoutSnapshot, window: &mut Window) -> Result<(), String> {
        self.flush();
        let Some(key) = self.key else { return Ok(()) };
        let Some(semantic) = self.semantic.as_ref().filter(|s| s.key == key).cloned() else {
            return Ok(());
        };
        if layout.window_generation != key.window_generation
            || layout.semantic_revision != key.semantic_revision
        {
            return Ok(());
        }
        if self.sink.is_none() {
            let (sink, receiver) = ActionSink::channel(semantic.tree.clone(), 128);
            if self.native {
                let bridge = NativeBridge::attach_unpublished(window, sink.clone())
                    .map_err(|e| format!("Accessibility attachment: {e:?}"))?;
                self.materialized = bridge.materialized_nodes();
                self.bridge = Some(bridge);
            }
            // This is the real sink/cache tree, never an invented empty baseline.
            self.pending_seed = Some((
                sink.current()
                    .map_err(|e| format!("Accessibility baseline capture: {e:?}"))?,
                key,
            ));
            self.receiver = Some(receiver);
            self.sink = Some(sink);
            self.flush();
        }
        let changed = self.measured_key.is_none_or(|old| old.request != key)
            || self
                .measured
                .as_ref()
                .is_none_or(|old| old.nodes != layout.nodes);
        if changed {
            self.layout_revision = self.layout_revision.wrapping_add(1).max(1);
            layout.revision = self.layout_revision;
            self.measured_key = Some(FrameKey {
                request: key,
                request_seq: self.sequence,
                layout_revision: self.layout_revision,
            });
            self.measured = Some(layout.clone());
            if let Some(bridge) = &mut self.bridge {
                let _ = bridge.invalidate_geometry();
            }
            if let Some(old) = self.pending_layout.take() {
                self.retire(old);
            }
            self.pending_layout = Some(Box::new(LayoutRequest {
                key: self.measured_key.unwrap(),
                semantic,
                layout,
                materialized: self.materialized.clone(),
            }));
            self.flush();
        }
        if self.pending_ack.is_some() {
            return Ok(());
        }
        let Some(frame) = self.candidate.take() else {
            return Ok(());
        };
        if Some(frame.key) != self.measured_key || Some(frame.key.request) != self.key {
            self.retire(frame);
            return Ok(());
        }
        let result = if let Some(bridge) = &mut self.bridge {
            bridge
                .publish_prepared(frame.clone(), frame.key)
                .map(|outcome| {
                    self.converted_frames += outcome.converted_frames;
                    (
                        outcome.retired,
                        outcome
                            .notification_error
                            .map(|e| format!("Accessibility notification: {e:?}")),
                    )
                })
                .map_err(|e| format!("Accessibility publication: {e:?}"))
        } else {
            self.sink
                .as_ref()
                .unwrap()
                .install_prepared(&frame, frame.key, || {})
                .map(|retired| (retired, None))
                .map_err(|e| format!("Accessibility publication: {e:?}"))
        };
        match result {
            Ok((retired, notification_error)) => {
                self.retire(retired);
                if let Some(old) = self.installed.replace(frame.clone()) {
                    self.retire(old);
                }
                self.pending_ack = Some(frame); // Installed-with-error still ACKs.
                self.flush();
                if let Some(error) = notification_error {
                    return Err(error);
                }
            }
            Err(error) => {
                // A membership/precommit rejection needs fresh worker preparation.
                self.retire(frame);
                self.measured = None;
                return Err(error);
            }
        }
        Ok(())
    }
    pub fn close(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        worker.cancel();
        let mut pins = std::mem::take(&mut self.retired);
        pins.push(Box::new(self.semantic.take()));
        pins.push(Box::new(self.installed.take()));
        pins.push(Box::new(self.candidate.take()));
        pins.push(Box::new(self.pending_request.take()));
        pins.push(Box::new(self.pending_layout.take()));
        pins.push(Box::new(self.pending_ack.take()));
        pins.push(Box::new(self.pending_seed.take()));
        pins.push(Box::new(self.receiver.take()));
        pins.push(Box::new(std::mem::replace(
            &mut self.materialized,
            Arc::new(MaterializedNodes::default()),
        )));
        if let Some(sink) = self.sink.take() {
            sink.close();
            if let Some(bridge) = self.bridge.take() {
                self.retirement.close(bridge, sink, worker, pins);
                return;
            }
            pins.push(Box::new(sink));
        }
        worker.close_with(pins);
    }
}
impl Drop for PreparedHost {
    fn drop(&mut self) {
        self.close();
    }
}
