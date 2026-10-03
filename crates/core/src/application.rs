//! Imperative, actor-owned application extracted from src/app.rs at 25ffceb.
//! The host must execute this object on a background actor, never a GPUI view.
use crate::{
    cursor::CursorState as ListState,
    domain::data::Folder,
    model::*,
    services::{
        bookmarks::{next_free_shortcut, read_bookmarks, read_bookmarks_from},
        drives::{eject_drive, mount_drive},
        file_info::{
            build_info_fast, build_info_full, dir_size, on_disk_bytes, size_line_final, DirSize,
            InfoEvent, SizeInfo, WalkHandle,
        },
        folders::list_common_folders,
        list_files::{list_files_bounded, list_files_chunked, FEntry, LISTING_CHUNK},
        state::{load_state, load_state_from, SessionState, SizeEntry},
        transfer::{
            spawn_delete_job, spawn_job, spawn_job_with_provider, Job, JobControl, JobEvent,
            JobKind, JobStatus, NoReplaceProvider, OverwritePolicy,
        },
    },
    utils::{
        fuzzy::fuzzy_score,
        is_dir::{get_directory, get_parent_directory},
    },
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::{
    error,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime},
};
#[cfg(test)]
type TransferProbeTest = Arc<dyn Fn(&str) -> bool + Send + Sync>;

// Process-wide admission survives App/window destruction. Root and neutral core
// each have one finite lane; a mixed process therefore has at most two lanes.
static TRANSFER_PROBE_BUSY: AtomicBool = AtomicBool::new(false);
struct TransferProbeOccupancy(Arc<AtomicBool>);
impl Drop for TransferProbeOccupancy {
    fn drop(&mut self) {
        // A reply, if any, was sent before this completion signal. A panic or
        // failed spawn also completes physical work without inventing a reply.
        self.0.store(true, std::sync::atomic::Ordering::Release);
        TRANSFER_PROBE_BUSY.store(false, std::sync::atomic::Ordering::Release);
    }
}

pub type AppResult<T> = std::result::Result<T, Box<dyn error::Error>>;
struct WalkSlot {
    handle: WalkHandle,
    started: Instant,
}
type BoundedListing = (usize, u64, std::io::Result<(Vec<FEntry>, bool)>);
#[derive(Clone, Copy)]
enum BlockingOperation {
    EnterFolder,
    ParentFolder,
    Create,
    Goto,
    Rename,
    Drive(usize),
    Eject,
}
#[derive(Clone)]
struct OperationState {
    panes: [Pane; 2],
    active_pane: usize,
    show_hidden: bool,
    search_query: Option<String>,
    renaming: Option<RenamePrompt>,
    new_entry: Option<NewEntryPrompt>,
    goto_prompt: Option<String>,
    drives: Option<Vec<Folder>>,
}
struct OperationRequest {
    epoch: u64,
    navigation_ticket: Option<u64>,
    operation: BlockingOperation,
    state: OperationState,
}
struct OperationResult {
    epoch: u64,
    navigation_ticket: Option<u64>,
    before: OperationState,
    source: [(Option<String>, u64); 2],
    state: OperationState,
    status: Option<Status>,
    listings: Vec<(usize, bool)>,
    host_requests: Vec<HostRequest>,
}
enum PersistenceRequest {
    State(Option<PathBuf>, SessionState),
    Bookmarks(Option<PathBuf>, Vec<Folder>),
    Barrier(mpsc::Sender<()>),
    CheckedBarrier(mpsc::Sender<Result<PersistenceReceipt, PersistenceFailure>>),
}
#[derive(PartialEq, Eq)]
struct PaneTickSemantics {
    projection: u64,
    listing: u64,
    settled: bool,
    folder: Option<Folder>,
    cursor: Option<usize>,
    pending_select: Option<String>,
    user_navigated: bool,
}
#[derive(PartialEq, Eq)]
struct TickSemantics {
    panes: [PaneTickSemantics; 2],
    initializing: bool,
    running: bool,
    context: crate::input::InputContext,
    renaming: Option<RenamePrompt>,
    new_entry: Option<NewEntryPrompt>,
    goto_prompt: Option<String>,
    search_query: Option<String>,
    status: Option<Status>,
    transfer_dest: Option<TransferDestSync>,
}
fn same_entries(left: &[FEntry], right: &[FEntry]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(a, b)| {
            a.path == b.path
                && a.label == b.label
                && a.is_dir == b.is_dir
                && a.size == b.size
                && a.modified == b.modified
        })
}
struct SearchProjection {
    pane: usize,
    generation: u64,
    files: usize,
    query: String,
    indices: Arc<Vec<usize>>,
}

pub struct App {
    search_projection: std::cell::RefCell<Option<SearchProjection>>,
    pub(crate) snapshot_cache: [std::cell::RefCell<Option<crate::observable::PaneProjection>>; 2],
    pub(crate) deletion_generation: u64,
    last_sequence: Option<u64>,
    operation_epoch: Arc<AtomicU64>,
    pending_editor: Option<OpenEditorRequest>,
    next_document_id: u64,
    transfer_probe_tx: mpsc::Sender<(u64, TransferDestSync, bool)>,
    transfer_probe_rx: mpsc::Receiver<(u64, TransferDestSync, bool)>,
    transfer_generation: u64,
    transfer_probe_pending: Option<u64>,
    transfer_probe_finished: Option<Arc<AtomicBool>>,
    transfer_probe_last_attempt: Option<Instant>,
    #[cfg(test)]
    transfer_probe_test: Option<TransferProbeTest>,
    #[cfg(test)]
    transfer_probe_spawn_error: bool,
    startup_tx: mpsc::Sender<(SessionState, Vec<(String, String)>)>,
    startup_rx: mpsc::Receiver<(SessionState, Vec<(String, String)>)>,
    initializing: bool,
    startup_inputs: VecDeque<crate::input::Input>,
    clock_override: Option<Instant>,
    existing_paths: existing_path::State,
    navigation_generation: [u64; 2],
    navigation_inflight: [Option<u64>; 2],
    navigation_waiting_listing: [bool; 2],
    relative_navigation: VecDeque<(usize, u64, BlockingOperation)>,
    pending_operations: usize,
    persistence_tx: mpsc::Sender<PersistenceRequest>,
    pub bookmarks_path: Option<PathBuf>,
    operation_tx: Option<mpsc::Sender<OperationRequest>>,
    operation_result_tx: mpsc::Sender<OperationResult>,
    operation_result_rx: mpsc::Receiver<OperationResult>,
    defer_listings: bool,
    deferred_listings: Vec<(usize, bool)>,
    bounded_tx: mpsc::Sender<BoundedListing>,
    bounded_rx: mpsc::Receiver<BoundedListing>,
    /// Is the application running?
    pub running: bool,

    /// size checks
    pub size: (u16, u16),

    pub drives: Option<Vec<Folder>>,

    pub folders: Option<Vec<Folder>>,
    pub bookmarks: Option<Vec<Folder>>,

    /// The two file-browser panes (the right one is used only when `split`).
    pub panes: [Pane; 2],
    /// Index of the pane that receives navigation input.
    pub active_pane: usize,
    /// Whether the files area is split into two side-by-side panes.
    pub split: bool,

    /// Active fuzzy-search query; `None` when not searching.
    pub search_query: Option<String>,
    /// Whether hidden entries (dotfiles) are listed.
    pub show_hidden: bool,

    /// Active copy/move jobs (newest last).
    pub jobs: Vec<Job>,
    /// Whether the Copy Board sidebar is open.
    pub copy_board: bool,
    /// Whether keyboard focus is on the Copy Board.
    pub board_focused: bool,
    /// Copy Board job selection.
    pub copy_board_state: ListState,

    /// Pending destructive-action confirmation (delete).
    pub confirming: Option<Confirm>,
    /// Active rename edit; `None` when not renaming.
    pub renaming: Option<RenamePrompt>,
    /// Open metadata dialog; `None` when closed.
    pub info: Option<InfoDialog>,
    /// Aggregate info dialog for a multi-selection; `None` when closed.
    pub multi_info: Option<MultiInfoState>,
    /// "Create new" dialog (`n`); `None` when closed.
    pub new_entry: Option<NewEntryPrompt>,
    /// "Go to path" dialog (`[`); `None` when closed. Existing paths are
    /// navigated to; missing ones are created (nested, kind by extension).
    pub goto_prompt: Option<String>,
    /// Live destination sync while a transfer writes into a folder.
    pub transfer_dest: Option<TransferDestSync>,
    /// Keybindings help dialog (`*`); closed by any key.
    pub keybindings_visible: bool,
    /// Marquee scroll offset (chars) for the contextual hint bar; advanced
    /// on every tick and wrapped modulo the rendered text length.
    pub hint_offset: usize,

    /// Transient status/error message shown in the bottom bar until it
    /// expires (or the next action replaces it).
    pub status: Option<Status>,

    /// In-progress batch deletion (background worker); `None` when idle.
    pub deletion: Option<DeletionState>,
    /// The deletion progress dialog was dismissed with a key; stays hidden
    /// until the deletion finishes.
    pub deletion_box_hidden: bool,
    /// Paths queued for/being deleted (drives the file-list spinners).
    deleting_paths: HashSet<String>,
    /// Scroll acceleration state: direction (+1 down / -1 up / 0 idle),
    /// consecutive repeats within [`SCROLL_REPEAT_WINDOW`], and the time of
    /// the last move.
    scroll_dir: i8,
    scroll_repeat: u32,
    last_scroll: Option<Instant>,

    transfer_provider: Option<Arc<NoReplaceProvider>>,
    job_tx: mpsc::Sender<JobEvent>,
    job_rx: mpsc::Receiver<JobEvent>,
    info_tx: mpsc::Sender<InfoEvent>,
    info_rx: mpsc::Receiver<InfoEvent>,
    /// Folder sizes measured this session (partial while walking), keyed by
    /// path. Survives dialog dismissal; re-querying shows it instantly.
    size_cache: HashMap<String, SizeInfo>,
    /// Active background walks keyed by path (one per folder, cancellable).
    size_walks: HashMap<String, WalkSlot>,
    next_job_id: u64,

    /// Latest drive list produced by the background poller.
    drive_cache: Arc<Mutex<Vec<Folder>>>,
    /// Monotonic counter bumped when the background poller publishes a new
    /// drive list. `refresh_drives` only re-renders when this changes.
    drive_generation: Arc<Mutex<u64>>,
    /// Generation observed on the last `refresh_drives()` call.
    seen_drive_generation: u64,
    /// Whether a background drive poller is running for this app.
    /// (Only the pool thread writes this; the UI thread reads it.)

    /// Startup file listings run on worker threads and are delivered here, so
    /// a slow folder (e.g. a cold spin-up HDD) can't block `App::new()`.
    file_list_tx: mpsc::Sender<(usize, Vec<FEntry>, bool, u64)>,
    file_list_rx: mpsc::Receiver<(usize, Vec<FEntry>, bool, u64)>,
    pub edit: Option<EditState>,
    pub edit_focus: bool,
    pub state_path: Option<PathBuf>,
    pub theme_preset: crate::theme::ThemePreset,
    pub icons: crate::theme::icons::IconSet,
    pub revision: u64,
    pub ack_sequence: u64,
    pub window_generation: u64,
    pub document_generation: u64,
    pub focus_generation: u64,
    pub host_requests: Vec<HostRequest>,
}
impl Default for App {
    fn default() -> Self {
        let (transfer_probe_tx, transfer_probe_rx) = mpsc::channel();
        let (startup_tx, startup_rx) = mpsc::channel();
        let (persistence_tx, persistence_rx) = mpsc::channel();
        thread::spawn(move || application_persistence::worker(persistence_rx));
        let (operation_result_tx, operation_result_rx) = mpsc::channel();
        let (bounded_tx, bounded_rx) = mpsc::channel();
        let (job_tx, job_rx) = mpsc::channel();
        let (file_list_tx, file_list_rx) = mpsc::channel();
        let (info_tx, info_rx) = mpsc::channel();
        Self {
            search_projection: std::cell::RefCell::new(None),
            snapshot_cache: std::array::from_fn(|_| std::cell::RefCell::new(None)),
            deletion_generation: 0,
            last_sequence: None,
            operation_epoch: Arc::new(AtomicU64::new(0)),
            pending_editor: None,
            next_document_id: 1,
            transfer_probe_tx,
            transfer_probe_rx,
            transfer_generation: 0,
            transfer_probe_pending: None,
            transfer_probe_finished: None,
            transfer_probe_last_attempt: None,
            #[cfg(test)]
            transfer_probe_test: None,
            #[cfg(test)]
            transfer_probe_spawn_error: false,
            startup_tx,
            startup_rx,
            initializing: false,
            startup_inputs: VecDeque::new(),
            clock_override: None,
            existing_paths: existing_path::State::default(),
            navigation_generation: [0; 2],
            navigation_inflight: [None; 2],
            navigation_waiting_listing: [false; 2],
            relative_navigation: VecDeque::new(),
            pending_operations: 0,
            persistence_tx,
            bookmarks_path: None,
            operation_tx: None,
            operation_result_tx,
            operation_result_rx,
            defer_listings: false,
            deferred_listings: Vec::new(),
            bounded_tx,
            bounded_rx,
            running: true,
            size: (1024, 768),
            drives: None,
            folders: Some(list_common_folders()),
            bookmarks: Some(Vec::new()),
            panes: [Pane::default(), Pane::default()],
            state_path: None,
            active_pane: 0,
            search_query: None,
            show_hidden: false,
            split: false,
            jobs: Vec::new(),
            copy_board: false,
            board_focused: false,
            copy_board_state: ListState::default(),
            confirming: None,
            renaming: None,
            new_entry: None,
            goto_prompt: None,
            transfer_dest: None,
            info: None,
            multi_info: None,
            status: None,
            keybindings_visible: false,
            hint_offset: 0,
            deletion: None,
            deletion_box_hidden: false,
            deleting_paths: HashSet::new(),
            scroll_dir: 0,
            scroll_repeat: 0,
            last_scroll: None,
            transfer_provider: None,
            job_tx,
            job_rx,
            size_cache: HashMap::new(),
            size_walks: HashMap::new(),
            next_job_id: 0,
            drive_cache: Arc::new(Mutex::new(Vec::new())),
            drive_generation: Arc::new(Mutex::new(0)),
            seen_drive_generation: 0,
            file_list_tx,
            file_list_rx,
            info_tx,
            info_rx,
            edit: None,
            edit_focus: false,
            theme_preset: crate::theme::ThemePreset::default(),
            icons: crate::theme::icons::IconSet::Unicode,
            revision: 0,
            ack_sequence: 0,
            window_generation: 0,
            document_generation: 0,
            focus_generation: 0,
            host_requests: Vec::new(),
        }
    }
}

fn expand_path(raw: &str, base: Option<String>) -> std::path::PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" || trimmed.starts_with("~/") {
        if let Some(home) = dirs_next::home_dir() {
            let rest = trimmed.trim_start_matches('~').trim_start_matches('/');
            let joined = home.join(rest);
            let s = joined.to_string_lossy();
            // Drop the trailing separator home.join("") leaves behind.
            return std::path::PathBuf::from(s.trim_end_matches('/'));
        }
    }
    let p = std::path::Path::new(trimmed);
    if p.is_absolute() {
        p.to_path_buf()
    } else if let Some(b) = base {
        std::path::Path::new(&b).join(p)
    } else {
        p.to_path_buf()
    }
}

/// Kind rule shared by the create dialogs: a name whose last dot is not
/// leading and has a non-empty suffix is a file ("notes.txt"); otherwise a
/// folder ("notes", ".config", "backup.").
fn path_is_file_kind(path: &std::path::Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    match name.rsplit_once('.') {
        Some((stem, ext)) => !stem.is_empty() && !ext.is_empty(),
        None => false,
    }
}

/// Indices into `labels` matching `query` (fuzzy), best match first.
/// Empty query matches everything in order.
fn fuzzy_indices(labels: &[String], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..labels.len()).collect();
    }
    let mut scored: Vec<(u32, usize)> = labels
        .iter()
        .enumerate()
        .filter_map(|(i, label)| fuzzy_score(query, label).map(|s| (s, i)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

/// Finds the mounted removable drive whose mount point contains
/// `folder_path` (longest mount-point prefix wins).
fn matching_drive<'a>(drives: &'a [Folder], folder_path: &str) -> Option<&'a Folder> {
    drives
        .iter()
        .filter(|d| d.device.is_some() && !d.path.is_empty())
        .filter(|d| folder_path.starts_with(&d.path))
        .max_by_key(|d| d.path.len())
}

