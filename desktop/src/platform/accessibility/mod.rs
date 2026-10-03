//! Host-only accessibility provider; semantic queries do not call the filesystem or actor.
#[cfg(target_os = "macos")]
pub mod macos;
pub mod model;
#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "macos")]
pub use macos::NativeBridge;
use model::{Action, NodeId, SemanticTree, Stamp, Target};
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
}
struct Shared {
    tree: Mutex<Arc<SemanticTree>>,
    closing: AtomicBool,
}
#[derive(Clone)]
pub struct ActionSink {
    shared: Arc<Shared>,
    sender: SyncSender<AccessibilityIntent>,
}
pub struct ActionReceiver {
    shared: Arc<Shared>,
    receiver: Receiver<AccessibilityIntent>,
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
    })
}
impl ActionSink {
    pub fn channel(tree: Arc<SemanticTree>, capacity: usize) -> (Self, ActionReceiver) {
        let (sender, receiver) = mpsc::sync_channel(capacity.max(1));
        let shared = Arc::new(Shared {
            tree: Mutex::new(tree),
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
        if self.shared.closing.load(Ordering::Acquire) {
            return Err(Rejection::Closing);
        }
        let tree = self
            .shared
            .tree
            .try_lock()
            .map_err(|_| Rejection::Backpressure)?;
        resolve(&tree, &intent)?;
        self.sender.try_send(intent).map_err(|e| match e {
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
        if slot.stamp.window != tree.stamp.window
            || tree.stamp.revision < slot.stamp.revision
            || (tree.stamp.revision == slot.stamp.revision
                && tree.layout_revision < slot.layout_revision)
            || (tree.stamp.document == slot.stamp.document
                && tree.stamp.focus == slot.stamp.focus
                && tree.stamp.text_revision < slot.stamp.text_revision)
        {
            return Err(Rejection::Stale);
        }
        *slot = tree;
        Ok(())
    }
    pub fn current(&self) -> Result<Arc<SemanticTree>, Rejection> {
        self.shared
            .tree
            .try_lock()
            .map(|t| t.clone())
            .map_err(|_| Rejection::Backpressure)
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
            Ok(intent) => Some(if self.shared.closing.load(Ordering::Acquire) {
                Err(Rejection::Closing)
            } else {
                self.shared
                    .tree
                    .try_lock()
                    .map_err(|_| Rejection::Backpressure)
                    .and_then(|tree| resolve(&tree, &intent))
            }),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    }
}
