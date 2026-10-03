//! Stand-ins for native construction and cache data; production query logic is mirrored in parent.
use super::*;
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Unavailable,
    Unsupported,
    Invalid,
}
impl Error {
    pub(crate) fn from_hresult(_: i32) -> Self {
        Self::Invalid
    }
}
pub(crate) type Result<T> = std::result::Result<T, Error>;
pub(crate) type IRawElementProviderFragment = NodeId;
pub(crate) type NavigateDirection = i32;
pub(crate) const NavigateDirection_Parent: i32 = 0;
pub(crate) const NavigateDirection_FirstChild: i32 = 1;
pub(crate) const NavigateDirection_LastChild: i32 = 2;
pub(crate) const NavigateDirection_NextSibling: i32 = 3;
pub(crate) const NavigateDirection_PreviousSibling: i32 = 4;
pub(crate) const E_INVALIDARG: i32 = 5;
pub(crate) fn unavailable() -> Error {
    Error::Unavailable
}
pub(crate) fn unsupported() -> Error {
    Error::Unsupported
}
pub(crate) fn dispatch_error(e: Rejection) -> Error {
    match e {
        Rejection::Unsupported => unsupported(),
        _ => unavailable(),
    }
}
pub(crate) struct Cached {
    pub(crate) tree: Arc<SemanticTree>,
    pub(crate) prepared: Option<Arc<PreparedFrame>>,
}
pub(crate) struct State {
    pub(crate) cache: Mutex<Cached>,
    pub(crate) closing: AtomicBool,
    pub(crate) sink: ActionSink,
}
impl State {
    pub(crate) fn is_closing(&self) -> bool {
        self.closing.load(Ordering::Acquire) || self.sink.is_closing()
    }
    pub(crate) fn tree(&self) -> Result<Arc<SemanticTree>> {
        if self.closing.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        Ok(self
            .cache
            .try_lock()
            .map_err(|_| unavailable())?
            .tree
            .clone())
    }
    // The only fake native boundary: successful COM factory returns the requested ID.
    pub(crate) fn fragment(&self, id: NodeId) -> Result<NodeId> {
        Ok(id)
    }
}
pub(crate) fn fixture(count: usize) -> Snapshot {
    let mut s = App::default().snapshot();
    s.window_generation = 17;
    s.focus_generation = 3;
    s.document_generation = 9;
    s.revision = 1;
    s.input_context = InputContext::Pane(0);
    s.panes[0].folder = Some(Folder::new("fixture".into(), "/synthetic".into(), 'f'));
    s.panes[0].listing_generation = 5;
    s.panes[0].listing_settled = true;
    s.panes[0].rows = (0..count)
        .map(|i| Row {
            underlying_index: i,
            entry: FEntry {
                path: format!("/synthetic/{i}"),
                label: i.to_string(),
                is_dir: false,
                size: 3,
                modified: None,
            },
            selected: false,
            deleting: false,
        })
        .collect::<Vec<_>>()
        .into();
    s
}
pub(crate) fn make_state(
    s: &Snapshot,
    model: &mut AccessibilityModel,
    prepared: bool,
) -> Arc<State> {
    let tree = Arc::new(model.project(s, None, &LayoutSnapshot::default()));
    let p = if prepared {
        let key = RequestKey {
            window_generation: s.window_generation,
            semantic_revision: s.revision,
            document_generation: s.document_generation,
            focus_generation: s.focus_generation,
            native_text_revision: 0,
            host_focus_revision: 0,
            host_presentation_revision: 0,
        };
        Some(Arc::new(
            PreparedFrame::compatibility_baseline(tree.clone(), key).unwrap(),
        ))
    } else {
        None
    };
    Arc::new(State {
        sink: ActionSink::channel(tree.clone(), 1).0,
        cache: Mutex::new(Cached { tree, prepared: p }),
        closing: AtomicBool::new(false),
    })
}