impl App {
    /// Mark an externally replaced or edited pane's files/selection/filter indices changed.
    /// Actor commands do this automatically; factory/test direct field edits after publication
    /// must call this before publishing another snapshot. Cursor changes need no invalidation.
    pub fn invalidate_pane_projection(&mut self, pane_index: usize) {
        if let Some(pane) = self.panes.get_mut(pane_index) {
            pane.projection_generation = pane.projection_generation.wrapping_add(1);
            self.semantic_changed();
        }
    }

    fn invalidate_active_projection(&mut self) {
        self.invalidate_pane_projection(self.active_pane);
    }

    /// Install the host atomic no-replace primitive for future transfer workers.
    /// Already-running workers retain their provider; default core behavior stays fail-closed.
    pub fn set_transfer_provider(&mut self, provider: Arc<NoReplaceProvider>) {
        self.transfer_provider = Some(provider);
    }

    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        Self::new_with_paths(None, None)
    }
    pub fn new_with_paths(state_path: Option<PathBuf>, bookmarks_path: Option<PathBuf>) -> Self {
        let mut app = Self {
            state_path,
            bookmarks_path,
            initializing: true,
            ..Self::default()
        };
        app.host_requests.push(HostRequest::RefreshDrives);
        let tx = app.startup_tx.clone();
        let state_path = app.state_path.clone();
        let bookmarks_path = app.bookmarks_path.clone();
        thread::spawn(move || {
            let state = match state_path {
                Some(path) => load_state_from(&path),
                None => load_state(),
            };
            let bookmarks = match bookmarks_path {
                Some(path) => read_bookmarks_from(&path),
                None => read_bookmarks(),
            };
            let _ = tx.send((state, bookmarks));
        });
        app
    }
    pub fn is_initializing(&self) -> bool {
        self.initializing
    }
    fn drain_startup(&mut self) {
        if let Ok((state, pairs)) = self.startup_rx.try_recv() {
            self.apply_bookmark_pairs(pairs);
            let preset = self.apply_session_state(state);
            if let Some(preset) = preset.and_then(|s| crate::theme::ThemePreset::parse(&s)) {
                self.theme_preset = preset;
            }
            self.initializing = false;
            self.request_pane_listing(0);
            self.request_pane_listing(1);
            while let Some(input) = self.startup_inputs.pop_front() {
                let _ = self.dispatch(input);
            }
        }
    }

    fn semantic_changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
    // Constant in row count. Row mutations carry the existing projection version;
    // jobs/info/drives notify at accepted event application, not publication cadence.
    fn tick_semantics(&self) -> TickSemantics {
        TickSemantics {
            panes: std::array::from_fn(|i| {
                let pane = &self.panes[i];
                PaneTickSemantics {
                    projection: pane.projection_generation,
                    listing: pane.listing_generation,
                    settled: pane.listing_settled,
                    folder: pane.folder.clone(),
                    cursor: pane.state.selected(),
                    pending_select: pane.pending_select.clone(),
                    user_navigated: pane.user_navigated,
                }
            }),
            initializing: self.initializing,
            running: self.running,
            context: self.input_context(),
            renaming: self.renaming.clone(),
            new_entry: self.new_entry.clone(),
            goto_prompt: self.goto_prompt.clone(),
            search_query: self.search_query.clone(),
            status: self.status.clone(),
            transfer_dest: self.transfer_dest.clone(),
        }
    }

    /// Handles the tick event of the terminal.
    pub fn tick(&mut self) {
        let before = self.tick_semantics();
        let revision = self.revision;
        let focus = self.focus_generation;
        self.drain_startup();
        self.drain_operation_results();
        self.drain_existing_paths();
        self.hint_offset = self.hint_offset.wrapping_add(2);
        self.drain_jobs();
        self.drain_info_results();
        self.refresh_drives();
        self.pick_up_bounded_listings();
        self.pick_up_pane_listings();
        self.refresh_transfer_destinations();
        self.expire_status();
        self.start_pending_relative_navigation();
        let after = self.tick_semantics();
        if before.context != after.context && self.focus_generation == focus {
            self.focus_generation = self.focus_generation.wrapping_add(1);
        }
        if before != after && self.revision == revision {
            self.semantic_changed();
        }
    }

    /// Raises a transient bottom-bar message (replaces any current one).
    pub fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.observe_existing_path_status(is_error);
        let status = Status {
            text: text.into(),
            is_error,
            raised: self.now(),
        };
        if self.status.as_ref() != Some(&status) {
            self.status = Some(status);
            self.semantic_changed();
        }
    }

    /// Routes pasted text to whichever input dialog is active.
    pub fn handle_paste(&mut self, text: &str) {
        let cleaned = text.strip_suffix('\n').unwrap_or(text);
        let cleaned = cleaned.strip_suffix('\r').unwrap_or(cleaned);
        if cleaned.is_empty() {
            return;
        }
        if self.edit_focus {
            if let Some(document_id) = self.editor_document_id() {
                self.host_requests.push(HostRequest::EditorPaste {
                    document_id,
                    text: cleaned.to_string(),
                });
            }
            return;
        }
        if self.goto_prompt.is_some() {
            self.goto_push(cleaned);
        } else if let Some(p) = self.new_entry.as_mut() {
            for c in cleaned.chars() {
                p.text.insert(p.cursor, c);
                p.cursor += 1;
            }
        } else if let Some(r) = self.renaming.as_mut() {
            for c in cleaned.chars() {
                r.text.insert(r.cursor, c);
                r.cursor += 1;
            }
        } else if self.is_searching() {
            if let Some(q) = &mut self.search_query {
                q.push_str(cleaned);
            }
            self.pane_mut().state.select(Some(0));
        }
    }

    /// Dismisses the error dialog immediately (any key while it is open).
    pub fn clear_status(&mut self) {
        self.observe_existing_path_status(false);
        if self.status.take().is_some() {
            self.semantic_changed();
        }
    }

    /// Drops the status message once its TTL has elapsed.
    fn expire_status(&mut self) {
        if self
            .status
            .as_ref()
            .is_some_and(|s| self.now().saturating_duration_since(s.raised) >= STATUS_TTL)
        {
            self.clear_status();
        }
    }

    /// Spawns a background thread that re-scans attached drives every 2 s and
    /// publishes the result (with a generation bump) through shared state, so
    /// the render thread never blocks on `lsblk`. Works the same way on
    /// Linux, Windows and macOS: `self.cached_drives()` runs on this worker, not on
    /// the render path.
    pub fn quit(&mut self) {
        self.running = false;
    }

    /// Opens the keybindings help dialog (closed by any key).
    pub fn show_keybindings(&mut self) {
        self.cancel_pending_editor();
        self.keybindings_visible = true;
    }

    /// Closes the keybindings help dialog.
    pub fn close_keybindings(&mut self) {
        self.keybindings_visible = false;
    }

    /// Whether a modal or text-input state owns the keyboard right now, so
    /// the contextual hint bar (and its `*` button) would only distract:
    /// rename/goto/new editors, search typing, confirmations, dialogs, the
    /// deletion box, the focused Copy Board and the focused preview text
    /// editor (the buffer captures every key while it has focus).
    pub fn hint_bar_blocked(&self) -> bool {
        self.renaming.is_some()
            || self.goto_prompt.is_some()
            || self.new_entry.is_some()
            || self.confirming.is_some()
            || self.deletion_box_visible()
            || self.multi_info.is_some()
            || self.info.is_some()
            || self.keybindings_visible
            || self.is_searching()
            || self.board_has_focus()
            || self.edit_focus
    }

    /// Key/description pairs for the contextual hint bar, for the state the
    /// user is in. Empty while a transient status notice owns the bar or a
    /// modal/input state blocks it.
    pub fn contextual_hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.hint_bar_blocked() || self.status.is_some() {
            return Vec::new();
        }
        vec![
            ("↑/↓", "move"),
            ("←/→", "open/leave"),
            ("c", "copy"),
            ("m", "move"),
            (".", "hidden"),
            (",", "sort"),
            ("v", "preview"),
            ("b", "bookmark"),
            ("n", "new"),
            ("/", "search"),
        ]
    }

    pub fn set_terminal_size(&mut self, width: u16, height: u16) {
        self.size = (width, height);
    }

    pub fn should_increase_size(&mut self, width: u16, height: u16) -> bool {
        width < 90 || height < 15
    }

    /// The pane that currently receives input.
    fn pane(&self) -> &Pane {
        &self.panes[self.active_pane]
    }

    fn pane_mut(&mut self) -> &mut Pane {
        &mut self.panes[self.active_pane]
    }

    /// Consumes the latest drive list from the background poller, but only
    /// replaces the UI list when the poller reported a change. Never blocks
    /// on `lsblk`; already runs on a worker thread.
    pub fn refresh_drives(&mut self) {
        let gen = *self.drive_generation.lock().unwrap();
        if gen == self.seen_drive_generation {
            return;
        }
        self.seen_drive_generation = gen;
        let drives = self.drive_cache.lock().unwrap().clone();
        if self.drives.as_ref() != Some(&drives) {
            self.drives = Some(drives);
            self.semantic_changed();
        }
    }

    pub fn list_files_from_selected_folder(&mut self) {
        self.list_files_for_pane(self.active_pane);
    }

    /// Refreshes the inactive pane when it is showing `folder`, so content
    /// created from the active pane (new entry, go-to create) appears in
    /// both listings.
    fn refresh_other_pane_if_same_folder(&mut self, folder: &str) {
        let other = 1 - self.active_pane;
        if self.panes[other]
            .folder
            .as_ref()
            .is_some_and(|f| f.path == folder)
        {
            self.list_files_for_pane(other);
        }
    }

    /// Lists `pane_index`'s folder asynchronously (chunked streaming +
    /// sorted final pass) so huge/slow folders never block the UI.
    fn list_files_for_pane(&mut self, pane_index: usize) {
        if self.defer_listings {
            self.deferred_listings.push((pane_index, false));
            return;
        }
        if pane_index >= self.panes.len() {
            return;
        }
        // Any new listing request invalidates in-flight results (a stale
        // async listing must never replace a newer sync/streamed one).
        self.panes[pane_index].listing_generation =
            self.panes[pane_index].listing_generation.wrapping_add(1);
        self.semantic_changed();
        let Some(path) = self.panes[pane_index]
            .folder
            .as_ref()
            .map(|f| f.path.clone())
        else {
            return;
        };
        let generation = self.panes[pane_index].listing_generation;
        let show_hidden = self.show_hidden;
        let tx = self.bounded_tx.clone();
        thread::spawn(move || {
            let result = list_files_bounded(&path, LISTING_CHUNK, show_hidden);
            let _ = tx.send((pane_index, generation, result));
        });
    }

    fn pick_up_bounded_listings(&mut self) {
        while let Ok((pane_index, generation, result)) = self.bounded_rx.try_recv() {
            if self
                .panes
                .get(pane_index)
                .is_none_or(|pane| pane.listing_generation != generation)
            {
                continue;
            }
            if let Ok((mut files, complete)) = result {
                if complete {
                    files.sort_by(|a, b| a.label.cmp(&b.label));
                    let len = files.len();
                    let pane = &mut self.panes[pane_index];
                    if !same_entries(&pane.files, &files)
                        || pane.selected.iter().any(|selected| *selected)
                    {
                        pane.projection_generation = pane.projection_generation.wrapping_add(1);
                    }
                    pane.files = files;
                    pane.selected = vec![false; len];
                    pane.render_scroll = 0;
                    pane.listing_settled = true;
                    // A just-created entry gets the cursor; otherwise the
                    // cursor rests on the first entry (folder-open default).
                    match pane.pending_select.take() {
                        Some(sel) => {
                            if let Some(i) = pane.files.iter().position(|f| f.path == sel) {
                                pane.state.select(Some(i));
                            } else {
                                pane.state.select(Some(0));
                            }
                        }
                        None => {
                            if pane.files.is_empty() {
                                pane.state.select(None);
                            } else {
                                pane.state.select(Some(0));
                            }
                        }
                    }
                    // Same-folder refresh: keep the confirmed filter applied.
                    let query = pane.filter_query.clone();
                    if let Some(q) = query {
                        let labels: Vec<String> =
                            pane.files.iter().map(|f| f.label.clone()).collect();
                        let indices = fuzzy_indices(&labels, &q);
                        if pane.filter_indices != indices {
                            pane.projection_generation = pane.projection_generation.wrapping_add(1);
                        }
                        pane.filter_indices = indices;
                    }
                    continue;
                }
            }
            // Preserve the oracle fallback: a large/failed bounded pass
            // starts a separate generation and clears rows before streaming.
            self.request_pane_listing(pane_index);
        }
    }

    /// `true` when pane `pane_index`'s current listing finished (sorted pass
    /// applied) or errored out. Test/UI hook for the async listing flow.
    pub fn file_list_settled(&self, pane_index: usize) -> bool {
        self.panes
            .get(pane_index)
            .is_some_and(|p| p.listing_settled)
    }

    /// Loads a pane's initial file list on a worker thread; the result is
    /// delivered on `tick()` via [`App::pick_up_pane_listings`]. This keeps
    /// slow directories (cold spin-up HDDs, network shares) off the render
    /// path at startup.
    fn request_pane_listing(&mut self, pane_index: usize) {
        if self.defer_listings {
            self.deferred_listings.push((pane_index, true));
            return;
        }
        if pane_index >= self.panes.len() {
            return;
        }
        let pane = &mut self.panes[pane_index];
        pane.listing_settled = false;
        pane.listing_generation = pane.listing_generation.wrapping_add(1);
        // Clear the rows now: the pane shows "Loading…" until the first
        // chunk of the NEW folder arrives (never stale mixed content).
        pane.projection_generation = pane.projection_generation.wrapping_add(1);
        pane.files.clear();
        pane.selected.clear();
        pane.render_scroll = 0;
        pane.state.select(None);
        let generation = pane.listing_generation;
        let path = pane.folder.as_ref().map(|f| f.path.clone());
        self.semantic_changed();
        let Some(path) = path else {
            return;
        };
        let show_hidden = self.show_hidden;
        let tx = self.file_list_tx.clone();
        thread::spawn(move || {
            // Phase 1: stream chunks as they are read so the first rows
            // appear instantly on huge folders (readdir order, unsorted).
            let mut streamed: Vec<FEntry> = Vec::new();
            let listed = list_files_chunked(&path, LISTING_CHUNK, &mut |mut chunk| {
                if !show_hidden {
                    chunk.retain(|f| !f.label.starts_with('.'));
                }
                if chunk.is_empty() {
                    return;
                }
                streamed.extend(chunk.iter().cloned());
                let _ = tx.send((pane_index, chunk, false, generation));
            });
            if listed.is_err() && streamed.is_empty() {
                let _ = tx.send((pane_index, Vec::new(), true, generation));
                return;
            }
            // Phase 2: authoritative fully-sorted list replaces the stream.
            streamed.sort_by(|a, b| a.label.cmp(&b.label));
            let _ = tx.send((pane_index, streamed, true, generation));
        });
    }

    fn pick_up_pane_listings(&mut self) {
        while let Ok((pane_index, files, done, generation)) = self.file_list_rx.try_recv() {
            if pane_index >= self.panes.len() {
                continue;
            }
            // A newer listing was requested for this pane in the meantime:
            // everything from the stale run is garbage, drop it.
            if self.panes[pane_index].listing_generation != generation {
                continue;
            }
            let pane = &mut self.panes[pane_index];
            if done {
                if !same_entries(&pane.files, &files)
                    || pane.selected.iter().any(|selected| *selected)
                {
                    pane.projection_generation = pane.projection_generation.wrapping_add(1);
                }
                pane.files = files;
                pane.selected = vec![false; pane.files.len()];
                pane.render_scroll = 0;
                pane.grid_top = 0;
                pane.listing_settled = true;
                match pane.pending_select.take() {
                    Some(sel) => {
                        if let Some(i) = pane.files.iter().position(|f| f.path == sel) {
                            pane.state.select(Some(i));
                        } else {
                            pane.state.select(Some(0));
                        }
                    }
                    None => {
                        if pane.files.is_empty() {
                            pane.state.select(None);
                        } else {
                            pane.state.select(Some(0));
                        }
                    }
                }
                let query = pane.filter_query.clone();
                if let Some(q) = query {
                    let labels: Vec<String> = pane.files.iter().map(|f| f.label.clone()).collect();
                    let indices = fuzzy_indices(&labels, &q);
                    if pane.filter_indices != indices {
                        pane.projection_generation = pane.projection_generation.wrapping_add(1);
                    }
                    pane.filter_indices = indices;
                }
            } else {
                pane.listing_settled = false;
                pane.selected
                    .extend(std::iter::repeat_n(false, files.len()));
                if !files.is_empty() {
                    pane.projection_generation = pane.projection_generation.wrapping_add(1);
                }
                pane.files.extend(files);
                // While streaming, follow the pending target as soon as its
                // entry arrives (transfer results land mid-stream). The
                // target is consumed by the final sorted pass.
                if let Some(sel) = &pane.pending_select {
                    if let Some(i) = pane.files.iter().position(|f| &f.path == sel) {
                        pane.state.select(Some(i));
                    }
                }
            }
        }
    }

    /// Installs the terminal image picker probed at startup (before raw
    /// mode); see `main`. Also starts the bounded decode worker pool and a
    /// one-shot disk-cache prune. `truecolor` is forwarded to the braille
    /// fallback so Terminal.app / Windows get xterm-256 instead of 24-bit SGR.
    pub fn cycle_preview(&mut self) {
        let next = match self.pane().preview_mode {
            PreviewMode::Details => PreviewMode::Column,
            PreviewMode::Column => PreviewMode::Grid,
            PreviewMode::Grid => PreviewMode::Off,
            PreviewMode::Off => PreviewMode::Details,
        };
        self.pane_mut().preview_mode = next;
        let label = match next {
            PreviewMode::Off => "off",
            PreviewMode::Column => "column",
            PreviewMode::Grid => "grid",
            PreviewMode::Details => "details",
        };
        self.set_status(format!("Preview: {label}"), false);
    }

    /// Resolves the active pane's selected entry in *visible-row* index
    /// space (live search, confirmed filter, or the full listing).
    /// `state.selected()` is an index into those rows — never `files`.
    pub fn selected_visible_entry(&self) -> Option<&FEntry> {
        self.selected_visible_entry_for(self.active_pane)
    }

    /// Per-pane variant of [`Self::selected_visible_entry`].
    pub fn selected_visible_entry_for(&self, pane_index: usize) -> Option<&FEntry> {
        let pane = &self.panes[pane_index];
        let vis = pane.state.selected()?;
        let index = if self.is_searching() {
            self.search_match_indices().get(vis).copied()
        } else if pane.filter_query.is_some() {
            pane.filter_indices.get(vis).copied()
        } else {
            Some(vis)
        };
        index.and_then(|i| pane.files.get(i))
    }
    pub fn get_drive_shortcuts(&self) -> Vec<char> {
        self.drives
            .as_ref()
            .map(|drives| drives.iter().map(|d| d.shortcut).collect())
            .unwrap_or_default()
    }

    pub fn get_common_folders_shortcuts(&self) -> Vec<char> {
        self.folders
            .as_ref()
            .map(|folders| folders.iter().map(|f| f.shortcut).collect())
            .unwrap_or_default()
    }

    fn set_folder_from_drives_blocking(&mut self, initial_shortcut: usize) {
        let Ok(mut drives) = self.cached_drives() else {
            return;
        };
        let Some(drive) = drives.get(initial_shortcut).cloned() else {
            return;
        };

        // Mount on demand when the selected drive is not already mounted.
        let mount_point = if drive.path.is_empty() {
            let Some(device) = drive.device.as_deref() else {
                return;
            };
            match mount_drive(device) {
                Ok(mp) => {
                    drives[initial_shortcut].path = mp.clone();
                    mp
                }
                Err(err) => {
                    self.set_status(format!("Failed to mount {device}: {err}"), true);
                    return;
                }
            }
        } else {
            drive.path.clone()
        };

        self.drives = Some(drives);
        let pane = self.pane_mut();
        pane.projection_generation = pane.projection_generation.wrapping_add(1);
        pane.filter_query = None;
        pane.filter_indices.clear();
        pane.folder = Some(Folder {
            label: drive.label,
            path: mount_point,
            shortcut: '#',
            device: drive.device,
        });
        self.search_query = None;
        self.list_files_from_selected_folder();
        self.pane_mut().state.select(None);
    }

    /// Ejects (`udisksctl unmount`) the removable drive that contains the
    /// active pane's current folder, then points any pane that lived on that
    /// mount at Home so it never shows a dead listing.
    fn eject_active_drive_blocking(&mut self) {
        let Some(folder_path) = self.pane().folder.as_ref().map(|f| f.path.clone()) else {
            return;
        };
        let Some(drives) = &self.drives else {
            return;
        };
        let Some(drive) = matching_drive(drives, &folder_path) else {
            self.set_status(
                format!("No removable drive is mounted at {folder_path}"),
                true,
            );
            return;
        };
        let Some(device) = drive.device.as_deref() else {
            return;
        };
        let mount_point = drive.path.clone();
        if let Err(err) = eject_drive(device) {
            // udisksctl errors like "GDBus.Error...target is busy" are the
            // common case; trim the bus prefix for a human-readable line.
            let reason = err.to_string();
            let reason = reason
                .rsplit("GDBus.Error:")
                .next()
                .unwrap_or(&reason)
                .trim();
            self.set_status(format!("Failed to eject {device}: {reason}"), true);
            return;
        }

        // The mount point is gone; move every pane that lived there to Home
        // (the drive bar updates itself within one poller tick).
        let home = dirs_next::home_dir().map(|p| p.to_string_lossy().into_owned());
        for pane in self.panes.iter_mut() {
            let stale = pane
                .folder
                .as_ref()
                .is_some_and(|f| f.path.starts_with(&mount_point));
            if stale {
                pane.folder = home
                    .as_deref()
                    .map(|h| Folder::new("Home".to_string(), h.to_string(), '#'));
                pane.state.select(None);
            }
        }
        self.search_query = None;
        self.list_files_from_selected_folder();
    }

    pub fn set_folder_from_common_folders(&mut self, initial_shortcut: usize) {
        let Some(selected) = self
            .folders
            .as_ref()
            .and_then(|folders| folders.get(initial_shortcut).cloned())
        else {
            return;
        };
        let pane = self.pane_mut();
        pane.projection_generation = pane.projection_generation.wrapping_add(1);
        pane.filter_query = None;
        pane.filter_indices.clear();
        pane.folder = Some(selected);
        self.search_query = None;
        self.list_files_from_selected_folder();
        self.pane_mut().state.select(None);
    }

    pub fn set_folder_from_bookmark(&mut self, index: usize) {
        let Some(bookmark) = self
            .bookmarks
            .as_ref()
            .and_then(|bookmarks| bookmarks.get(index).cloned())
        else {
            return;
        };
        self.pane_mut().folder = Some(bookmark);
        self.search_query = None;
        self.list_files_from_selected_folder();
        self.pane_mut().state.select(None);
    }

    fn enter_folder_blocking(&mut self) {
        let Some(idx) = self.pane().state.selected() else {
            return;
        };
        let Some(path) = self.visible_entry(idx).map(|f| f.path.clone()) else {
            return;
        };

        if let Ok(Some(actual_folder)) = get_directory(&path) {
            let pane = self.pane_mut();
            pane.projection_generation = pane.projection_generation.wrapping_add(1);
            pane.filter_query = None;
            pane.filter_indices.clear();
            pane.pending_select = None;
            pane.folder = Some(actual_folder);
            self.search_query = None;
            self.list_files_from_selected_folder();
            self.pane_mut().state.select(None);
        } else if self.visible_entry(idx).map(|f| f.is_dir) == Some(false) {
            // it's a file, just open it
            self.host_requests
                .push(HostRequest::OpenFile(PathBuf::from(path)));
        }
    }

    fn out_of_folder_blocking(&mut self) {
        let Some(current_path) = self.pane().folder.as_ref().map(|f| f.path.clone()) else {
            return;
        };

        match get_parent_directory(&current_path) {
            Ok(Some(folder)) => {
                let pane = self.pane_mut();
                pane.projection_generation = pane.projection_generation.wrapping_add(1);
                pane.filter_query = None;
                pane.filter_indices.clear();
                // Remember the folder we are leaving: once the parent's
                // listing settles, the cursor lands on it.
                pane.pending_select = Some(current_path);
                pane.folder = Some(folder);
                self.search_query = None;
                self.list_files_from_selected_folder();
            }
            Ok(None) => {}
            Err(_) => {}
        }
    }

    pub fn next_item(&mut self) {
        self.pane_mut().user_navigated = true;
        let count = self.visible_count();
        if count == 0 {
            self.pane_mut().state.select(None);
            return;
        }
        let step = self.scroll_step(1);
        let next = match self.pane().state.selected() {
            Some(i) => Some((i + step).min(count - 1)),
            None => Some(0),
        };
        self.pane_mut().state.select(next);
    }

    pub fn prev_item(&mut self) {
        self.pane_mut().user_navigated = true;
        match self.pane().state.selected() {
            Some(i) if i > 0 => {
                let step = self.scroll_step(-1);
                self.pane_mut().state.select(Some(i.saturating_sub(step)));
            }
            Some(_) => {}
            None => {
                if self.visible_count() > 0 {
                    self.pane_mut().state.select(Some(0));
                }
            }
        }
    }

    /// Switches to the next built-in theme preset (`\`), announces it in
    /// the bottom banner, and persists the choice immediately so a crash
    /// never loses it. `theme.toml` per-key overrides stay applied.
    pub fn cycle_theme(&mut self) {
        self.theme_preset = self.theme_preset.next();
        self.set_status(format!("Switched to {}", self.theme_preset.label()), false);
        self.persist_state();
    }

    /// Cycles the active pane's sort mode (Name → Size → Modified → Kind),
    /// re-sorting its files in place while carrying the multi-select flags
    /// along and keeping the cursor on the same file. With a confirmed
    /// filter active, the filtered view is recomputed over the new order.
    pub fn cycle_sort(&mut self) {
        const SORT_MODES: usize = 4;
        let pane_index = self.active_pane;
        let (mode, selected_path) = {
            // While searching (live query) the visible rows come from
            // `search_matches`; with a confirmed filter they come from
            // `filter_indices`. Either way `state.selected()` is a visible
            // row that must be mapped to a `files` index first.
            let visible_to_file: Option<Vec<usize>> =
                if self.is_searching() && pane_index == self.active_pane {
                    Some(self.search_matches())
                } else if self.panes[pane_index].filter_query.is_some() {
                    Some(self.panes[pane_index].filter_indices.clone())
                } else {
                    None
                };
            let pane = &mut self.panes[pane_index];
            pane.sort_mode = (pane.sort_mode + 1) % SORT_MODES;
            // Note the file under the cursor as a path, so the cursor can be
            // restored once the new order is known.
            let path = pane
                .state
                .selected()
                .and_then(|vis| match &visible_to_file {
                    Some(indices) => indices.get(vis).copied(),
                    None => Some(vis),
                })
                .and_then(|i| pane.files.get(i))
                .map(|f| f.path.clone());
            (pane.sort_mode, path)
        };

        let search_active = self.is_searching() && pane_index == self.active_pane;
        let search_query = if search_active {
            self.search_query.clone().unwrap_or_default()
        } else {
            String::new()
        };

        let pane = &mut self.panes[pane_index];
        // Stable sort of indices, then one permutation applied to both the
        // entries and the parallel `selected` flags.
        let mut order: Vec<usize> = (0..pane.files.len()).collect();
        let files = &pane.files;
        order.sort_by(|&a, &b| {
            let (x, y) = (&files[a], &files[b]);
            match mode {
                1 => {
                    // Size: files largest first; directories (size 0) group
                    // last, alphabetical within each group.
                    match (x.is_dir, y.is_dir) {
                        (true, true) => x.label.cmp(&y.label),
                        (true, false) => std::cmp::Ordering::Greater,
                        (false, true) => std::cmp::Ordering::Less,
                        (false, false) => y.size.cmp(&x.size).then(x.label.cmp(&y.label)),
                    }
                }
                2 => {
                    // Modified: newest first; unknown timestamps last.
                    match (&x.modified, &y.modified) {
                        (Some(a), Some(b)) => b.cmp(a).then(x.label.cmp(&y.label)),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => x.label.cmp(&y.label),
                    }
                }
                3 => {
                    // Kind: directories first, then files, alphabetical.
                    y.is_dir.cmp(&x.is_dir).then(x.label.cmp(&y.label))
                }
                _ => x.label.cmp(&y.label),
            }
        });
        pane.projection_generation = pane.projection_generation.wrapping_add(1);
        pane.files = order.iter().map(|&i| files[i].clone()).collect();
        pane.selected = order
            .iter()
            .map(|&i| pane.selected.get(i).copied().unwrap_or(false))
            .collect();

        // Recompute the confirmed filter over the new label order so the
        // filtered rows keep pointing at the same files.
        let filter_query = pane.filter_query.clone();
        if let Some(q) = &filter_query {
            let labels: Vec<String> = pane.files.iter().map(|f| f.label.clone()).collect();
            pane.filter_indices = fuzzy_indices(&labels, q);
        }

        // Keep the cursor on the same file at its new position. Under a
        // filter or live search the cursor is a visible row, so map the
        // file's new index back through the (recomputed) visible list.
        let new_file_index = selected_path
            .as_deref()
            .and_then(|p| pane.files.iter().position(|f| f.path == p));
        let cursor = match new_file_index {
            Some(fi) if filter_query.is_some() => pane.filter_indices.iter().position(|&i| i == fi),
            Some(fi) if search_active => {
                let labels: Vec<String> = pane.files.iter().map(|f| f.label.clone()).collect();
                fuzzy_indices(&labels, &search_query)
                    .iter()
                    .position(|&i| i == fi)
            }
            other => other,
        };
        pane.render_scroll = 0;
        pane.state.select(cursor);

        let notice = match mode {
            1 => "Sorted by size (largest first)",
            2 => "Sorted by last modified (newest first)",
            3 => "Sorted by kind",
            _ => "Sorted by name",
        };
        self.set_status(notice, false);
    }

    /// Clears the held-key ramp (jump navigation starts from step 1).
    fn reset_scroll_ramp(&mut self) {
        self.scroll_dir = 0;
        self.scroll_repeat = 0;
        self.last_scroll = None;
    }

    /// Rows to move for this navigation key: 1 for a fresh press, ramping
    /// to [`SCROLL_MAX_STEP`] while the key is held (repeats within
    /// [`SCROLL_REPEAT_WINDOW`]). Direction change or pause resets the ramp.
    fn scroll_step(&mut self, dir: i8) -> usize {
        let now = self.now();
        let held = self.scroll_dir == dir
            && self
                .last_scroll
                .is_some_and(|t| now.duration_since(t) <= SCROLL_REPEAT_WINDOW);
        self.scroll_dir = dir;
        self.last_scroll = Some(now);
        self.scroll_repeat = if held { self.scroll_repeat + 1 } else { 0 };
        (1 + (self.scroll_repeat / SCROLL_RAMP_EVERY) as usize).min(SCROLL_MAX_STEP)
    }

    pub fn goto_top(&mut self) {
        self.reset_scroll_ramp();
        self.pane_mut().user_navigated = true;
        if self.visible_count() > 0 {
            self.pane_mut().state.select(Some(0));
        } else {
            self.pane_mut().state.select(None);
        }
    }

    pub fn goto_bottom(&mut self) {
        self.reset_scroll_ramp();
        self.pane_mut().user_navigated = true;
        let count = self.visible_count();
        if count > 0 {
            self.pane_mut().state.select(Some(count - 1));
        } else {
            self.pane_mut().state.select(None);
        }
    }

    /// Toggles the vertical split of the files area.
    pub fn toggle_split(&mut self) {
        if self.split {
            self.split = false;
            self.active_pane = 0;
        } else {
            self.split = true;
            self.active_pane = 0;
            // A freshly opened panel always mirrors the open pane's path:
            // the pane's stale location (a previous split's browsing or the
            // saved session's `right` folder) is deliberately not resumed.
            let settled = self.panes[0].listing_settled;
            let (src_side, dst_side) = self.panes.split_at_mut(1);
            let src = &src_side[0];
            let dst = &mut dst_side[0];
            dst.folder = src.folder.clone();
            dst.sort_mode = src.sort_mode;
            dst.state.select(None);
            dst.render_scroll = 0;
            dst.grid_top = 0;
            dst.pending_select = None;
            dst.user_navigated = false;
            if settled {
                // Mirror the settled view verbatim — no extra disk walk.
                dst.projection_generation = dst.projection_generation.wrapping_add(1);
                dst.files = src.files.clone();
                dst.selected = vec![false; dst.files.len()];
                dst.filter_query = src.filter_query.clone();
                dst.filter_indices = src.filter_indices.clone();
                dst.listing_settled = true;
            }
            if !settled {
                // Source still streaming: clone would freeze pane 1 on a
                // prefix. Give it its own listing of the mirrored path
                // ("Loading…" until its own generation arrives).
                self.request_pane_listing(1);
            }
        }
    }

    /// Switches focus among: pane 0 -> pane 1 (when split) -> Copy Board
    /// (when open) -> pane 0.
    pub fn switch_pane(&mut self) {
        self.search_query = None;
        // Tab reaches the preview editor when the focused pane shows an
        // editable text file in column mode. Tab WHILE editing discards
        // unsaved changes and falls through to the normal focus cycle —
        // it must not re-offer the editor it just closed (same selection,
        // same pane: that would trap the user in the editor forever).
        let was_editing = self.edit_focus;
        if was_editing {
            self.edit_focus = false;
            self.close_edit();
        }
        if !was_editing && !self.board_focused && self.active_pane_offers_edit() {
            self.open_edit();
            if self.edit.is_some() || self.pending_editor.is_some() {
                self.edit_focus = true;
                return;
            }
        }
        self.advance_pane_focus();
    }
    fn advance_pane_focus(&mut self) {
        if self.copy_board {
            if self.board_focused {
                self.board_focused = false;
                self.active_pane = 0;
            } else if self.split && self.active_pane == 0 {
                self.active_pane = 1;
            } else {
                self.board_focused = true;
            }
        } else if self.split {
            self.active_pane = 1 - self.active_pane;
        }
    }

    /// Spawns the user's terminal emulator in the active pane's folder
    /// without suspending ira.
    pub fn spawn_native_terminal(&mut self) {
        match self.pane().folder.as_ref() {
            Some(folder) => self
                .host_requests
                .push(HostRequest::Terminal(PathBuf::from(&folder.path))),
            None => self.set_status("No folder open to start a terminal in.", true),
        }
    }

    /// `(target, is_dir, cwd)` for the file-browser reveal (`-`): the cursor
    /// entry when the pane has one — the platform reveals *that* — otherwise
    /// the pane's own folder.
    fn reveal_target(&self) -> Option<(String, bool, String)> {
        let folder = self.pane().folder.as_ref()?.path.clone();
        match self.selected_visible_entry() {
            Some(entry) if !entry.path.is_empty() => {
                Some((entry.path.clone(), entry.is_dir, folder))
            }
            _ => Some((folder.clone(), true, folder)),
        }
    }

    /// Reveals the active pane's selection in the OS file browser (`-`).
    ///
    /// Finder and File Explorer select the item in its folder; on Linux the
    /// item is opened when it is a directory and its containing folder
    /// otherwise, because no file manager exposes a portable "select this
    /// file" request. Best-effort like `0` — the window opening is the
    /// feedback — and a total failure reports what it tried.
    pub fn open_in_file_manager(&mut self) {
        if let Some((target, is_dir, cwd)) = self.reveal_target() {
            self.host_requests.push(HostRequest::Reveal {
                target: PathBuf::from(target),
                is_dir,
                cwd: PathBuf::from(cwd),
            });
        } else {
            self.set_status("No folder open to reveal.", true);
        }
    }

    // ---- Copy / move between panes (async, via the Copy Board) ----

    pub fn request_copy(&mut self) {
        self.request_transfer(JobKind::Copy);
    }

    pub fn request_move(&mut self) {
        self.request_transfer(JobKind::Move);
    }

    /// Pre-validates and stages a copy/move confirmation dialog: pressing
    /// `c`/`m` never starts a transfer by itself.
    fn request_transfer(&mut self, kind: JobKind) {
        let other = 1 - self.active_pane;
        let Some(dest) = self.panes[other].folder.as_ref().map(|f| f.path.clone()) else {
            self.set_status("The other pane has no folder to copy into.", true);
            return;
        };
        let sources = self.collect_sources();
        if sources.is_empty() {
            return;
        }
        let dest_path = std::path::Path::new(&dest);
        if sources
            .iter()
            .any(|src| dest_path.starts_with(std::path::Path::new(src)))
        {
            self.set_status("Cannot copy/move a folder into itself.", true);
            return;
        }
        let label = if sources.len() == 1 {
            let name = std::path::Path::new(&sources[0])
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| sources[0].clone());
            format!("'{name}'")
        } else {
            format!("{} items", sources.len())
        };
        self.confirming = Some(Confirm {
            action: match kind {
                JobKind::Copy => ConfirmAction::Copy,
                JobKind::Move => ConfirmAction::Move,
            },
            policy: OverwritePolicy::AutoRename,
            label,
            paths: sources,
            dest_dir: Some(dest),
        });
    }

    /// Applies the pending copy/move confirmation: spawns the jobs.
    pub fn confirm_transfer(&mut self, kind: JobKind) {
        let Some(confirm) = self.confirming.take() else {
            return;
        };
        let Some(dest) = confirm.dest_dir else {
            return;
        };
        self.spawn_transfer_jobs(kind, confirm.paths, dest, confirm.policy);
    }

    /// Cycles the confirmation dialog's overwrite policy:
    /// auto-rename -> overwrite -> skip existing -> auto-rename.
    pub fn cycle_confirm_policy(&mut self) {
        if let Some(confirm) = self.confirming.as_mut() {
            confirm.policy = match confirm.policy {
                OverwritePolicy::AutoRename => OverwritePolicy::Overwrite,
                OverwritePolicy::Overwrite => OverwritePolicy::SkipExisting,
                OverwritePolicy::SkipExisting => OverwritePolicy::AutoRename,
            };
        }
    }

    // ---- Go to path (`[`) ----

    /// Opens the go-to-path dialog: paste (Ctrl+V) or type a path.
    /// Existing paths are navigated to; missing ones are created (nested).
    pub fn start_goto(&mut self) {
        // One input dialog at a time.
        self.new_entry = None;
        self.renaming = None;
        self.search_query = None;
        self.goto_prompt = Some(String::new());
    }

    pub fn goto_push(&mut self, text: &str) {
        if let Some(p) = self.goto_prompt.as_mut() {
            p.push_str(text);
        }
    }

    pub fn goto_pop(&mut self) {
        if let Some(p) = self.goto_prompt.as_mut() {
            p.pop();
        }
    }

    pub fn cancel_goto(&mut self) {
        self.goto_prompt = None;
    }

    /// Resolves the goto prompt: navigate to existing paths (files select
    /// their containing folder + the file), create missing ones (nested
    /// dirs, kind by extension), then navigate there.
    fn confirm_goto_blocking(&mut self) {
        let Some(raw) = self.goto_prompt.take() else {
            return;
        };
        let path = expand_path(&raw, self.pane().folder.as_ref().map(|f| f.path.clone()));
        if path.as_os_str().is_empty() {
            return;
        }
        let is_file = path_is_file_kind(&path);

        if path.exists() {
            self.goto_navigate(&path);
            return;
        }

        // Missing: create the whole chain. The parent must exist or be
        // creatable; the final entry kind follows the extension rule.
        if let Some(parent) = path.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                self.set_status(
                    format!("Failed to create '{}': {err}", parent.display()),
                    true,
                );
                return;
            }
        }
        let result = if is_file {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map(|_| ())
        } else {
            std::fs::create_dir_all(&path)
        };
        match result {
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                // The entry appeared between the existence check and the
                // create: never overwrite it — navigate to it instead.
                self.goto_navigate(&path);
                return;
            }
            Err(err) => {
                self.set_status(
                    format!("Failed to create '{}': {err}", path.display()),
                    true,
                );
                return;
            }
            Ok(()) => {}
        }
        // The parent folder just gained content: keep the other pane in
        // sync when it is showing the same folder.
        if let Some(created_in) = path.parent() {
            self.refresh_other_pane_if_same_folder(&created_in.to_string_lossy());
        }
        self.goto_navigate(&path);
    }

    /// Navigates a pane to `path`: a folder becomes the pane folder; a file
    /// opens its parent folder with the file selected after the refresh.
    fn goto_navigate(&mut self, path: &std::path::Path) {
        let meta = std::fs::symlink_metadata(path);
        let is_file = meta.as_ref().map(|m| !m.is_dir()).unwrap_or(false);
        let target_folder = if is_file {
            path.parent().map(|p| p.to_path_buf())
        } else {
            Some(path.to_path_buf())
        };
        let Some(folder) = target_folder else {
            return;
        };
        {
            let pane = self.pane_mut();
            pane.projection_generation = pane.projection_generation.wrapping_add(1);
            pane.filter_query = None;
            pane.filter_indices.clear();
            pane.folder = Some(Folder::new(
                folder
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| folder.to_string_lossy().into_owned()),
                folder.to_string_lossy().into_owned(),
                '#',
            ));
            if is_file {
                pane.pending_select = Some(path.to_string_lossy().into_owned());
            } else {
                pane.state.select(None);
            }
        }
        self.search_query = None;
        self.list_files_from_selected_folder();
    }

    /// Copies the active pane's current folder path to the clipboard (`]`).
    pub fn copy_folder_path(&mut self) {
        if let Some(folder) = &self.pane().folder {
            self.host_requests
                .push(HostRequest::CopyText(folder.path.clone()));
        }
    }

    /// Single entry point for whichever confirmation is pending (`y`/Enter).
    /// Opens the create dialog for the active pane's folder. The entry kind
    /// is decided by the typed name: an extension makes it a file.
    pub fn start_new_entry(&mut self) {
        if self.confirming.is_some() || self.info.is_some() || self.renaming.is_some() {
            return;
        }
        if self.pane().folder.is_none() {
            return;
        }
        self.new_entry = Some(NewEntryPrompt {
            text: Vec::new(),
            cursor: 0,
        });
    }

    pub fn new_entry_insert(&mut self, c: char) {
        if let Some(p) = self.new_entry.as_mut() {
            p.text.insert(p.cursor, c);
            p.cursor += 1;
        }
    }

    pub fn new_entry_backspace(&mut self) {
        if let Some(p) = self.new_entry.as_mut() {
            if p.cursor > 0 {
                p.cursor -= 1;
                p.text.remove(p.cursor);
            }
        }
    }

    pub fn new_entry_left(&mut self) {
        if let Some(p) = self.new_entry.as_mut() {
            p.cursor = p.cursor.saturating_sub(1);
        }
    }

    pub fn new_entry_right(&mut self) {
        if let Some(p) = self.new_entry.as_mut() {
            p.cursor = (p.cursor + 1).min(p.text.len());
        }
    }

    pub fn cancel_new_entry(&mut self) {
        self.new_entry = None;
    }

    /// Creates the entry and refreshes the listing, selecting it. Kind rule:
    /// a name whose last dot is not leading and has a non-empty suffix is a
    /// file ("notes.txt", "data.v2.json"); otherwise a folder ("notes",
    /// ".config", "backup.").
    fn confirm_new_entry_blocking(&mut self) {
        // Validation failures keep the dialog open so the name can be fixed.
        // Nested paths are supported: missing parent folders are created.
        let (name, is_file) = {
            let Some(prompt) = self.new_entry.as_ref() else {
                return;
            };
            let raw: String = prompt.text.iter().collect();
            let name = raw.trim().to_string();
            if name.is_empty() {
                self.set_status("Enter a name first.", true);
                return;
            }
            let is_file = path_is_file_kind(std::path::Path::new(&name));
            (name, is_file)
        };

        let Some(parent) = self.pane().folder.as_ref().map(|f| f.path.clone()) else {
            return;
        };
        let target = expand_path(&name, Some(parent.clone()));
        if target.exists() {
            self.set_status(format!("'{name}' already exists."), true);
            return;
        }
        // Nested path: create any missing parent folders first.
        if let Some(parent_dirs) = target.parent() {
            if let Err(err) = std::fs::create_dir_all(parent_dirs) {
                self.set_status(
                    format!("Failed to create '{}': {err}", parent_dirs.display()),
                    true,
                );
                return;
            }
        }
        let result = if is_file {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map(|_| ())
        } else {
            std::fs::create_dir_all(&target)
        };
        if let Err(err) = result {
            // create_new/create_dir_all never touch an existing entry, so
            // an AlreadyExists failure here is a race with the existence
            // check above — report it instead of clobbering anything.
            let status = if err.kind() == std::io::ErrorKind::AlreadyExists {
                if is_file {
                    format!("'{name}' already exists.")
                } else {
                    format!("'{name}' already exists and is not a folder.")
                }
            } else {
                format!("Failed to create '{name}': {err}")
            };
            self.set_status(status, true);
            return;
        }
        self.new_entry = None;
        if name.contains('/') {
            // Nested path: open the deepest created folder (or the file's
            // parent) so the user lands next to what they created.
            self.goto_navigate(&target);
            // The parent folder just gained content: keep the other pane in
            // sync when it is showing the same folder.
            if let Some(created_in) = target.parent() {
                self.refresh_other_pane_if_same_folder(&created_in.to_string_lossy());
            }
            return;
        }
        // Simple name: stay in the current folder, select the new entry
        // once the (async) listing refresh settles.
        let pane = self.pane_mut();
        pane.pending_select = Some(target.to_string_lossy().into_owned());
        self.list_files_from_selected_folder();
        self.refresh_other_pane_if_same_folder(&parent);
    }

    /// Single entry point for whichever confirmation is pending (`y`/Enter).
    pub fn confirm_pending(&mut self) {
        let Some(action) = self.confirming.as_ref().map(|c| c.action) else {
            return;
        };
        match action {
            ConfirmAction::Delete => self.confirm_delete(),
            ConfirmAction::Copy => self.confirm_transfer(JobKind::Copy),
            ConfirmAction::Move => self.confirm_transfer(JobKind::Move),
        }
    }

    /// Toggles multi-selection on the entry under the cursor, then moves the
    /// cursor down so consecutive Space presses select a run.
    pub fn toggle_select_current(&mut self) {
        let Some(cursor) = self.pane().state.selected() else {
            return;
        };
        let Some(file_idx) = self.visible_file_index(cursor) else {
            return;
        };
        self.invalidate_active_projection();
        if let Some(slot) = self.pane_mut().selected.get_mut(file_idx) {
            *slot = !*slot;
        }
        self.next_item();
    }

    /// Selects every entry when any is unselected; otherwise clears all.
    /// Operates on the visible set (filtered view when a filter is active).
    /// Bound to Super+A.
    pub fn toggle_select_all(&mut self) {
        let indices = self.visible_indices();
        if indices.is_empty() {
            return;
        }
        self.invalidate_active_projection();
        let any_unselected = indices.iter().any(|&i| !self.pane().selected[i]);
        for &i in &indices {
            self.pane_mut().selected[i] = any_unselected;
        }
    }

    /// Inverts the multi-selection within the visible set.
    /// Bound to Super+I.
    pub fn invert_selection(&mut self) {
        self.invalidate_active_projection();
        for i in self.visible_indices() {
            if let Some(s) = self.pane_mut().selected.get_mut(i) {
                *s = !*s;
            }
        }
    }

    /// Toggles whether hidden entries (dotfiles) are listed. Bound to `.`.
    pub fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.list_files_for_pane(0);
        self.list_files_for_pane(1);
        self.persist_state();
    }

    // ---- Rename (modal text editor) ----

    /// Opens the rename dialog for the entry under the cursor. Bound to Enter.
    pub fn start_rename(&mut self) {
        if self.confirming.is_some() || self.info.is_some() {
            return;
        }
        let Some(vis_idx) = self.pane().state.selected() else {
            return;
        };
        let Some(file_idx) = self.visible_file_index(vis_idx) else {
            return;
        };
        let Some(entry) = self.pane().files.get(file_idx) else {
            return;
        };
        let label = entry.label.clone();
        self.renaming = Some(RenamePrompt {
            index: file_idx,
            original: label.clone(),
            text: label.chars().collect(),
            cursor: label.chars().count(),
        });
    }

    pub fn cancel_rename(&mut self) {
        self.renaming = None;
    }

    /// Applies the edited name (if changed and valid) and closes the dialog.
    fn commit_rename_blocking(&mut self) {
        let Some(prompt) = self.renaming.take() else {
            return;
        };
        let new_name = chars_to_string(&prompt.text);
        if new_name.is_empty() || new_name == prompt.original {
            return; // nothing changed
        }
        let Some(entry) = self.panes[self.active_pane].files.get(prompt.index) else {
            return;
        };
        let src = entry.path.clone();
        let Some(parent) = std::path::Path::new(&src).parent() else {
            return;
        };
        let dst = parent.join(new_name);
        if std::fs::metadata(&dst).is_ok() {
            self.set_status(
                format!("Cannot rename: '{}' already exists.", prompt.original),
                true,
            );
            return;
        }
        match std::fs::rename(&src, &dst) {
            Ok(_) => {}
            Err(e) => self.set_status(format!("Failed to rename: {e}"), true),
        }
        // Drop the old selection; the list was rebuilt.
        self.list_files_for_pane(self.active_pane);
        self.pane_mut().state.select(None);
    }

    pub fn rename_insert(&mut self, c: char) {
        if let Some(p) = &mut self.renaming {
            let pos = p.cursor.min(p.text.len());
            let mut next: Vec<char> = Vec::with_capacity(p.text.len() + 1);
            for (i, ch) in p.text.iter().enumerate() {
                if i == pos {
                    next.push(c);
                }
                next.push(*ch);
            }
            if pos >= p.text.len() {
                next.push(c);
            }
            p.text = next;
            p.cursor = pos + 1;
        }
    }

    pub fn rename_backspace(&mut self) {
        if let Some(p) = &mut self.renaming {
            if p.cursor > 0 {
                p.text.remove(p.cursor - 1);
                p.cursor -= 1;
            }
        }
    }

    pub fn rename_cursor_left(&mut self) {
        if let Some(p) = &mut self.renaming {
            p.cursor = p.cursor.saturating_sub(1);
        }
    }

    /// Opens the metadata dialog for the entry under the cursor. Bound to `?`.
    /// The dialog renders instantly; the worker's `Meta` event (a stat) fills
    /// in Added/Modified, and folder sizes stream in from the background
    /// walk registered in `size_walks` — which keeps running after the
    /// dialog is dismissed until done or cancelled with `x`.
    pub fn rename_cursor_right(&mut self) {
        if let Some(p) = &mut self.renaming {
            p.cursor = (p.cursor + 1).min(p.text.len());
        }
    }

    /// Opens the metadata dialog for the entry under the cursor. Bound to `?`.
    /// The dialog renders instantly; the worker's `Meta` event (a stat) fills
    /// in Added/Modified, and folder sizes stream in from the background
    /// walk registered in `size_walks` — which keeps running after the
    /// dialog is dismissed until done or cancelled with `x`.
    pub fn show_info(&mut self) {
        if self.confirming.is_some() || self.renaming.is_some() {
            return;
        }
        self.multi_info = None;
        // Multi-selection: aggregate info dialog (sizes summed across all
        // selected folders/files).
        let sources = self.collect_sources();
        if sources.len() > 1 {
            self.show_multi_info(sources);
            return;
        }
        let Some(vis_idx) = self.pane().state.selected() else {
            return;
        };
        let Some(entry) = self.visible_entry(vis_idx) else {
            return;
        };
        let entry = entry.clone();
        self.info = Some(InfoDialog {
            lines: build_info_fast(&entry),
            path: entry.path.clone(),
            pending: true,
            started: self.now(),
        });
        if entry.is_dir {
            self.ensure_size_walk(&entry.path);
        }
        // Metadata worker: one stat for Added/Modified (and a file's size).
        let tx = self.info_tx.clone();
        let path = entry.path.clone();
        thread::spawn(move || {
            let lines = build_info_full(&entry, None);
            let _ = tx.send(InfoEvent::Meta { path, lines });
        });
    }

    /// Opens the aggregate info dialog for a multi-selection: spawns size
    /// walks for selected folders and stats the selected files; the dialog
    /// sums everything live from the size cache. Any key dismisses it (the
    /// walks keep running and stay cached).
    fn show_multi_info(&mut self, paths: Vec<String>) {
        let pane = self.pane();
        let mut folders = 0usize;
        let mut files = 0usize;
        let mut dir_paths = Vec::new();
        let mut file_paths = Vec::new();
        for p in &paths {
            let is_dir = pane
                .files
                .iter()
                .find(|f| f.path == *p)
                .map(|f| f.is_dir)
                .unwrap_or(false);
            if is_dir {
                folders += 1;
                dir_paths.push(p.clone());
            } else {
                files += 1;
                file_paths.push(p.clone());
            }
        }
        self.info = None;
        self.multi_info = Some(MultiInfoState {
            paths: paths.clone(),
            folders,
            files,
            started: self.now(),
        });
        for p in dir_paths {
            self.ensure_size_walk(&p);
        }
        // Selected files: one worker stats them (len + allocated size) and
        // reports each as a Done size-cache entry.
        if !file_paths.is_empty() {
            let tx = self.info_tx.clone();
            thread::spawn(move || {
                for p in file_paths {
                    if let Ok(meta) = std::fs::symlink_metadata(&p) {
                        let _ = tx.send(InfoEvent::Done {
                            path: p,
                            size: DirSize {
                                bytes: meta.len(),
                                items: 1,
                                on_disk: on_disk_bytes(&meta),
                            },
                        });
                    }
                }
            });
        }
    }

    /// Aggregate sums for the open multi-selection dialog:
    /// (complete, data bytes, on-disk bytes, items).
    pub fn multi_info_aggregate(&self) -> (bool, u64, u64, u64) {
        let Some(m) = &self.multi_info else {
            return (true, 0, 0, 0);
        };
        let mut complete = true;
        let mut bytes = 0u64;
        let mut items = 0u64;
        let mut on_disk = 0u64;
        for p in &m.paths {
            match self.size_cache.get(p) {
                Some(si) => {
                    bytes += si.bytes;
                    items += si.items;
                    on_disk += si.on_disk;
                    if !si.complete {
                        complete = false;
                    }
                }
                None => complete = false,
            }
        }
        (complete, bytes, items, on_disk)
    }

    /// Ensures a background size walk is running for `path` (one per folder;
    /// skipped when already measured or already running).
    fn ensure_size_walk(&mut self, path: &str) {
        if self.size_cache.get(path).is_some_and(|s| s.complete) {
            return;
        }
        if self.size_walks.contains_key(path) {
            return;
        }
        let handle = WalkHandle::new();
        self.size_walks.insert(
            path.to_string(),
            WalkSlot {
                handle: handle.clone(),
                started: self.now(),
            },
        );
        let tx = self.info_tx.clone();
        let walk_path = path.to_string();
        thread::spawn(move || {
            let mut on_progress = |bytes: u64, items: u64, on_disk: u64| {
                if !handle.cancelled() {
                    let _ = tx.send(InfoEvent::Progress {
                        path: walk_path.clone(),
                        bytes,
                        items,
                        on_disk,
                    });
                }
            };
            let size = dir_size(Path::new(&walk_path), &handle, &mut on_progress);
            if !handle.cancelled() {
                let _ = tx.send(InfoEvent::Done {
                    path: walk_path,
                    size,
                });
            }
        });
    }

    /// Applies queued events: walk progress/done update the size cache (and
    /// the open dialog's Size line), the metadata worker fills the dialog.
    fn drain_info_results(&mut self) {
        while let Ok(event) = self.info_rx.try_recv() {
            let mut changed = false;
            match event {
                InfoEvent::Progress {
                    path,
                    bytes,
                    items,
                    on_disk,
                } => {
                    changed = true;
                    self.size_cache.insert(
                        path.clone(),
                        SizeInfo {
                            bytes,
                            items,
                            on_disk,
                            complete: false,
                            updated: SystemTime::now(),
                        },
                    );
                }
                InfoEvent::Done { path, size } => {
                    changed = true;
                    self.size_walks.remove(&path);
                    self.size_cache.insert(
                        path.clone(),
                        SizeInfo {
                            bytes: size.bytes,
                            items: size.items,
                            on_disk: size.on_disk,
                            complete: true,
                            updated: SystemTime::now(),
                        },
                    );
                    // A completed measurement is worth keeping across
                    // restarts; save immediately so a crash keeps it too.
                    self.persist_state();
                }
                InfoEvent::Meta { path, lines } => {
                    if let Some(dialog) = self.info.as_mut() {
                        if dialog.pending && dialog.path == path {
                            changed = true;
                            dialog.pending = false;
                            dialog.lines = lines;
                        }
                    }
                }
            }
            if changed {
                self.semantic_changed();
            }
        }
        self.sync_dialog_size_line();
    }

    /// Keeps the open dialog's Size line in sync with the cache: inserts the
    /// final line once the walk completes (the renderer draws the animated
    /// partial line while the walk runs or after a cancellation).
    fn sync_dialog_size_line(&mut self) {
        let Some(dialog) = self.info.as_mut() else {
            return;
        };
        if dialog.pending {
            return;
        }
        let Some(si) = self.size_cache.get(&dialog.path) else {
            return;
        };
        if si.complete && !dialog.lines.iter().any(|l| l.starts_with("Size:")) {
            let line = size_line_final(si);
            dialog.lines.insert(4.min(dialog.lines.len()), line);
            self.semantic_changed();
        }
    }

    /// Cancels the background walk for the open dialog's folder (`x`); the
    /// partial measurement stays in the cache.
    pub fn cancel_dialog_size_walk(&mut self) {
        let Some(dialog) = self.info.as_ref() else {
            return;
        };
        if let Some(slot) = self.size_walks.remove(&dialog.path) {
            slot.handle.cancel();
        }
    }

    /// Start time of the walk for `path`, if one is running (drives the
    /// spinner icon in the file list).
    pub fn size_walk_started(&self, path: &str) -> Option<Instant> {
        self.size_walks.get(path).map(|s| s.started)
    }

    /// Cached measurement for `path`, if any (partial or complete).
    pub fn size_info(&self, path: &str) -> Option<&SizeInfo> {
        self.size_cache.get(path)
    }

    pub fn close_info(&mut self) {
        // Deliberately does NOT cancel the walk: measurements continue in
        // the background so sizes are ready whenever the user returns.
        self.info = None;
    }

    /// Full paths to operate on: all multi-selected entries, or the cursor
    /// entry when nothing is selected.
    fn collect_sources(&self) -> Vec<String> {
        let pane = self.pane();
        if pane.selected.iter().any(|&s| s) {
            return pane
                .files
                .iter()
                .enumerate()
                .filter(|(i, _)| pane.selected[*i])
                .map(|(_, f)| f.path.clone())
                .collect();
        }
        let Some(vis_idx) = pane.state.selected() else {
            return Vec::new();
        };
        self.visible_file_index(vis_idx)
            .and_then(|i| pane.files.get(i))
            .map(|f| vec![f.path.clone()])
            .unwrap_or_default()
    }

    fn spawn_transfer_jobs(
        &mut self,
        kind: JobKind,
        sources: Vec<String>,
        dest: String,
        policy: OverwritePolicy,
    ) {
        let dest_path = std::path::Path::new(&dest);
        // One batch job for the whole selection: a single worker thread
        // processes the paths sequentially — never one thread per file.
        let mut paths = Vec::new();
        for src in sources {
            if dest_path.starts_with(std::path::Path::new(&src)) {
                self.set_status("Cannot copy/move a folder into itself.", true);
                continue;
            }
            paths.push(src);
        }
        if paths.is_empty() {
            return;
        }
        let label = if paths.len() == 1 {
            std::path::Path::new(&paths[0])
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "item".to_string())
        } else {
            format!("{} items", paths.len())
        };

        let id = self.next_job_id;
        self.next_job_id += 1;
        let reveal = std::path::Path::new(&dest).join(
            std::path::Path::new(&paths[0])
                .file_name()
                .unwrap_or_default(),
        );
        self.jobs.push(Job {
            id,
            kind,
            overwrite: policy,
            paths,
            dest_dir: dest.clone(),
            label,
            total_bytes: None,
            copied_bytes: 0,
            current: String::new(),
            status: JobStatus::Running,
            started_at: self.now(),
            control: JobControl::new(),
        });
        let job = self.jobs.last().unwrap();
        if let Some(provider) = &self.transfer_provider {
            spawn_job_with_provider(job, self.job_tx.clone(), Arc::clone(provider));
        } else {
            spawn_job(job, self.job_tx.clone());
        }

        self.transfer_generation = self.transfer_generation.wrapping_add(1);
        self.transfer_probe_pending = None;
        self.transfer_probe_last_attempt = None;
        self.transfer_dest = Some(TransferDestSync {
            dest_dir: dest.clone(),
            reveal_path: reveal.to_string_lossy().into_owned(),
            last_refresh: self.now(),
        });

        // The selection is handled; drop it. Show the Copy Board so a large
        // transfer is visibly in progress, but do NOT take the keyboard:
        // focus stays on the source files pane and browsing continues, and
        // Tab reaches the board when you want to pause/cancel (mirroring the
        // preview panel). Select the new job so it is highlighted on arrival.
        self.invalidate_active_projection();
        self.pane_mut().selected.fill(false);
        self.copy_board = true;
        self.copy_board_state.select(Some(self.jobs.len() - 1));
        // A fresh transfer may reveal its incoming item again: clear the
        // "user took the cursor" marks left by earlier browsing.
        for pane in self.panes.iter_mut() {
            pane.user_navigated = false;
        }
    }

    /// Whether keyboard focus is on the Copy Board.
    pub fn board_has_focus(&self) -> bool {
        self.board_focused
    }

    pub fn toggle_copy_board(&mut self) {
        self.copy_board = !self.copy_board;
        if self.copy_board {
            self.board_focused = true;
            self.copy_board_state
                .select(if self.jobs.is_empty() { None } else { Some(0) });
        } else {
            self.board_focused = false;
        }
    }

    pub fn copy_board_prev(&mut self) {
        match self.copy_board_state.selected() {
            Some(i) if i > 0 => self.copy_board_state.select(Some(i - 1)),
            Some(_) => {}
            None => {
                if !self.jobs.is_empty() {
                    self.copy_board_state.select(Some(0));
                }
            }
        }
    }

    pub fn copy_board_next(&mut self) {
        let count = self.jobs.len();
        if count == 0 {
            self.copy_board_state.select(None);
            return;
        }
        match self.copy_board_state.selected() {
            Some(i) => self.copy_board_state.select(Some((i + 1).min(count - 1))),
            None => self.copy_board_state.select(Some(0)),
        }
    }

    pub fn cancel_selected_job(&mut self) {
        if let Some(i) = self.copy_board_state.selected() {
            if let Some(job) = self.jobs.get(i) {
                if matches!(job.status, JobStatus::Running | JobStatus::Paused) {
                    job.control.request_cancel();
                }
            }
        }
    }

    pub fn toggle_selected_job_pause(&mut self) {
        if let Some(i) = self.copy_board_state.selected() {
            if let Some(job) = self.jobs.get_mut(i) {
                match job.status {
                    JobStatus::Running => {
                        job.control.set_paused(true);
                        job.status = JobStatus::Paused;
                    }
                    JobStatus::Paused => {
                        job.control.set_paused(false);
                        job.status = JobStatus::Running;
                    }
                    _ => {}
                }
            }
        }
    }

    // ---- Delete (with confirmation) ----

    /// Starts the delete flow for the selected/cursor entries: shows the
    /// confirmation prompt.
    pub fn request_delete(&mut self) {
        let paths = self.collect_sources();
        if paths.is_empty() {
            return;
        }
        let label = if paths.len() == 1 {
            let name = std::path::Path::new(&paths[0])
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| paths[0].clone());
            format!("'{name}'")
        } else {
            format!("{} items", paths.len())
        };
        self.confirming = Some(Confirm {
            action: ConfirmAction::Delete,
            policy: OverwritePolicy::AutoRename,
            label,
            paths,
            dest_dir: None,
        });
    }

    /// Confirms the pending deletion: spawns the background delete worker
    /// and returns immediately. Progress arrives on the job channel and is
    /// shown in the (dismissable) progress dialog plus file-list spinners.
    pub fn confirm_delete(&mut self) {
        let Some(confirm) = self.confirming.take() else {
            return;
        };
        if confirm.paths.is_empty() {
            return;
        }
        self.deletion_generation = self.deletion_generation.wrapping_add(1);
        self.deleting_paths = confirm.paths.iter().cloned().collect();
        let tx = self.job_tx.clone();
        let control = spawn_delete_job(confirm.paths.clone(), tx);
        self.deletion = Some(DeletionState {
            total: confirm.paths.len(),
            done: 0,
            current: None,
            started: self.now(),
            control,
        });
        self.deletion_box_hidden = false;
    }

    /// `Some(started)` when `path` is queued/being deleted (file-list spinner).
    pub fn deleting_started(&self, path: &str) -> Option<Instant> {
        self.deleting_paths
            .contains(path)
            .then_some(())
            .and(self.deletion.as_ref().map(|d| d.started))
    }

    /// A batch deletion is running and its progress dialog is visible.
    pub fn deletion_box_visible(&self) -> bool {
        self.deletion.is_some() && !self.deletion_box_hidden
    }

    /// Cancels the pending confirmation.
    pub fn cancel_confirm(&mut self) {
        self.confirming = None;
    }

    fn drain_jobs(&mut self) {
        while let Ok(event) = self.job_rx.try_recv() {
            let mut changed = false;
            match event {
                JobEvent::Started { id, total_bytes } => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        changed = j.total_bytes != total_bytes;
                        j.total_bytes = total_bytes;
                    }
                }
                JobEvent::Progress {
                    id,
                    copied_bytes,
                    current,
                } => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        changed = j.copied_bytes != copied_bytes || j.current != current;
                        j.copied_bytes = copied_bytes;
                        j.current = current;
                    }
                }
                JobEvent::Done { id } => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        changed = j.status != JobStatus::Done;
                        j.status = JobStatus::Done;
                        self.transfer_dest = None;
                        // Land the destination pane's cursor on the last
                        // item of the batch so the new content is visible
                        // without hunting for it.
                        if let Some(last) = j.paths.last() {
                            let dest_item = std::path::Path::new(&j.dest_dir)
                                .join(std::path::Path::new(last).file_name().unwrap_or_default());
                            let dest_item = dest_item.to_string_lossy().into_owned();
                            if let Some(pane) = self
                                .panes
                                .iter_mut()
                                .find(|p| p.folder.as_ref().is_some_and(|f| f.path == j.dest_dir))
                            {
                                // Land on the finished item — unless the user
                                // took the cursor over while it streamed, in
                                // which case leave it where they put it.
                                let target = if pane.user_navigated {
                                    pane.state
                                        .selected()
                                        .and_then(|i| pane.files.get(i))
                                        .map(|f| f.path.clone())
                                } else {
                                    Some(dest_item)
                                };
                                if target.is_some() {
                                    pane.pending_select = target;
                                }
                            }
                        }
                    }
                    self.refresh_after_job();
                }
                JobEvent::Cancelled { id } => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        changed = j.status != JobStatus::Cancelled;
                        j.status = JobStatus::Cancelled;
                    }
                    self.refresh_after_job();
                }
                JobEvent::Failed { id, error } => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        changed = j.status != JobStatus::Failed(error.clone());
                        j.status = JobStatus::Failed(error);
                    }
                }
                JobEvent::DeleteProgress {
                    done,
                    total,
                    current,
                } => {
                    if let Some(d) = self.deletion.as_mut() {
                        changed = d.done != done
                            || d.total != total
                            || d.current.as_ref() != Some(&current);
                        d.done = done;
                        d.total = total;
                        d.current = Some(current.clone());
                    }
                    if self.deleting_paths.remove(&current) {
                        self.deletion_generation = self.deletion_generation.wrapping_add(1);
                        changed = true;
                    }
                }
                JobEvent::DeleteDone { cancelled, failed } => {
                    changed = self.deletion.is_some()
                        || !self.deleting_paths.is_empty()
                        || self.deletion_box_hidden;
                    self.deletion = None;
                    self.deletion_generation = self.deletion_generation.wrapping_add(1);
                    self.deleting_paths.clear();
                    self.deletion_box_hidden = false;
                    if !cancelled {
                        self.invalidate_active_projection();
                        self.pane_mut().selected.fill(false);
                        self.refresh_after_job();
                    }
                    if failed.is_empty() {
                        // Success is visible in the listing itself — no
                        // dialog for a normal completion.
                    } else {
                        let (path, err) = &failed[0];
                        let more = if failed.len() > 1 {
                            format!(" (+{} more)", failed.len() - 1)
                        } else {
                            String::new()
                        };
                        self.set_status(format!("Failed to delete '{path}': {err}{more}"), true);
                    }
                }
            }
            if changed {
                self.semantic_changed();
            }
        }
    }

    /// Re-lists both panes so completed/cancelled transfers are reflected.
    fn refresh_after_job(&mut self) {
        self.list_files_for_pane(0);
        self.list_files_for_pane(1);
    }

    /// While a transfer writes into a folder, re-lists panes that show that
    /// folder (or are inside it) once per second, so copied items appear
    /// live. Never blocks the UI: listings run on background workers.
    fn refresh_transfer_destinations(&mut self) {
        let finished = self
            .transfer_probe_finished
            .as_ref()
            .is_some_and(|finished| finished.load(std::sync::atomic::Ordering::Acquire));
        while let Ok((generation, sync, exists)) = self.transfer_probe_rx.try_recv() {
            if generation != self.transfer_generation {
                continue;
            }
            self.transfer_probe_pending = None;
            let current = self.transfer_dest.as_ref().is_some_and(|current| {
                current.dest_dir == sync.dest_dir
                    && current.reveal_path == sync.reveal_path
                    && current.last_refresh == sync.last_refresh
            });
            if exists && current {
                self.apply_transfer_destination_refresh(sync);
            }
        }
        if finished {
            // The worker can unwind or fail to spawn without delivering a
            // result. Such completion must not wedge semantic pending forever.
            self.transfer_probe_finished = None;
            self.transfer_probe_pending = None;
        }
        let Some(sync) = self.transfer_dest.clone() else {
            return;
        };
        if !self
            .jobs
            .iter()
            .any(|j| matches!(j.status, JobStatus::Running | JobStatus::Paused))
        {
            self.transfer_dest = None;
            return;
        }
        let now = self.now();
        if self.transfer_probe_pending.is_some()
            || now.saturating_duration_since(sync.last_refresh) < Duration::from_secs(1)
            || self
                .transfer_probe_last_attempt
                .is_some_and(|last| now.saturating_duration_since(last) < Duration::from_secs(1))
        {
            return;
        }
        if TRANSFER_PROBE_BUSY
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_err()
        {
            // Do not consume the attempt or pending slot on denied admission.
            return;
        }
        let completed = Arc::new(AtomicBool::new(false));
        let occupancy = TransferProbeOccupancy(Arc::clone(&completed));
        self.transfer_probe_finished = Some(completed);
        let generation = self.transfer_generation;
        self.transfer_probe_pending = Some(generation);
        self.transfer_probe_last_attempt = Some(now);
        let tx = self.transfer_probe_tx.clone();
        #[cfg(test)]
        let probe = self.transfer_probe_test.clone();
        let worker = move || {
            let _occupancy = occupancy;
            #[cfg(test)]
            let exists = probe.as_ref().map_or_else(
                || std::fs::metadata(&sync.dest_dir).is_ok(),
                |probe| probe(&sync.dest_dir),
            );
            #[cfg(not(test))]
            let exists = std::fs::metadata(&sync.dest_dir).is_ok();
            let _ = tx.send((generation, sync, exists));
        };
        #[cfg(test)]
        let spawn_result = if self.transfer_probe_spawn_error {
            drop(worker);
            Err(std::io::Error::other(
                "fixture metadata worker spawn failure",
            ))
        } else {
            thread::Builder::new().spawn(worker)
        };
        #[cfg(not(test))]
        let spawn_result = thread::Builder::new().spawn(worker);
        if spawn_result.is_err() {
            self.transfer_probe_pending = None;
        }
    }
    fn apply_transfer_destination_refresh(&mut self, sync: TransferDestSync) {
        for i in 0..self.panes.len() {
            let viewing = self.panes[i].folder.as_ref().is_some_and(|f| {
                f.path == sync.dest_dir || f.path.starts_with(&format!("{}/", sync.dest_dir))
            });
            if !viewing {
                continue;
            }
            // Once the user has taken the cursor over in this pane, leave it
            // entirely alone: a periodic re-list would reset the scroll
            // window and re-select the position captured when the refresh
            // started — a stale target, so the cursor appears to jump by
            // however far the user moved in the meantime. The transfer's
            // progress stays visible in the Copy Board; the pane catches up
            // in one clean re-list when the job finishes.
            if self.panes[i].user_navigated {
                continue;
            }
            let pane = &mut self.panes[i];
            if pane
                .folder
                .as_ref()
                .is_some_and(|f| f.path == sync.dest_dir)
            {
                // Reveal mode: keep the cursor on the incoming item.
                pane.pending_select = Some(sync.reveal_path.clone());
            } else if let Some(cur) = pane.state.selected().and_then(|vi| pane.files.get(vi)) {
                // Inside the incoming folder: preserve the cursor position.
                pane.pending_select = Some(cur.path.clone());
            }
            self.list_files_for_pane(i);
        }
        let now = self.now();
        if let Some(s) = &mut self.transfer_dest {
            s.last_refresh = now;
        }
    }

    /// Whether fuzzy search within the current folder is active.
    pub fn is_searching(&self) -> bool {
        self.search_query.is_some()
    }

    /// Indices into the active pane's files matching the current query, best match first.
    fn search_matches(&self) -> Vec<usize> {
        self.search_match_indices().as_ref().clone()
    }

    fn search_match_indices(&self) -> Arc<Vec<usize>> {
        let pane = self.pane();
        let query = self.search_query.as_deref().unwrap_or("");
        let mut cache = self.search_projection.borrow_mut();
        if cache.as_ref().is_none_or(|cached| {
            cached.pane != self.active_pane
                || cached.generation != pane.projection_generation
                || cached.files != pane.files.len()
                || cached.query != query
        }) {
            let labels: Vec<String> = pane.files.iter().map(|f| f.label.clone()).collect();
            *cache = Some(SearchProjection {
                pane: self.active_pane,
                generation: pane.projection_generation,
                files: pane.files.len(),
                query: query.to_owned(),
                indices: Arc::new(fuzzy_indices(&labels, query)),
            });
        }
        Arc::clone(&cache.as_ref().expect("search initialized").indices)
    }

    /// Clears the pane's confirmed filter, keeping the cursor on the entry
    /// that was selected in the filtered view.
    pub fn clear_filter(&mut self) {
        let file_idx = self
            .pane()
            .state
            .selected()
            .and_then(|vis| self.pane().filter_indices.get(vis).copied());
        let pane = self.pane_mut();
        pane.projection_generation = pane.projection_generation.wrapping_add(1);
        pane.filter_query = None;
        pane.filter_indices.clear();
        match file_idx {
            Some(i) => pane.state.select(Some(i)),
            None => {
                if pane.files.is_empty() {
                    pane.state.select(None);
                } else {
                    pane.state.select(Some(0));
                }
            }
        }
    }

    /// The currently visible entries of the active pane (filtered by search),
    /// as `(file_index, entry)` pairs so callers can map rows back to the
    /// source file list (e.g. for multi-select).
    pub fn visible_rows(&self) -> Vec<(usize, &FEntry)> {
        self.pane_visible_rows(self.active_pane)
    }

    /// Visible rows of any pane (live search while typing, confirmed filter,
    /// or the full listing).
    pub fn pane_visible_rows(&self, pane_index: usize) -> Vec<(usize, &FEntry)> {
        let pane = &self.panes[pane_index];
        let indices: Vec<usize> = if self.is_searching() && pane_index == self.active_pane {
            self.search_matches()
        } else if let Some(_q) = &pane.filter_query {
            pane.filter_indices.clone()
        } else {
            (0..pane.files.len()).collect()
        };
        indices
            .into_iter()
            .filter_map(|i| pane.files.get(i).map(|f| (i, f)))
            .collect()
    }

    /// Indices into `files` of the active pane's visible rows.
    fn visible_indices(&self) -> Vec<usize> {
        if self.is_searching() {
            self.search_matches()
        } else if let Some(_q) = &self.pane().filter_query {
            self.pane().filter_indices.clone()
        } else {
            (0..self.pane().files.len()).collect()
        }
    }

    /// Maps a visible-list index to the underlying `files` index.
    fn visible_file_index(&self, visible_idx: usize) -> Option<usize> {
        if self.is_searching() {
            self.search_match_indices().get(visible_idx).copied()
        } else if self.pane().filter_query.is_some() {
            self.pane().filter_indices.get(visible_idx).copied()
        } else {
            (visible_idx < self.pane().files.len()).then_some(visible_idx)
        }
    }

    pub fn visible_count(&self) -> usize {
        if self.is_searching() {
            self.search_match_indices().len()
        } else if self.pane().filter_query.is_some() {
            self.pane().filter_indices.len()
        } else {
            self.pane().files.len()
        }
    }

    /// Maps a visible-list index to the underlying file entry.
    fn visible_entry(&self, visible_idx: usize) -> Option<&FEntry> {
        let files = &self.pane().files;
        self.visible_file_index(visible_idx)
            .and_then(|i| files.get(i))
    }

    pub fn start_search(&mut self) {
        self.search_query = Some(String::new());
        if self.visible_count() > 0 {
            self.pane_mut().state.select(Some(0));
        }
    }

    pub fn cancel_search(&mut self) {
        self.search_query = None;
        if self.pane().files.is_empty() {
            self.pane_mut().state.select(None);
        } else {
            self.pane_mut().state.select(Some(0));
        }
    }

    pub fn confirm_search(&mut self) {
        // Promote the query to a sticky filter: the pane keeps showing only
        // the matches, and every action operates on that view. Empty query
        // just exits typing (shows everything).
        let query = self.search_query.take();
        let pane = self.pane_mut();
        match query {
            Some(q) if !q.trim().is_empty() => {
                let labels: Vec<String> = pane.files.iter().map(|f| f.label.clone()).collect();
                let indices = fuzzy_indices(&labels, &q);
                if pane.filter_indices != indices {
                    pane.projection_generation = pane.projection_generation.wrapping_add(1);
                }
                pane.filter_indices = indices;
                pane.projection_generation = pane.projection_generation.wrapping_add(1);
                pane.filter_query = Some(q);
            }
            _ => {
                pane.projection_generation = pane.projection_generation.wrapping_add(1);
                pane.filter_query = None;
                pane.filter_indices.clear();
            }
        }
    }

    pub fn push_search_char(&mut self, c: char) {
        if self.search_query.is_none() {
            self.search_query = Some(String::new());
        }
        if let Some(q) = &mut self.search_query {
            q.push(c);
        }
        self.pane_mut().state.select(Some(0));
    }

    pub fn pop_search_char(&mut self) {
        if let Some(q) = &mut self.search_query {
            q.pop();
        }
        self.pane_mut().state.select(Some(0));
    }

    /// Loads persisted bookmarks, assigning shortcuts in keyboard order.
    fn apply_bookmark_pairs(&mut self, pairs: Vec<(String, String)>) {
        let mut loaded: Vec<Folder> = Vec::new();
        for (label, path) in pairs {
            let Some(shortcut) = next_free_shortcut(&loaded) else {
                break;
            };
            loaded.push(Folder::new(label, path, shortcut));
        }
        self.bookmarks = Some(loaded);
    }

    fn persist_bookmarks(&self) {
        if let Some(bookmarks) = &self.bookmarks {
            let _ = self.persistence_tx.send(PersistenceRequest::Bookmarks(
                self.bookmarks_path
                    .clone()
                    .or_else(crate::services::bookmarks::bookmarks_file),
                bookmarks.clone(),
            ));
        }
    }

    pub fn get_bookmark_shortcuts(&self) -> Vec<char> {
        self.bookmarks
            .as_ref()
            .map(|b| b.iter().map(|f| f.shortcut).collect())
            .unwrap_or_default()
    }

    /// Toggles a bookmark for the current folder: adds it (assigning the next
    /// available letter) if not bookmarked, or removes it if already present.
    pub fn toggle_bookmark(&mut self) {
        let Some((label, path)) = self
            .pane()
            .folder
            .as_ref()
            .map(|f| (f.label.clone(), f.path.clone()))
        else {
            return;
        };

        if let Some(bookmarks) = &mut self.bookmarks {
            if let Some(pos) = bookmarks.iter().position(|b| b.path == path) {
                bookmarks.remove(pos);
            } else {
                let Some(shortcut) = next_free_shortcut(bookmarks) else {
                    self.set_status("No free bookmark shortcut available (a-p are taken)", true);
                    return;
                };
                bookmarks.push(Folder::new(label, path, shortcut));
            }
        }
        self.persist_bookmarks();
    }

    /// Restores the persisted session state (split layout and pane folders).
    /// Returns the persisted theme preset id, if any, so theme loading can
    /// reuse it when `theme.toml` does not pin one. The icon set is not
    /// persisted: it is re-detected from the available fonts every launch.
    pub fn restore_state(&mut self) -> Option<String> {
        let state = match &self.state_path {
            Some(p) => load_state_from(p),
            None => load_state(),
        };
        self.apply_session_state(state)
    }
    fn apply_session_state(&mut self, state: SessionState) -> Option<String> {
        let prefs = state.theme.clone();
        self.split = state.split;
        self.active_pane = state.active_pane.min(1);
        self.show_hidden = state.show_hidden;
        if let Some(left) = state.left {
            self.panes[0].folder = Some(left);
        }
        if let Some(right) = state.right {
            self.panes[1].folder = Some(right);
        }
        // Preview modes are per pane and survive restarts.
        self.panes[0].preview_mode = PreviewMode::from_u8(state.preview[0]);
        self.panes[1].preview_mode = PreviewMode::from_u8(state.preview[1]);
        // Folder sizes measured in earlier sessions reappear instantly;
        // re-querying one of these folders skips the walk.
        self.restore_sizes(state.sizes);
        // File lists are populated asynchronously from `App::new` via
        // `request_pane_listing`, so a slow drive doesn't block startup.
        prefs
    }

    /// Restores persisted size entries into the cache (epoch -> `SystemTime`).
    fn restore_sizes(&mut self, entries: Vec<SizeEntry>) {
        for e in entries {
            self.size_cache.insert(
                e.path.clone(),
                SizeInfo {
                    bytes: e.bytes,
                    items: e.items,
                    on_disk: e.on_disk,
                    complete: e.complete,
                    updated: SystemTime::UNIX_EPOCH + Duration::from_secs(e.updated_epoch),
                },
            );
        }
    }

    /// Maps the cache into persistable entries: complete measurements only —
    /// partials are stale after a restart anyway.
    fn size_entries(&self) -> Vec<SizeEntry> {
        self.size_cache
            .iter()
            .filter(|(_, si)| si.complete)
            .map(|(path, si)| SizeEntry {
                path: path.clone(),
                bytes: si.bytes,
                items: si.items,
                on_disk: si.on_disk,
                complete: si.complete,
                updated_epoch: si
                    .updated
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            })
            .collect()
    }

    /// Persists the current session state (split layout, pane folders and
    /// folder sizes) as `key=value` lines.
    pub fn persist_state(&self) {
        let state = SessionState {
            split: self.split,
            active_pane: self.active_pane,
            show_hidden: self.show_hidden,
            left: self.panes[0].folder.clone(),
            right: self.panes[1].folder.clone(),
            preview: [
                self.panes[0].preview_mode.as_u8(),
                self.panes[1].preview_mode.as_u8(),
            ],
            theme: Some(self.theme_preset.id().to_string()),
            sizes: self.size_entries(),
        };
        let _ = self.persistence_tx.send(PersistenceRequest::State(
            self.state_path
                .clone()
                .or_else(crate::services::state::state_file),
            state,
        ));
    }
    pub fn recalculate_dialog_size(&mut self) {
        let Some(dialog) = self.info.as_ref() else {
            return;
        };
        let path = dialog.path.clone();
        self.size_cache.remove(&path);
        if let Some(slot) = self.size_walks.remove(&path) {
            slot.handle.cancel();
        }
        self.ensure_size_walk(&path);
        let label = std::path::Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&path)
            .to_string();
        let entry = FEntry {
            path: path.clone(),
            label,
            is_dir: true,
            size: 0,
            modified: None,
        };
        let lines = build_info_fast(&entry);
        if let Some(dialog) = self.info.as_mut() {
            dialog.pending = true;
            dialog.lines = lines;
        }
        // Same metadata worker as `show_info`, so `pending` clears once stat
        // lines arrive instead of sticking forever.
        let tx = self.info_tx.clone();
        thread::spawn(move || {
            let lines = build_info_full(&entry, None);
            let _ = tx.send(InfoEvent::Meta { path, lines });
        });
    }
}

