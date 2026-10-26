//! Additive native-selection lane. Legacy goto/listing fallbacks are unchanged.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExistingPathKind {
    File,
    Folder,
}
// Physical read admission survives App/window replacement through callback consumption.
#[cfg(test)]
static TEST_LANE: Mutex<()> = Mutex::new(());
static EXISTING_PATH_BUSY: AtomicBool = AtomicBool::new(false);
struct Occupancy;
impl Drop for Occupancy {
    fn drop(&mut self) {
        EXISTING_PATH_BUSY.store(false, Ordering::Release);
    }
}
#[derive(Debug)]
struct ScopeOwner;
impl PartialEq for ScopeOwner {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
impl Eq for ScopeOwner {}
#[derive(Debug)]
struct ErrorOwner;
impl PartialEq for ErrorOwner {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
impl Eq for ErrorOwner {}
/// Receipt-owned post-settlement authority; unrelated semantic revisions are excluded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExistingPathFocusStamp {
    pub window: u64,
    pub pane: usize,
    document: u64,
    focus: u64,
    folder: Option<String>,
    listing: u64,
    navigation: u64,
    epoch: u64,
    owner: Arc<ScopeOwner>,
    context_generation: u64,
    context: crate::input::InputContext,
    error_owner: Option<Arc<ErrorOwner>>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExistingPathScope {
    pub window: u64,
    pub document: u64,
    pub focus: u64,
    pub pane: usize,
    pub folder: Option<String>,
    pub listing: u64,
    navigation: u64,
    epoch: u64,
    owner: Arc<ScopeOwner>,
    context_generation: u64,
    hidden: bool,
    sort: usize,
    filter: Option<String>,
}
#[derive(Debug)]
pub struct ExistingPathReceipt {
    pub request_id: u64,
    pub scope: ExistingPathScope,
    pub listing_generation: u64,
    pub exact_target: Option<PathBuf>,
    pub result: Result<(), String>,
    pub focus_stamp: Option<ExistingPathFocusStamp>,
}
#[derive(Debug)]
struct Loaded {
    folder: PathBuf,
    target: Option<PathBuf>,
    files: Vec<FEntry>,
}
struct Reply {
    _occupancy: Occupancy,
    id: u64,
    exact_target: Option<PathBuf>,
    scope: ExistingPathScope,
    result: Result<Loaded, String>,
}
pub(super) struct State {
    owner: Arc<ScopeOwner>,
    error_owner: Option<Arc<ErrorOwner>>,
    tx: mpsc::Sender<Reply>,
    rx: mpsc::Receiver<Reply>,
    #[cfg(test)]
    before_load: Option<Arc<dyn Fn() + Send + Sync>>,
    generation: u64,
    pending: Option<(u64, ExistingPathScope)>,
    receipts: VecDeque<ExistingPathReceipt>,
}
impl Default for State {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            owner: Arc::new(ScopeOwner),
            error_owner: None,
            tx,
            rx,
            #[cfg(test)]
            before_load: None,
            generation: 0,
            pending: None,
            receipts: VecDeque::new(),
        }
    }
}
#[path = "application_existing_path_read.rs"]
mod reader;
use reader::load;
#[cfg(test)]
use reader::read_entries;
impl App {
    // Every installation owns a distinct error identity, even equal Status values.
    pub(super) fn observe_existing_path_status(&mut self, is_error: bool) {
        self.existing_paths.error_owner = is_error.then(|| Arc::new(ErrorOwner));
    }
    /// Comparison only: events must retain the stamp captured by their settlement.
    pub fn existing_path_focus_stamp(&self) -> Option<ExistingPathFocusStamp> {
        let pane = self.active_pane;
        if self.window_generation == 0
            || pane >= 2
            || (pane == 1 && !self.split)
            || self.initializing
            || !self.panes[pane].listing_settled
        {
            return None;
        }
        let context = self.input_context();
        let error_owner = match context {
            crate::input::InputContext::Pane(p) if p == pane => None,
            crate::input::InputContext::Error => Some(self.existing_paths.error_owner.clone()?),
            _ => return None,
        };
        Some(ExistingPathFocusStamp {
            window: self.window_generation,
            pane,
            document: self.document_generation,
            focus: self.focus_generation,
            folder: self.panes[pane].folder.as_ref().map(|f| f.path.clone()),
            listing: self.panes[pane].listing_generation,
            navigation: self.navigation_generation[pane],
            epoch: self.operation_epoch.load(Ordering::Acquire),
            owner: self.existing_paths.owner.clone(),
            context_generation: self.existing_paths.generation,
            context,
            error_owner,
        })
    }
    pub fn existing_path_focus_is_current(&self, stamp: &ExistingPathFocusStamp) -> bool {
        self.existing_path_focus_stamp().as_ref() == Some(stamp)
    }
    pub fn existing_path_scope(&self, pane: usize) -> Option<ExistingPathScope> {
        if self.window_generation == 0
            || pane >= 2
            || pane != self.active_pane
            || (pane == 1 && !self.split)
            || self.initializing
            || !matches!(self.input_context(),crate::input::InputContext::Pane(p) if p==pane)
            || self.navigation_inflight[pane].is_some()
            || self.navigation_waiting_listing[pane]
            || self.pending_operations() != 0
            || !self.panes[pane].listing_settled
        {
            return None;
        }
        Some(ExistingPathScope {
            window: self.window_generation,
            document: self.document_generation,
            focus: self.focus_generation,
            pane,
            folder: self.panes[pane].folder.as_ref().map(|f| f.path.clone()),
            listing: self.panes[pane].listing_generation,
            owner: self.existing_paths.owner.clone(),
            context_generation: self.existing_paths.generation,
            hidden: self.show_hidden,
            sort: self.panes[pane].sort_mode,
            filter: self.panes[pane].filter_query.clone(),
            navigation: self.navigation_generation[pane],
            epoch: self.operation_epoch.load(Ordering::Acquire),
        })
    }
    pub fn existing_path_scope_is_current(&self, scope: &ExistingPathScope) -> bool {
        self.existing_path_scope(scope.pane).as_ref() == Some(scope)
    }
    pub fn request_existing_path(
        &mut self,
        pane: usize,
        path: PathBuf,
        kind: ExistingPathKind,
        scope: ExistingPathScope,
        id: u64,
    ) -> Result<(), String> {
        if pane != scope.pane || !self.existing_path_scope_is_current(&scope) {
            return Err("Chooser scope expired".into());
        }
        if self.existing_paths.pending.is_some() {
            return Err("Selected path worker is busy".into());
        }
        if EXISTING_PATH_BUSY
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("Selected path physical worker is busy".into());
        }
        let occupancy = Occupancy;
        let tx = self.existing_paths.tx.clone();
        let captured = scope.clone();
        let exact_target = if kind == ExistingPathKind::File {
            Some(path.clone())
        } else {
            None
        };
        let hidden = self.show_hidden;
        let sort = self.panes[pane].sort_mode;
        self.existing_paths.pending = Some((id, scope));
        #[cfg(test)]
        let before_load = self.existing_paths.before_load.clone();
        if let Err(error) = thread::Builder::new()
            .name("ira-existing-path".into())
            .spawn(move || {
                #[cfg(test)]
                if let Some(hook) = before_load {
                    hook();
                }
                let result = load(path, kind, hidden, sort);
                let _ = tx.send(Reply {
                    _occupancy: occupancy,
                    id,
                    exact_target,
                    scope: captured,
                    result,
                });
            })
        {
            self.existing_paths.pending = None;
            return Err(format!("Selected path worker unavailable: {error}"));
        }
        Ok(())
    }
    pub(super) fn drain_existing_paths(&mut self) {
        while let Ok(reply) = self.existing_paths.rx.try_recv() {
            if self.existing_paths.pending.as_ref() != Some(&(reply.id, reply.scope.clone())) {
                continue;
            }
            self.existing_paths.pending = None;
            if !self.existing_path_scope_is_current(&reply.scope) {
                continue;
            }
            let pane_index = reply.scope.pane;
            let before_context = self.input_context();
            let before_focus = self.focus_generation;
            let (target, result) = match reply.result {
                Err(error) => {
                    self.set_status(error.clone(), true);
                    (reply.exact_target, Err(error))
                }
                Ok(loaded) => {
                    let pane = &mut self.panes[pane_index];
                    // Representability was validated on the worker before any mutation.
                    let identity = loaded.folder.to_str().expect("validated folder").to_owned();
                    let label = loaded
                        .folder
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&identity)
                        .to_owned();
                    pane.folder = Some(Folder::new(label, identity, '#'));
                    pane.files = loaded.files;
                    pane.selected = vec![false; pane.files.len()];
                    pane.filter_query = None;
                    pane.filter_indices.clear();
                    pane.pending_select = None;
                    pane.render_scroll = 0;
                    pane.grid_top = 0;
                    pane.user_navigated = false;
                    pane.listing_generation = pane.listing_generation.wrapping_add(1);
                    pane.listing_settled = true;
                    pane.projection_generation = pane.projection_generation.wrapping_add(1);
                    pane.state.select(
                        loaded
                            .target
                            .as_ref()
                            .and_then(|t| pane.files.iter().position(|f| Path::new(&f.path) == t))
                            .or_else(|| {
                                if loaded.target.is_none() && !pane.files.is_empty() {
                                    Some(0)
                                } else {
                                    None
                                }
                            }),
                    );
                    self.navigation_generation[pane_index] =
                        self.navigation_generation[pane_index].wrapping_add(1);
                    self.search_query = None;
                    self.semantic_changed();
                    (loaded.target, Ok(()))
                }
            };
            // Observe the chooser-owned transition now, before tick's peer drains.
            if self.input_context() != before_context && self.focus_generation == before_focus {
                self.focus_generation = self.focus_generation.wrapping_add(1);
            }
            let focus_stamp = self.existing_path_focus_stamp();
            self.existing_paths.receipts.push_back(ExistingPathReceipt {
                request_id: reply.id,
                scope: reply.scope,
                listing_generation: self.panes[pane_index].listing_generation,
                exact_target: target,
                result,
                focus_stamp,
            });
        }
    }
    pub fn invalidate_existing_path_requests(&mut self) {
        self.existing_paths.generation = self.existing_paths.generation.wrapping_add(1);
    }
    pub fn take_existing_path_receipts(&mut self) -> Vec<ExistingPathReceipt> {
        self.existing_paths.receipts.drain(..).collect()
    }
}
#[cfg(test)]
#[path = "application_existing_path_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "application_existing_path_neighbor_tests.rs"]
mod neighbor_tests;
#[cfg(test)]
#[path = "application_existing_path_settlement_tests.rs"]
mod settlement_tests;
#[cfg(test)]
#[path = "application_existing_path_sort_tests.rs"]
mod sort_tests;
