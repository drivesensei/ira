//! Host-only accessibility provider; semantic queries do not call the filesystem or actor.
#[cfg(target_os = "macos")]
pub mod macos;
pub mod model;
#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "macos")]
pub use macos::NativeBridge;
use model::{Action, FrameKey, NodeId, PreparedFrame, SemanticTree, Stamp, Target};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
};
#[cfg(target_os = "windows")]
pub use windows::NativeBridge;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    Stale,
    Disabled,
    Closing,
    Backpressure,
    Unsupported,
    InvalidRange,
}
#[derive(Clone, Debug)]
pub struct AccessibilityIntent {
    pub node: NodeId,
    pub stamp: Stamp,
    pub action: Action,
}
#[derive(Clone, Debug)]
pub struct ResolvedAction {
    pub node: NodeId,
    pub stamp: Stamp,
    pub target: Target,
    pub action: Action,
    pub prepared_key: Option<model::RequestKey>,
}
struct PublicationState {
    tree: Arc<SemanticTree>,
    frame: Option<Arc<PreparedFrame>>,
    sequence: u64,
}
/// Transfer this ownership to the preparation worker; dropping it may destroy O(N) nodes.
pub struct RetiredPublication {
    pub tree: Arc<SemanticTree>,
    pub frame: Option<Arc<PreparedFrame>>,
}
struct QueuedIntent {
    intent: AccessibilityIntent,
    prepared_key: Option<model::RequestKey>,
}
struct Shared {
    tree: Mutex<PublicationState>,
    closing: AtomicBool,
}
#[derive(Clone)]
pub struct ActionSink {
    shared: Arc<Shared>,
    sender: SyncSender<QueuedIntent>,
}
pub struct ActionReceiver {
    shared: Arc<Shared>,
    receiver: Receiver<QueuedIntent>,
}
fn resolve(tree: &SemanticTree, intent: &AccessibilityIntent) -> Result<ResolvedAction, Rejection> {
    if intent.stamp != tree.stamp {
        return Err(Rejection::Stale);
    }
    let node = tree.nodes.get(&intent.node).ok_or(Rejection::Stale)?;
    if !node.enabled || !tree.in_modal_scope(intent.node) {
        return Err(Rejection::Disabled);
    }
    if !node.capabilities.contains(&intent.action.capability()) {
        return Err(Rejection::Unsupported);
    }
    if let Action::SetSelection(range) = &intent.action
        && !model::valid_text_range(node.value.as_deref().unwrap_or(""), range)
    {
        return Err(Rejection::InvalidRange);
    }
    Ok(ResolvedAction {
        node: intent.node,
        stamp: intent.stamp,
        target: node.target.clone(),
        action: intent.action.clone(),
        prepared_key: None,
    })
}
impl ActionSink {
    pub fn channel(tree: Arc<SemanticTree>, capacity: usize) -> (Self, ActionReceiver) {
        let (sender, receiver) = mpsc::sync_channel(capacity.max(1));
        let shared = Arc::new(Shared {
            tree: Mutex::new(PublicationState {
                tree,
                frame: None,
                sequence: 0,
            }),
            closing: AtomicBool::new(false),
        });
        (
            Self {
                shared: shared.clone(),
                sender,
            },
            ActionReceiver { shared, receiver },
        )
    }
    pub fn try_dispatch(&self, intent: AccessibilityIntent) -> Result<(), Rejection> {
        self.try_dispatch_inner(intent, None)
    }
    /// Preserve the exact prepared key observed by a concurrent native callback.
    pub fn try_dispatch_prepared(
        &self,
        intent: AccessibilityIntent,
        key: model::RequestKey,
    ) -> Result<(), Rejection> {
        self.try_dispatch_inner(intent, Some(key))
    }
    fn try_dispatch_inner(
        &self,
        intent: AccessibilityIntent,
        expected: Option<model::RequestKey>,
    ) -> Result<(), Rejection> {
        if self.shared.closing.load(Ordering::Acquire) {
            return Err(Rejection::Closing);
        }
        let tree = self
            .shared
            .tree
            .try_lock()
            .map_err(|_| Rejection::Backpressure)?;
        if expected.is_some() && expected != tree.frame.as_ref().map(|f| f.key.request) {
            return Err(Rejection::Stale);
        }
        resolve(&tree.tree, &intent)?;
        let queued = QueuedIntent {
            intent,
            prepared_key: tree.frame.as_ref().map(|f| f.key.request),
        };
        self.sender.try_send(queued).map_err(|e| match e {
            TrySendError::Full(_) => Rejection::Backpressure,
            TrySendError::Disconnected(_) => Rejection::Closing,
        })
    }
    pub fn publish(&self, tree: Arc<SemanticTree>) -> Result<(), Rejection> {
        if self.shared.closing.load(Ordering::Acquire) {
            return Err(Rejection::Closing);
        }
        let mut slot = self
            .shared
            .tree
            .try_lock()
            .map_err(|_| Rejection::Backpressure)?;
        if slot.tree.stamp.window != tree.stamp.window
            || tree.stamp.revision < slot.tree.stamp.revision
            || (tree.stamp.revision == slot.tree.stamp.revision
                && tree.layout_revision < slot.tree.layout_revision)
            || (tree.stamp.document == slot.tree.stamp.document
                && tree.stamp.focus == slot.tree.stamp.focus
                && tree.stamp.text_revision < slot.tree.stamp.text_revision)
        {
            return Err(Rejection::Stale);
        }
        slot.tree = tree;
        slot.frame = None;
        slot.sequence = 0;
        Ok(())
    }
    /// Commit sink and native cache under one gate. The closure must be infallible and
    /// must not call native notification/client code while the gate is held.
    pub fn install_prepared(
        &self,
        frame: &Arc<PreparedFrame>,
        expected: FrameKey,
        install_cache: impl FnOnce(),
    ) -> Result<RetiredPublication, Rejection> {
        if self.is_closing() {
            return Err(Rejection::Closing);
        }
        if frame.key != expected {
            return Err(Rejection::Stale);
        }
        let mut slot = self
            .shared
            .tree
            .try_lock()
            .map_err(|_| Rejection::Backpressure)?;
        if self.is_closing() {
            return Err(Rejection::Closing);
        }
        if frame.base_publication_seq != slot.sequence
            || frame.publication_seq <= slot.sequence
            || frame.semantic.tree.stamp.window != slot.tree.stamp.window
            || frame
                .base_tree
                .as_ref()
                .is_none_or(|base| !Arc::ptr_eq(base, &slot.tree))
        {
            return Err(Rejection::Stale);
        }
        install_cache();
        let tree = std::mem::replace(&mut slot.tree, frame.semantic.tree.clone());
        let retired_frame = slot.frame.replace(frame.clone());
        slot.sequence = frame.publication_seq;
        Ok(RetiredPublication {
            tree,
            frame: retired_frame,
        })
    }
    pub fn current_frame(&self) -> Result<Option<Arc<PreparedFrame>>, Rejection> {
        self.shared
            .tree
            .try_lock()
            .map(|slot| slot.frame.clone())
            .map_err(|_| Rejection::Backpressure)
    }
    /// Bounded attachment preflight; current() remains available for retirement.
    fn attachment_tree(&self) -> Result<Arc<SemanticTree>, Rejection> {
        if self.is_closing() {
            return Err(Rejection::Closing);
        }
        let tree = self.current()?;
        if self.is_closing() {
            return Err(Rejection::Closing);
        }
        Ok(tree)
    }
    pub fn current(&self) -> Result<Arc<SemanticTree>, Rejection> {
        self.shared
            .tree
            .try_lock()
            .map(|t| t.tree.clone())
            .map_err(|_| Rejection::Backpressure)
    }
    /// Close first, then transfer large pure ownership to the worker. A callback
    /// holding the gate causes bounded backpressure; retry without waiting on UI.
    pub fn close_and_retire(&self) -> Result<RetiredPublication, Rejection> {
        self.close();
        let mut slot = self
            .shared
            .tree
            .try_lock()
            .map_err(|_| Rejection::Backpressure)?;
        let empty = Arc::new(SemanticTree {
            stamp: slot.tree.stamp,
            layout_revision: slot.tree.layout_revision,
            root: slot.tree.root,
            focused: None,
            active_modal: None,
            nodes: std::collections::BTreeMap::new(),
        });
        let tree = std::mem::replace(&mut slot.tree, empty);
        let frame = slot.frame.take();
        Ok(RetiredPublication { tree, frame })
    }
    pub fn close(&self) {
        self.shared.closing.store(true, Ordering::Release);
    }
    pub fn is_closing(&self) -> bool {
        self.shared.closing.load(Ordering::Acquire)
    }
}
impl ActionReceiver {
    /// Validate again at dequeue; queued actions may be stale after a new snapshot/window teardown.
    pub fn try_next(&self) -> Option<Result<ResolvedAction, Rejection>> {
        match self.receiver.try_recv() {
            Ok(queued) => Some(if self.shared.closing.load(Ordering::Acquire) {
                Err(Rejection::Closing)
            } else {
                self.shared
                    .tree
                    .try_lock()
                    .map_err(|_| Rejection::Backpressure)
                    .and_then(|tree| {
                        if queued.prepared_key != tree.frame.as_ref().map(|f| f.key.request) {
                            return Err(Rejection::Stale);
                        }
                        let mut resolved = resolve(&tree.tree, &queued.intent)?;
                        resolved.prepared_key = queued.prepared_key;
                        Ok(resolved)
                    })
            }),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    }
}

/// A successful native commit always returns this, even if notification delivery failed.
/// ACK publication_seq before handling the optional delivery error; retire on worker.
pub struct PublishedOutcome<E> {
    pub publication_seq: u64,
    pub retired: RetiredPublication,
    pub notification_error: Option<E>,
    pub converted_frames: usize,
    pub notification_visits: usize,
    pub cleanup_visits: usize,
}