impl App {
    pub(crate) fn size_snapshot(&self) -> Vec<(PathBuf, SizeInfo)> {
        self.size_cache
            .iter()
            .map(|(p, s)| (PathBuf::from(p), *s))
            .collect()
    }
    fn cached_drives(&self) -> std::io::Result<Vec<Folder>> {
        Ok(self
            .drives
            .clone()
            .unwrap_or_else(|| self.drive_cache.lock().unwrap().clone()))
    }
    pub fn apply_drives(&mut self, drives: Vec<Folder>) {
        if self.panes[0].folder.is_none() && self.navigation_generation[0] == 0 {
            if let Some(drive) = drives.iter().find(|d| !d.path.is_empty()) {
                self.panes[0].folder = Some(drive.clone());
                self.semantic_changed();
                if !self.initializing {
                    self.request_pane_listing(0);
                }
            }
        }
        *self.drive_cache.lock().unwrap() = drives;
        *self.drive_generation.lock().unwrap() += 1;
        self.refresh_drives();
    }
    pub fn take_host_requests(&mut self) -> Vec<HostRequest> {
        std::mem::take(&mut self.host_requests)
    }
    pub fn apply_clipboard_result(&mut self, success: bool) {
        if success {
            self.set_status("Folder path copied to clipboard", false);
        } else {
            self.set_status("Failed to copy the folder path to the clipboard.", true);
        }
    }
    pub fn pane_shows_text_preview(&self, pane: usize) -> bool {
        self.panes[pane].preview_mode == PreviewMode::Column
            && self
                .selected_visible_entry_for(pane)
                .is_some_and(|e| !e.is_dir && preview_kind(&e.path) == Some(PreviewKind::Text))
    }
    fn active_pane_offers_edit(&self) -> bool {
        self.pane_shows_text_preview(self.active_pane)
    }
    pub fn open_edit(&mut self) {
        let Some(entry) = self.selected_visible_entry() else {
            return;
        };
        if entry.is_dir || preview_kind(&entry.path) != Some(PreviewKind::Text) {
            return;
        }
        let target = EntryTarget {
            pane: self.active_pane,
            path: PathBuf::from(&entry.path),
            listing_generation: self.panes[self.active_pane].listing_generation,
        };
        self.document_generation = self.document_generation.wrapping_add(1);
        let request = OpenEditorRequest {
            window_generation: self.window_generation,
            document_id: self.next_document_id,
            target,
            document_generation: self.document_generation,
            focus_generation: self.focus_generation,
        };
        self.next_document_id = self.next_document_id.wrapping_add(1);
        self.pending_editor = Some(request.clone());
        self.host_requests.push(HostRequest::OpenEditor(request));
    }
    pub fn close_edit(&mut self) {
        if let Some(document_id) = self.editor_document_id() {
            self.host_requests.retain(|request|!matches!(request,HostRequest::OpenEditor(request) if request.document_id==document_id) && !matches!(request,HostRequest::EditorKey{document_id:id,..}|HostRequest::EditorPaste{document_id:id,..} if *id==document_id));
        }
        self.edit = None;
        self.pending_editor = None;
        self.document_generation = self.document_generation.wrapping_add(1);
    }
    fn editor_document_id(&self) -> Option<u64> {
        self.edit
            .as_ref()
            .map(|e| e.document_id)
            .or_else(|| self.pending_editor.as_ref().map(|r| r.document_id))
    }
    pub fn save_edit(&mut self) {
        if let Some(snapshot) = self.editor_save_snapshot() {
            self.host_requests.push(HostRequest::SaveEditor(snapshot));
        }
    }
    pub fn edit_input(&mut self, key: crate::input::KeyEvent) -> bool {
        if let Some(document_id) = self.editor_document_id() {
            self.host_requests
                .push(HostRequest::EditorKey { document_id, key });
        }
        false
    }
    pub fn update_editor_draft(
        &mut self,
        document_id: u64,
        edit_revision: u64,
        content: String,
    ) -> bool {
        let Some(edit) = self.edit.as_mut() else {
            return false;
        };
        if edit.document_id != document_id || edit_revision < edit.edit_revision {
            return false;
        }
        // Revision identifies immutable content: identical replay is idempotent,
        // but conflicting reuse must not let an older save clear dirty text.
        if edit_revision == edit.edit_revision {
            return edit.content == content;
        }
        if edit.read_only {
            return false;
        }
        if edit.content != content {
            edit.dirty = true;
        }
        edit.content = content;
        edit.edit_revision = edit_revision;
        self.revision = self.revision.wrapping_add(1);
        true
    }
    pub fn editor_save_snapshot(&self) -> Option<crate::editor::SaveSnapshot> {
        let edit = self.edit.as_ref()?;
        let document = crate::editor::EditorDocument {
            id: edit.document_id,
            listed_path: PathBuf::from(&edit.path),
            canonical_path: edit.fs_path.clone(),
            mtime: edit.mtime_at_open,
            permissions: edit.permissions.clone(),
            read_only: edit.read_only,
            crlf: edit.crlf,
            content: edit.content.clone(),
        };
        Some(crate::editor::SaveSnapshot {
            document_id: edit.document_id,
            base_document: document,
            mtime: edit.mtime_at_open,
            edit_revision: edit.edit_revision,
            content: edit.content.clone(),
        })
    }
    pub fn apply_open_editor(
        &mut self,
        request: OpenEditorRequest,
        result: Result<crate::editor::EditorDocument, crate::editor::EditorError>,
    ) -> bool {
        if self.pending_editor.as_ref().map(|r| r.document_id) != Some(request.document_id) {
            return false;
        }
        if request.window_generation != self.window_generation
            || request.document_generation != self.document_generation
            || request.focus_generation != self.focus_generation
            || self.resolve_target(&request.target).is_none()
        {
            self.cancel_pending_editor();
            return false;
        }
        if result.as_ref().is_ok_and(|document| {
            document.id != request.document_id || document.listed_path != request.target.path
        }) {
            return false;
        }
        self.pending_editor = None;
        match result {
            Ok(document)
                if document.id == request.document_id
                    && document.listed_path == request.target.path =>
            {
                self.edit = Some(EditState {
                    pane_index: request.target.pane,
                    path: document.listed_path.to_string_lossy().into_owned(),
                    fs_path: document.canonical_path,
                    mtime_at_open: document.mtime,
                    permissions: document.permissions,
                    content: document.content,
                    document_id: document.id,
                    edit_revision: 0,
                    last_saved_revision: None,
                    dirty: false,
                    read_only: document.read_only,
                    crlf: document.crlf,
                });
                self.edit_focus = true;
            }
            Ok(_) => return false,
            Err(error) => {
                self.edit_focus = false;
                self.set_status(error.to_string(), true);
                self.advance_pane_focus();
            }
        }
        self.focus_generation = self.focus_generation.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
        true
    }
    pub fn apply_save_result(
        &mut self,
        document_id: u64,
        edit_revision: u64,
        result: Result<crate::editor::SaveCompletion, crate::editor::EditorError>,
    ) -> bool {
        if self.edit.as_ref().map(|edit| edit.document_id) != Some(document_id) {
            return false;
        }
        match result {
            Ok(completion) => {
                if completion.document_id != document_id
                    || completion.edit_revision != edit_revision
                {
                    return false;
                }
                let edit = self.edit.as_mut().unwrap();
                if edit
                    .last_saved_revision
                    .is_some_and(|last| completion.edit_revision < last)
                {
                    return false;
                }
                edit.last_saved_revision = Some(completion.edit_revision);
                edit.mtime_at_open = completion.document.mtime;
                if completion.may_clear_dirty(edit.document_id, edit.edit_revision) {
                    edit.dirty = false;
                }
                let pane_index = edit.pane_index;
                let name = Path::new(&edit.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.panes[pane_index].projection_generation =
                    self.panes[pane_index].projection_generation.wrapping_add(1);
                if let Some(entry) = self.panes[pane_index]
                    .files
                    .iter_mut()
                    .find(|e| Path::new(&e.path) == completion.invalidate_path)
                {
                    entry.size = completion.new_size;
                    entry.modified = completion
                        .document
                        .mtime
                        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64);
                }
                self.host_requests
                    .push(HostRequest::InvalidatePreview(completion.invalidate_path));
                self.set_status(format!("Saved {name}"), false);
            }
            Err(error) => self.set_status(error.to_string(), true),
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }
    pub fn dispatch(&mut self, input: crate::input::Input) -> AppResult<()> {
        if self.initializing && !matches!(input, crate::input::Input::Tick) {
            let quit = matches!(
                input,
                crate::input::Input::Action(crate::input::Command::Quit)
            ) || matches!(
                &input,
                crate::input::Input::Key(crate::input::KeyEvent {
                    code: crate::input::KeyCode::Char('q'),
                    phase: crate::input::KeyPhase::Press,
                    ..
                })
            ) || matches!(
                &input,
                crate::input::Input::Key(crate::input::KeyEvent {
                    code: crate::input::KeyCode::Char('c' | 'C'),
                    modifiers: crate::input::KeyModifiers::CONTROL,
                    phase: crate::input::KeyPhase::Press
                })
            );
            if quit {
                self.quit();
            } else {
                self.startup_inputs.push_back(input);
            }
            return Ok(());
        }
        let is_tick = matches!(&input, crate::input::Input::Tick);
        let context_before = self.input_context();
        let focus_before = self.focus_generation;
        match input {
            crate::input::Input::Key(key) => {
                if key.phase == crate::input::KeyPhase::Press {
                    crate::input::handle_key_events(key, self)?;
                }
            }
            crate::input::Input::Paste(text) => self.handle_paste(&text),
            crate::input::Input::Tick => self.tick(),
            crate::input::Input::Action(command) => self.apply_command(command),
        };
        if self.input_context() != context_before && self.focus_generation == focus_before {
            self.focus_generation = self.focus_generation.wrapping_add(1);
        }
        // A Tab request owns the editor focus transition made by THIS command.
        // Stamp its queued ticket once; later unrelated focus changes invalidate it.
        if self.input_context() == crate::input::InputContext::Editor {
            if let Some(request) = &mut self.pending_editor {
                if request.focus_generation == focus_before {
                    request.focus_generation = self.focus_generation;
                    for host_request in &mut self.host_requests {
                        if let HostRequest::OpenEditor(queued) = host_request {
                            if queued.document_id == request.document_id {
                                queued.focus_generation = request.focus_generation;
                            }
                        }
                    }
                }
            }
        }
        if !is_tick {
            self.semantic_changed();
        }
        Ok(())
    }
    pub fn dispatch_envelope(
        &mut self,
        envelope: crate::input::CommandEnvelope,
    ) -> AppResult<bool> {
        // Sequence high-water marks are scoped to the attached window.
        // A stale window must not poison the new window's sequence space.
        if envelope.window_generation != self.window_generation {
            return Ok(false);
        }
        if self
            .last_sequence
            .is_some_and(|last| envelope.sequence <= last)
        {
            return Ok(false);
        }
        self.last_sequence = Some(envelope.sequence);
        self.ack_sequence = self.ack_sequence.max(envelope.sequence);
        if matches!(
            envelope.command,
            crate::input::Input::Action(crate::input::Command::Quit)
        ) {
            self.quit();
            self.revision = self.revision.wrapping_add(1);
            return Ok(true);
        }
        if envelope.window_generation != self.window_generation
            || envelope.document_generation != self.document_generation
            || envelope.focus_generation != self.focus_generation
        {
            return Ok(false);
        }
        self.dispatch(envelope.command)?;
        Ok(true)
    }
    pub fn resolve_target(&self, target: &EntryTarget) -> Option<&FEntry> {
        let pane = self.panes.get(target.pane)?;
        if pane.listing_generation != target.listing_generation {
            return None;
        }
        pane.files
            .iter()
            .find(|e| Path::new(&e.path) == target.path)
    }
}
/// What a path can preview as, by extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    /// Decoded in-process by the `image` crate.
    Image,
    /// Video container — needs a frame extracted by an external `ffmpeg`
    /// binary (optional runtime dependency, never a build dependency).
    Video,
    /// HEIC/HEIF photo — needs HEVC; attempted via `ffmpeg` when available
    /// (many builds decode it, some don't).
    Heic,
    /// First page rasterized by the external `pdftoppm` binary (poppler,
    /// optional runtime dependency). There is no viable pure-Rust PDF
    /// rasterizer (pdfium/mupdf are C bindings; mupdf is AGPL).
    Pdf,
    /// Plain-text/code file — rendered natively as cells in the preview
    /// column (no protocol, no thumbnail).
    Text,
}

/// Classifies a path by extension, then by name. `None` = no preview at all.
///
/// Extensionless files and dotfiles (`.env`, `.env.local`, `.gitignore`,
/// `Makefile`, `LICENSE`, …) count as text: the reader NUL-sniffs the head and
/// shows a "binary file" placeholder, so a mislabelled binary degrades
/// gracefully instead of being unopenable.
pub fn preview_kind(path: &str) -> Option<PreviewKind> {
    let p = std::path::Path::new(path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    if let Some(kind) = ext.as_deref().and_then(kind_from_ext) {
        return Some(kind);
    }
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if is_text_like_name(name) {
        return Some(PreviewKind::Text);
    }
    None
}

/// Preview kind for a known file extension.
fn kind_from_ext(ext: &str) -> Option<PreviewKind> {
    match ext {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" => Some(PreviewKind::Image),
        "mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi" => Some(PreviewKind::Video),
        "heic" | "heif" => Some(PreviewKind::Heic),
        "pdf" => Some(PreviewKind::Pdf),
        "txt" | "md" | "markdown" | "rst" | "log" | "csv" | "tsv" | "json" | "toml" | "yaml"
        | "yml" | "xml" | "ini" | "conf" | "cfg" | "properties" | "env" | "sh" | "bash" | "zsh"
        | "fish" | "ps1" | "bat" | "py" | "rb" | "pl" | "js" | "ts" | "jsx" | "tsx" | "rs"
        | "go" | "c" | "h" | "cpp" | "hpp" | "cc" | "java" | "kt" | "swift" | "sql" | "html"
        | "htm" | "css" | "scss" | "less" | "lua" | "vim" | "service" | "desktop" => {
            Some(PreviewKind::Text)
        }
        _ => None,
    }
}

/// Extensionless or dotfile names that are text: well-known build/doc files
/// (by full name or stem), every dotfile (`.env`, `.env.local`, `.bashrc`,
/// `.gitignore`, …), and anything with no dot at all (`Makefile`, `LICENSE`,
/// `configure`, an extensionless script).
fn is_text_like_name(name: &str) -> bool {
    const NAMES: &[&str] = &[
        "makefile",
        "dockerfile",
        "containerfile",
        "justfile",
        "rakefile",
        "gemfile",
        "procfile",
        "vagrantfile",
        "cmakelists.txt",
        "license",
        "licence",
        "copying",
        "readme",
        "changelog",
        "authors",
        "notice",
        "install",
        "todo",
    ];
    let lower = name.to_ascii_lowercase();
    if NAMES.contains(&lower.as_str()) {
        return true;
    }
    // `Makefile.am`, `Dockerfile.dev`, `README.old`: a known stem whose
    // trailing part the extension table did not recognise still reads as text.
    let stem = lower.split('.').next().unwrap_or_default();
    if !stem.is_empty() && NAMES.contains(&stem) {
        return true;
    }
    // Dotfiles — including ones whose trailing part looks like an extension
    // (`.env.local`, `.env.production`).
    if lower.starts_with('.') {
        return true;
    }
    // No dot at all: `Makefile`, `LICENSE`, `configure`. Binaries mislabelled
    // here render as the "binary file" placeholder, not as garbage.
    !lower.contains('.')
}

fn chars_to_string(chars: &[char]) -> String {
    chars.iter().collect()
}

#[cfg(test)]
#[path = "application_tests.rs"]
mod tests;

impl App {
    pub fn enter_folder(&mut self) {
        self.submit_operation(BlockingOperation::EnterFolder);
    }
}

impl App {
    pub fn out_of_folder(&mut self) {
        self.submit_operation(BlockingOperation::ParentFolder);
    }
}

impl App {
    pub fn confirm_new_entry(&mut self) {
        self.submit_operation(BlockingOperation::Create);
    }
}

impl App {
    pub fn confirm_goto(&mut self) {
        self.submit_operation(BlockingOperation::Goto);
    }
}

impl App {
    pub fn commit_rename(&mut self) {
        self.submit_operation(BlockingOperation::Rename);
    }
}

impl App {
    pub fn eject_active_drive(&mut self) {
        self.submit_operation(BlockingOperation::Eject);
    }
}

impl App {
    pub fn set_folder_from_drives(&mut self, index: usize) {
        self.submit_operation(BlockingOperation::Drive(index));
    }
    fn operation_state(&self) -> OperationState {
        OperationState {
            panes: self.panes.clone(),
            active_pane: self.active_pane,
            show_hidden: self.show_hidden,
            search_query: self.search_query.clone(),
            renaming: self.renaming.clone(),
            new_entry: self.new_entry.clone(),
            goto_prompt: self.goto_prompt.clone(),
            drives: self.drives.clone(),
        }
    }
    fn submit_operation(&mut self, operation: BlockingOperation) {
        let pane = self.active_pane;
        let relative = matches!(
            operation,
            BlockingOperation::EnterFolder | BlockingOperation::ParentFolder
        );
        if relative
            && (self.navigation_inflight[pane].is_some() || self.navigation_waiting_listing[pane])
        {
            self.relative_navigation
                .push_back((pane, self.navigation_generation[pane], operation));
            return;
        }
        let navigation = relative
            || matches!(
                operation,
                BlockingOperation::Goto | BlockingOperation::Drive(_)
            );
        let navigation_ticket = if navigation {
            self.navigation_generation[pane] = self.navigation_generation[pane].wrapping_add(1);
            let ticket = self.navigation_generation[pane];
            self.navigation_inflight[pane] = Some(ticket);
            Some(ticket)
        } else {
            None
        };
        self.pending_operations += 1;
        if self.operation_tx.is_none() {
            let (tx, rx) = mpsc::channel::<OperationRequest>();
            let result_tx = self.operation_result_tx.clone();
            let epoch = self.operation_epoch.clone();
            thread::spawn(move || {
                while let Ok(request) = rx.recv() {
                    let source = std::array::from_fn(|i| {
                        (
                            request.state.panes[i]
                                .folder
                                .as_ref()
                                .map(|f| f.path.clone()),
                            request.state.panes[i].listing_generation,
                        )
                    });
                    let before = request.state.clone();
                    let mut worker = App {
                        panes: request.state.panes,
                        active_pane: request.state.active_pane,
                        show_hidden: request.state.show_hidden,
                        search_query: request.state.search_query,
                        renaming: request.state.renaming,
                        new_entry: request.state.new_entry,
                        goto_prompt: request.state.goto_prompt,
                        drives: request.state.drives,
                        defer_listings: true,
                        ..App::default()
                    };
                    if request.epoch == epoch.load(Ordering::Acquire) {
                        match request.operation {
                            BlockingOperation::EnterFolder => worker.enter_folder_blocking(),
                            BlockingOperation::ParentFolder => worker.out_of_folder_blocking(),
                            BlockingOperation::Create => worker.confirm_new_entry_blocking(),
                            BlockingOperation::Goto => worker.confirm_goto_blocking(),
                            BlockingOperation::Rename => worker.commit_rename_blocking(),
                            BlockingOperation::Drive(i) => {
                                worker.set_folder_from_drives_blocking(i)
                            }
                            BlockingOperation::Eject => worker.eject_active_drive_blocking(),
                        }
                    };
                    let result = OperationResult {
                        epoch: request.epoch,
                        navigation_ticket: request.navigation_ticket,
                        before,
                        source,
                        state: worker.operation_state(),
                        status: worker.status.take(),
                        listings: std::mem::take(&mut worker.deferred_listings),
                        host_requests: std::mem::take(&mut worker.host_requests),
                    };
                    if result_tx.send(result).is_err() {
                        break;
                    }
                }
            });
            self.operation_tx = Some(tx);
        }
        let request = OperationRequest {
            epoch: self.operation_epoch.load(Ordering::Acquire),
            navigation_ticket,
            operation,
            state: self.operation_state(),
        };
        // Rename and goto close immediately in the oracle, including on errors.
        if matches!(operation, BlockingOperation::Rename) {
            self.renaming = None;
        }
        if matches!(operation, BlockingOperation::Goto) {
            self.goto_prompt = None;
        }
        if let Some(tx) = &self.operation_tx {
            let _ = tx.send(request);
        }
    }
    fn drain_operation_results(&mut self) {
        while let Ok(result) = self.operation_result_rx.try_recv() {
            self.apply_operation_result(result);
        }
    }
    fn apply_operation_result(&mut self, result: OperationResult) {
        self.pending_operations = self.pending_operations.saturating_sub(1);
        if result.epoch != self.operation_epoch.load(Ordering::Acquire) {
            return;
        }
        let active = result.state.active_pane;
        if let Some(ticket) = result.navigation_ticket {
            if self.navigation_generation[active] != ticket {
                return;
            }
            self.navigation_inflight[active] = None;
        }
        let mut accepted = [false; 2];
        for (i, expected) in result.source.iter().enumerate() {
            let current = (
                self.panes[i].folder.as_ref().map(|f| f.path.clone()),
                self.panes[i].listing_generation,
            );
            if &current == expected || (i == active && result.navigation_ticket.is_some()) {
                let before = &result.before.panes[i];
                let after = &result.state.panes[i];
                let pane = &mut self.panes[i];
                if before.folder != after.folder {
                    pane.folder = after.folder.clone();
                    pane.projection_generation = pane.projection_generation.wrapping_add(1);
                    pane.files.clear();
                    pane.selected.clear();
                    pane.state.select(None);
                    pane.listing_settled = false;
                }
                if before.filter_query != after.filter_query {
                    pane.projection_generation = pane.projection_generation.wrapping_add(1);
                    pane.filter_query = after.filter_query.clone();
                    pane.filter_indices = after.filter_indices.clone();
                }
                if before.pending_select != after.pending_select {
                    pane.pending_select = after.pending_select.clone();
                }
                if before.state != after.state {
                    pane.state = after.state.clone();
                }
                accepted[i] = true;
            }
        }
        if accepted[active] {
            if result.before.search_query != result.state.search_query {
                self.search_query = result.state.search_query;
            }
            if self.new_entry == result.before.new_entry {
                self.new_entry = result.state.new_entry;
            }
            if let Some(status) = result.status {
                self.observe_existing_path_status(status.is_error);
                self.status = Some(status);
            }
            if self.drives != result.state.drives {
                self.drives = result.state.drives;
                self.semantic_changed();
            }
        }
        self.host_requests.extend(result.host_requests);
        for (pane, streaming) in result.listings {
            if accepted[pane] {
                if result.navigation_ticket.is_some() {
                    self.navigation_waiting_listing[pane] = true;
                }
                if streaming {
                    self.request_pane_listing(pane);
                } else {
                    self.list_files_for_pane(pane);
                }
            }
        }
    }
    fn start_pending_relative_navigation(&mut self) {
        for pane in 0..2 {
            if self.navigation_waiting_listing[pane] && self.panes[pane].listing_settled {
                self.navigation_waiting_listing[pane] = false;
            }
        }
        let Some(&(pane, generation, _)) = self.relative_navigation.front() else {
            return;
        };
        if self.navigation_inflight[pane].is_some() || self.navigation_waiting_listing[pane] {
            return;
        }
        let (_, _, operation) = self.relative_navigation.pop_front().unwrap();
        // A later absolute request supersedes preceding relative intents.
        if generation != self.navigation_generation[pane] {
            return;
        }
        let active = self.active_pane;
        self.active_pane = pane;
        self.submit_operation(operation);
        self.active_pane = active;
        // Relative intents in the same ordered chain follow this new ticket.
        for (queued_pane, queued_generation, _) in &mut self.relative_navigation {
            if *queued_pane == pane && *queued_generation == generation {
                *queued_generation = self.navigation_generation[pane];
            }
        }
    }
    pub fn pending_operations(&self) -> usize {
        self.pending_operations + self.relative_navigation.len()
    }
}

impl App {
    /// Checked receipt of all earlier accepted writes. Retain this original
    /// receiver across timeouts. Failure owns a sealed persistence-only retry.
    pub fn checked_persistence_barrier(
        &self,
    ) -> mpsc::Receiver<Result<PersistenceReceipt, PersistenceFailure>> {
        let (tx, rx) = mpsc::channel();
        let _ = self
            .persistence_tx
            .send(PersistenceRequest::CheckedBarrier(tx));
        rx
    }

    /// Queue a lossless persistence barrier; the host waits off the command lane.
    pub fn persistence_barrier(&self) -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel();
        let _ = self.persistence_tx.send(PersistenceRequest::Barrier(tx));
        rx
    }
}

impl App {
    pub fn apply_command(&mut self, command: crate::input::Command) {
        use crate::input::Command::*;
        match command {
            Quit => self.quit(),
            MoveNext => self.next_item(),
            MovePrevious => self.prev_item(),
            MoveTop => self.goto_top(),
            MoveBottom => self.goto_bottom(),
            EnterFolder => self.enter_folder(),
            ParentFolder => self.out_of_folder(),
            ToggleSplit => self.toggle_split(),
            SwitchPane => self.switch_pane(),
            ToggleSelectCurrent => self.toggle_select_current(),
            ToggleSelectAll => self.toggle_select_all(),
            InvertSelection => self.invert_selection(),
            ToggleHidden => self.toggle_hidden(),
            StartSearch => self.start_search(),
            CancelSearch => self.cancel_search(),
            ConfirmSearch => self.confirm_search(),
            ClearFilter => self.clear_filter(),
            CycleSort => self.cycle_sort(),
            CyclePreview => self.cycle_preview(),
            CycleTheme => self.cycle_theme(),
            ToggleBookmark => self.toggle_bookmark(),
            StartRename => self.start_rename(),
            CancelRename => self.cancel_rename(),
            CommitRename => self.commit_rename(),
            StartNewEntry => self.start_new_entry(),
            CancelNewEntry => self.cancel_new_entry(),
            ConfirmNewEntry => self.confirm_new_entry(),
            StartGoto => self.start_goto(),
            CancelGoto => self.cancel_goto(),
            ConfirmGoto => self.confirm_goto(),
            RequestCopy => self.request_copy(),
            RequestMove => self.request_move(),
            RequestDelete => self.request_delete(),
            ConfirmPending => self.confirm_pending(),
            CancelConfirm => self.cancel_confirm(),
            CycleConfirmPolicy => self.cycle_confirm_policy(),
            ToggleCopyBoard => self.toggle_copy_board(),
            BoardNext => self.copy_board_next(),
            BoardPrevious => self.copy_board_prev(),
            PauseSelectedJob => self.toggle_selected_job_pause(),
            CancelSelectedJob => self.cancel_selected_job(),
            ShowInfo => self.show_info(),
            CloseInfo => self.close_info(),
            CancelSizeWalk => self.cancel_dialog_size_walk(),
            RecalculateSize => self.recalculate_dialog_size(),
            ShowHelp => self.show_keybindings(),
            CloseHelp => self.close_keybindings(),
            ClearStatus => self.clear_status(),
            CopyFolderPath => self.copy_folder_path(),
            Reveal => self.open_in_file_manager(),
            Terminal => self.spawn_native_terminal(),
            Eject => self.eject_active_drive(),
            Drive(i) => self.set_folder_from_drives(i),
            CommonFolder(i) => self.set_folder_from_common_folders(i),
            Bookmark(i) => self.set_folder_from_bookmark(i),
            SelectEntry(target) => {
                if self.resolve_target(&target).is_some() {
                    if let Some(row) = self
                        .pane_visible_rows(target.pane)
                        .iter()
                        .position(|(_, e)| Path::new(&e.path) == target.path)
                    {
                        self.active_pane = target.pane;
                        self.panes[target.pane].state.select(Some(row));
                        self.panes[target.pane].user_navigated = true;
                    }
                }
            }
        }
    }
}

impl App {
    fn now(&self) -> Instant {
        self.clock_override.unwrap_or_else(Instant::now)
    }
    pub fn dispatch_at(&mut self, input: crate::input::Input, now: Instant) -> AppResult<()> {
        self.clock_override = Some(now);
        let result = self.dispatch(input);
        self.clock_override = None;
        result
    }
}

impl App {
    /// Nonblocking close/cancel boundary. An in-flight OS syscall cannot be preempted,
    /// but queued mutations and all stale view completions are invalidated immediately.
    pub fn cancel_pending_work(&mut self) {
        self.operation_epoch.fetch_add(1, Ordering::AcqRel);
        self.pending_operations = 0;
        self.relative_navigation.clear();
        self.navigation_inflight = [None; 2];
        self.navigation_waiting_listing = [false; 2];
        self.startup_inputs.clear();
        for pane in &mut self.panes {
            pane.listing_generation = pane.listing_generation.wrapping_add(1);
        }
        for generation in &mut self.navigation_generation {
            *generation = generation.wrapping_add(1);
        }
        for slot in self.size_walks.values() {
            slot.handle.cancel();
        }
        self.size_walks.clear();
        for job in &self.jobs {
            job.control.request_cancel();
        }
        if let Some(deletion) = &self.deletion {
            deletion.control.request_cancel();
        }
        self.close_edit();
        self.edit_focus = false;
        self.focus_generation = self.focus_generation.wrapping_add(1);
        self.transfer_generation = self.transfer_generation.wrapping_add(1);
        self.transfer_probe_pending = None;
        self.transfer_dest = None;
        self.semantic_changed();
    }
    pub fn attach_window(&mut self, window_generation: u64) {
        if window_generation != self.window_generation {
            self.cancel_pending_work();
            self.window_generation = window_generation;
            self.last_sequence = None;
            self.ack_sequence = 0;
            self.revision = self.revision.wrapping_add(1);
        }
    }
}

impl App {
    pub fn pending_editor_request(&self) -> Option<OpenEditorRequest> {
        self.pending_editor.clone()
    }
}

impl App {
    fn cancel_pending_editor(&mut self) {
        let Some(request) = self.pending_editor.take() else {
            return;
        };
        self.semantic_changed();
        self.host_requests.retain(|r|!matches!(r,HostRequest::OpenEditor(queued) if queued.document_id==request.document_id) && !matches!(r,HostRequest::EditorKey{document_id,..}|HostRequest::EditorPaste{document_id,..} if *document_id==request.document_id));
        self.document_generation = self.document_generation.wrapping_add(1);
        if self.edit.is_none() {
            self.edit_focus = false;
        }
    }
}

#[path = "application_persistence.rs"]
mod application_persistence;
pub use application_persistence::{PersistenceFailure, PersistenceReceipt, PersistenceRetry};

#[cfg(test)]
#[path = "application_persistence_tests.rs"]
mod persistence_tests;

#[cfg(test)]
#[path = "application_transfer_probe_tests.rs"]
mod transfer_probe_tests;

#[path = "application_existing_path.rs"]
mod existing_path;
pub use existing_path::{
    ExistingPathFocusStamp, ExistingPathKind, ExistingPathReceipt, ExistingPathScope,
};
