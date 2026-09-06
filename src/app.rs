use std::collections::{HashMap, HashSet, VecDeque};
use std::error;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use open::that_detached;
use ratatui::widgets::ListState;
use ratatui_image::{picker::Picker, protocol::Protocol};

use crate::{
    domain::data::Folder,
    services::{
        bookmarks::{next_free_shortcut, read_bookmarks, write_bookmarks},
        drives::{eject_drive, list_drives, mount_drive},
        file_info::{
            build_info_fast, build_info_full, dir_size, on_disk_bytes, size_line_final, DirSize,
            InfoEvent, SizeInfo, WalkHandle,
        },
        folders::list_common_folders,
        list_files::{list_files_bounded, list_files_chunked, FEntry, LISTING_CHUNK},
        state::{load_state, load_state_from, save_state, save_state_to, SessionState, SizeEntry},
        thumbnails::{
            preview_kind, prune_cache, spawn_workers, PreviewKind, ThumbEvent, ThumbRequest,
            WorkerQueues, JOB_QUEUE_HI_CAP, JOB_QUEUE_LO_CAP,
        },
        transfer::{
            spawn_delete_job, spawn_job, Job, JobControl, JobEvent, JobKind, JobStatus,
            OverwritePolicy,
        },
    },
    utils::{
        fuzzy::fuzzy_score,
        is_dir::{get_directory, get_parent_directory},
    },
};
/// Application result type.
pub type AppResult<T> = std::result::Result<T, Box<dyn error::Error>>;

/// A single file-browser pane (one side of a vertical split).
#[derive(Debug, Default)]
pub struct Pane {
    pub folder: Option<Folder>,
    pub state: ListState,
    pub files: Vec<FEntry>,
    /// Parallel to `files`: `true` marks entries multi-selected with Space
    /// for batch copy/move/delete.
    pub selected: Vec<bool>,
    /// First row index of the visible render window (windowed rendering).
    pub render_scroll: usize,
    /// First visible index of the grid-mode window (thumbnail grid).
    pub grid_top: usize,
    /// Image preview presentation of THIS pane (`v` cycles it); modes are
    /// per pane and persist across restarts.
    pub preview_mode: PreviewMode,
    /// Preview column area in cells; written by the UI every frame while
    /// this pane's column is open (0 × 0 until the first preview frame).
    pub preview_area: (u16, u16),
    /// `false` while a chunked listing is still streaming; `true` once the
    /// final sorted pass replaced the streamed prefix (or the listing
    /// errored). Test/UI hook for "listing is complete".
    pub listing_settled: bool,
    /// Confirmed search: while set, the pane shows only the matching files
    /// (`filter_indices`, best match first) and all actions operate on that
    /// view. Cleared with Esc.
    pub filter_query: Option<String>,
    /// Visible file indices for the active filter (into `files`).
    pub filter_indices: Vec<usize>,
    /// Path to select on the next listing settle (e.g. a just-created entry).
    pub pending_select: Option<String>,
    /// Bumped on every listing request; results carrying an older
    /// generation are dropped, so overlapping listings never interleave
    /// (the "folder lists itself" bug).
    pub listing_generation: u64,
    /// Active sort mode of the listing, cycled with `,`
    /// (0=Name, 1=Size, 2=Modified, 3=Kind).
    pub sort_mode: usize,
}

/// Image preview presentation, cycled with `v`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PreviewMode {
    /// No preview.
    #[default]
    Off,
    /// Side column rendering the selected entry.
    Column,
    /// Thumbnail grid replacing the file list.
    Grid,
}

impl PreviewMode {
    /// Persisted representation (state file `preview<N>=`).
    pub fn as_u8(self) -> u8 {
        match self {
            PreviewMode::Off => 0,
            PreviewMode::Column => 1,
            PreviewMode::Grid => 2,
        }
    }

    /// Inverse of [`PreviewMode::as_u8`]; unknown values read as off.
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => PreviewMode::Column,
            2 => PreviewMode::Grid,
            _ => PreviewMode::Off,
        }
    }
}

/// An in-place rename being edited in a modal text box.
pub struct RenamePrompt {
    /// Index into the active pane's file list.
    pub index: usize,
    /// Original name (no-op / existence checks).
    pub original: String,
    /// Edited name as Unicode characters.
    pub text: Vec<char>,
    /// Cursor position (index into `text`).
    pub cursor: usize,
}

/// A transient message shown in the bottom status bar. `is_error` styles it
/// red; eject-busy and rename collisions are errors, "copied 3 items" is not.
#[derive(Debug, Clone)]
pub struct Status {
    pub text: String,
    /// `true` renders red; `false` renders as an ordinary notice.
    pub is_error: bool,
    /// When the message was raised; the bar clears itself after
    /// [`STATUS_TTL`].
    pub raised: Instant,
}

/// How long a status message stays visible.
pub const STATUS_TTL: Duration = Duration::from_secs(8);

/// Minimum rendered preview protocols kept in memory. The effective cap is
/// raised per frame to cover the grid working set (visible + prefetched
/// cells, see `render_grid`): if the cap were smaller, prefetch would evict
/// visible entries and they would re-decode every frame — visible as
/// endless thumbnail flashing on large monitors.
const THUMB_CACHE_CAP_MIN: usize = 256;

/// Gap between navigation keys under which they count as "held down"
/// (OS key-repeat fires at ~30 Hz; anything slower is a fresh press).
pub const SCROLL_REPEAT_WINDOW: Duration = Duration::from_millis(200);
/// Held-key repeats needed for each step-size increase (1,1,1,1,1,1 → 2 …).
pub const SCROLL_RAMP_EVERY: u32 = 6;
/// Maximum rows moved per key repeat once fully ramped.
pub const SCROLL_MAX_STEP: usize = 6;

/// Aggregate info dialog for a multi-selection: sums the sizes of all
/// selected folders (via their background walks) and files (via stat).
#[derive(Debug)]
pub struct MultiInfoState {
    /// Selected paths (folders get walks, files get stat'd).
    pub paths: Vec<String>,
    pub folders: usize,
    pub files: usize,
    pub started: Instant,
}

/// Live sync state while a transfer writes into a folder: the destination
/// pane's listing is refreshed periodically so copied items appear live,
/// even while the transfer is still running.
#[derive(Debug, Clone)]
pub struct TransferDestSync {
    pub dest_dir: String,
    /// Destination path of the first item (cursor reveal target).
    pub reveal_path: String,
    pub last_refresh: Instant,
}

/// Live state of the background batch deletion.
#[derive(Debug)]
pub struct DeletionState {
    pub total: usize,
    pub done: usize,
    /// Path currently being removed (spinner target).
    pub current: Option<String>,
    pub started: Instant,
    pub control: Arc<JobControl>,
}

/// Read-only metadata dialog for a file or folder. Opens instantly with the
/// no-filesystem fast lines; the worker's `Meta` event replaces them with
/// the stat lines, and the folder's Size line is injected/updated from the
/// size cache while its background walk runs.
pub struct InfoDialog {
    pub lines: Vec<String>,
    /// Queried path; matches events arriving from background threads.
    pub path: String,
    /// `true` until the worker's `Meta` event arrives.
    pub pending: bool,
    /// When the dialog opened; drives the loading spinner animation.
    pub started: Instant,
}

/// Maximum file size accepted by the in-app text editor. Anything larger
/// opens as a read-only preview.
pub const EDIT_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Live editing state for the preview column: the whole file in a textarea
/// plus everything `save_edit` needs to write it back safely.
pub struct EditState {
    /// Which pane's preview column owns this editor.
    pub pane_index: usize,
    /// Path as listed in the pane (display + entry lookup).
    pub path: String,
    /// Canonical filesystem path used for ALL I/O: saving via the listed
    /// path would `rename` over a symlink and destroy the link instead of
    /// updating its target.
    pub fs_path: std::path::PathBuf,
    /// Full-precision mtime when opened / last saved — the on-disk change
    /// guard for `save_edit`. Second-granularity comparisons clobber
    /// external edits made within the same second.
    pub mtime_at_open: Option<SystemTime>,
    /// Original permissions, restored on save.
    pub permissions: std::fs::Permissions,
    pub textarea: ratatui_textarea::TextArea<'static>,
    /// Any keypress actually modified the buffer.
    pub dirty: bool,
    /// Opened without write access: the buffer renders but never saves.
    pub read_only: bool,
    /// File uses CRLF endings; Enter inserts `\r\n` to stay consistent.
    pub crlf: bool,
}

/// Head of a text file for the preview column.
#[derive(Debug, Clone)]
pub struct TextPreview {
    /// Listed path of the file this head came from (cache eviction key).
    pub path: String,
    pub content: String,
    /// NUL byte found in the head — treat as binary, don't render text.
    pub binary: bool,
    /// The file was larger than the read cap.
    pub truncated: bool,
}

/// In-place input for creating a new entry. Extension decides the kind:
/// "notes" → folder, "notes.txt" → file.
pub struct NewEntryPrompt {
    pub text: Vec<char>,
    pub cursor: usize,
}

/// A pending destructive action awaiting confirmation.
pub struct Confirm {
    /// Which operation `y` confirms.
    pub action: ConfirmAction,
    /// Overwrite policy for copy/move confirmations.
    pub policy: OverwritePolicy,
    /// Prompt label, e.g. `'report.pdf'` or `3 items`.
    pub label: String,
    /// Full paths of the items the action would affect.
    pub paths: Vec<String>,
    /// Destination folder for copy/move; `None` for delete.
    pub dest_dir: Option<String>,
}

/// The operation a confirmation dialog refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction {
    Delete,
    Copy,
    Move,
}

/// A running folder-size walk plus its start time (drives the spinner).
struct WalkSlot {
    handle: WalkHandle,
    started: Instant,
}

/// Application.
pub struct App {
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
    drives_running: bool,

    /// Startup file listings run on worker threads and are delivered here, so
    /// a slow folder (e.g. a cold spin-up HDD) can't block `App::new()`.
    file_list_tx: mpsc::Sender<(usize, Vec<FEntry>, bool, u64)>,
    file_list_rx: mpsc::Receiver<(usize, Vec<FEntry>, bool, u64)>,
    /// Terminal image-rendering picker (protocol + font size), probed once
    /// at startup before raw mode. `None` disables previews entirely.
    pub picker: Option<Picker>,
    /// Finished preview jobs, delivered by the worker pool.
    thumb_tx: mpsc::Sender<ThumbEvent>,
    thumb_rx: mpsc::Receiver<ThumbEvent>,
    /// High-priority queue: on-screen cells jump ahead of prefetch.
    thumb_jobs_hi: mpsc::SyncSender<ThumbRequest>,
    thumb_jobs_hi_rx: Option<mpsc::Receiver<ThumbRequest>>,
    /// Prefetch queue: screens adjacent to the viewport.
    thumb_jobs_lo: mpsc::SyncSender<ThumbRequest>,
    thumb_jobs_lo_rx: Option<mpsc::Receiver<ThumbRequest>>,
    /// Rendered protocols keyed by [`ThumbRequest::mem_key`].
    thumb_cache: HashMap<String, Protocol>,
    /// Effective eviction cap for `thumb_cache` this frame: at least
    /// [`THUMB_CACHE_CAP_MIN`], raised by the grid renderer to cover its
    /// working set. Reset every frame by `ui::render`.
    thumb_cache_cap: usize,
    /// FIFO eviction order for `thumb_cache`.
    thumb_order: VecDeque<String>,
    /// Requests that failed this session (negative cache: a broken file
    /// must not re-dispatch a worker on every frame, and retrying permanent
    /// decode errors periodically just flashes the cell). A file that later
    /// changes gets a new mtime/size and therefore a new key. Unbounded only
    /// in the count of distinct broken files (small strings).
    thumb_failed: HashSet<String>,
    /// Requests already dispatched to a worker (dedup guard).
    thumb_pending: HashSet<String>,
    /// Whether the optional `ffmpeg` runtime dependency is usable (probed
    /// once in the background); gates video and HEIC previews.
    ffmpeg_available: Arc<AtomicBool>,
    /// Whether the optional `pdftoppm` (poppler) runtime dependency is
    /// usable; gates PDF previews.
    pdftoppm_available: Arc<AtomicBool>,
    /// Text-file heads keyed by [`ThumbRequest::mem_key`] (cols/rows are 0).
    text_cache: HashMap<String, TextPreview>,
    /// Live editor for the preview column; `Some` while a text file is open.
    pub edit: Option<EditState>,
    /// Whether keyboard focus is on the preview editor (Tab offered it).
    pub edit_focus: bool,
    /// Decode workers started (once, on the first picker install).
    thumb_workers_started: bool,
    /// State file location override; `None` = the real `~/.config/ira/state`.
    /// Integration tests set this so their walks never touch user config.
    pub state_path: Option<PathBuf>,
}

impl Default for App {
    fn default() -> Self {
        let (job_tx, job_rx) = mpsc::channel();
        let (file_list_tx, file_list_rx) = mpsc::channel();
        let (info_tx, info_rx) = mpsc::channel();
        let (thumb_tx, thumb_rx) = mpsc::channel();
        let (thumb_jobs_hi, thumb_jobs_hi_rx) = mpsc::sync_channel(JOB_QUEUE_HI_CAP);
        let (thumb_jobs_lo, thumb_jobs_lo_rx) = mpsc::sync_channel(JOB_QUEUE_LO_CAP);
        Self {
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
            deletion: None,
            deletion_box_hidden: false,
            deleting_paths: HashSet::new(),
            scroll_dir: 0,
            scroll_repeat: 0,
            last_scroll: None,
            job_tx,
            job_rx,
            size_cache: HashMap::new(),
            size_walks: HashMap::new(),
            next_job_id: 0,
            drive_cache: Arc::new(Mutex::new(Vec::new())),
            drive_generation: Arc::new(Mutex::new(0)),
            seen_drive_generation: 0,
            drives_running: false,
            file_list_tx,
            file_list_rx,
            picker: None,
            info_tx,
            thumb_tx,
            thumb_rx,
            thumb_jobs_hi,
            thumb_jobs_hi_rx: Some(thumb_jobs_hi_rx),
            thumb_jobs_lo,
            thumb_jobs_lo_rx: Some(thumb_jobs_lo_rx),
            thumb_pending: HashSet::new(),
            thumb_workers_started: false,
            thumb_cache: HashMap::new(),
            thumb_cache_cap: THUMB_CACHE_CAP_MIN,
            thumb_order: VecDeque::new(),
            thumb_failed: HashSet::new(),
            info_rx,
            ffmpeg_available: Arc::new(AtomicBool::new(false)),
            pdftoppm_available: Arc::new(AtomicBool::new(false)),
            text_cache: HashMap::new(),
            edit: None,
            edit_focus: false,
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
    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        let mut default = Self::default();
        default.load_bookmarks();
        if let Ok(app_drives) = list_drives() {
            // Start in the first already-mounted drive, if any; unmounted
            // drives are left for the user to mount on demand.
            if let Some(first_mounted) = app_drives.iter().find(|d| !d.path.is_empty()) {
                default.panes[0].folder = Some(first_mounted.clone());
                default.panes[0].state.select(None);
            }
            default.drives = Some(app_drives);
        }
        default.restore_state();
        default.start_drive_poller();
        // Startup pane listings run on the async chunked worker so a slow
        // drive can't block the first frame.
        default.request_pane_listing(0);
        default.request_pane_listing(1);
        default
    }

    /// Handles the tick event of the terminal.
    pub fn tick(&mut self) {
        self.drain_jobs();
        self.drain_info_results();
        self.refresh_drives();
        self.pick_up_pane_listings();
        self.refresh_transfer_destinations();
        self.pick_up_thumbnails();
        self.expire_status();
    }

    /// Raises a transient bottom-bar message (replaces any current one).
    pub fn set_status(&mut self, text: impl Into<String>, is_error: bool) {
        self.status = Some(Status {
            text: text.into(),
            is_error,
            raised: Instant::now(),
        });
    }

    /// Routes pasted text to whichever input dialog is active.
    pub fn handle_paste(&mut self, text: &str) {
        let cleaned = text.strip_suffix('\n').unwrap_or(text);
        let cleaned = cleaned.strip_suffix('\r').unwrap_or(cleaned);
        if cleaned.is_empty() {
            return;
        }
        if self.edit_focus {
            if let Some(edit) = &mut self.edit {
                if !edit.read_only {
                    edit.textarea.insert_str(cleaned);
                    edit.dirty = true;
                }
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
        self.status = None;
    }

    /// Drops the status message once its TTL has elapsed.
    fn expire_status(&mut self) {
        if self
            .status
            .as_ref()
            .is_some_and(|s| s.raised.elapsed() >= STATUS_TTL)
        {
            self.status = None;
        }
    }

    /// Spawns a background thread that re-scans attached drives every 2 s and
    /// publishes the result (with a generation bump) through shared state, so
    /// the render thread never blocks on `lsblk`. Works the same way on
    /// Linux, Windows and macOS: `list_drives()` runs on this worker, not on
    /// the render path.
    fn start_drive_poller(&mut self) {
        if self.drives_running {
            return;
        }
        self.drives_running = true;

        let cache = self.drive_cache.clone();
        let generation = self.drive_generation.clone();

        thread::spawn(move || loop {
            if let Ok(drives) = list_drives() {
                let mut guard = cache.lock().unwrap();
                if *guard != drives {
                    *guard = drives;
                    *generation.lock().unwrap() += 1;
                }
            }
            thread::sleep(Duration::from_secs(2));
        });
    }

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.running = false;
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
        self.drives = Some(drives);
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
        if pane_index >= self.panes.len() {
            return;
        }
        // Any new listing request invalidates in-flight results (a stale
        // async listing must never replace a newer sync/streamed one).
        self.panes[pane_index].listing_generation =
            self.panes[pane_index].listing_generation.wrapping_add(1);
        let Some(path) = self.panes[pane_index]
            .folder
            .as_ref()
            .map(|f| f.path.clone())
        else {
            return;
        };
        // Fast path: a bounded readdir decides sync vs streaming. Small
        // folders (the overwhelming majority) fill instantly, fully sorted,
        // with no Loading flash; the bounded read costs only a few ms.
        if let Ok((mut files, complete)) =
            list_files_bounded(&path, LISTING_CHUNK, self.show_hidden)
        {
            if complete {
                files.sort_by(|a, b| a.label.cmp(&b.label));
                let len = files.len();
                let pane = &mut self.panes[pane_index];
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
                    let labels: Vec<String> = pane.files.iter().map(|f| f.label.clone()).collect();
                    pane.filter_indices = fuzzy_indices(&labels, &q);
                }
                return;
            }
        }
        // Big folder: clear the pane and stream it in the background
        // (Loading… shows until the first chunk lands).
        self.request_pane_listing(pane_index);
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
        if pane_index >= self.panes.len() {
            return;
        }
        let pane = &mut self.panes[pane_index];
        pane.listing_settled = false;
        pane.listing_generation = pane.listing_generation.wrapping_add(1);
        // Clear the rows now: the pane shows "Loading…" until the first
        // chunk of the NEW folder arrives (never stale mixed content).
        pane.files.clear();
        pane.selected.clear();
        pane.render_scroll = 0;
        pane.state.select(None);
        let generation = pane.listing_generation;
        let Some(path) = pane.folder.as_ref().map(|f| f.path.clone()) else {
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
                    pane.filter_indices = fuzzy_indices(&labels, &q);
                }
            } else {
                pane.listing_settled = false;
                pane.selected
                    .extend(std::iter::repeat_n(false, files.len()));
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
    /// one-shot disk-cache prune.
    pub fn set_picker(&mut self, picker: Picker) {
        self.picker = Some(picker.clone());
        if self.thumb_workers_started {
            return;
        }
        self.thumb_workers_started = true;
        let hi = self
            .thumb_jobs_hi_rx
            .take()
            .expect("thumb workers started twice");
        let lo = self
            .thumb_jobs_lo_rx
            .take()
            .expect("thumb workers started twice");
        spawn_workers(
            picker,
            WorkerQueues {
                hi: Arc::new(Mutex::new(hi)),
                lo: Arc::new(Mutex::new(lo)),
            },
            self.thumb_tx.clone(),
        );
        std::thread::spawn(prune_cache);
        // Probe for the optional runtime dependencies once (background:
        // the spawns cost a few ms). While unknown, videos/HEIC/PDF show
        // glyphs and the preview column explains what is missing.
        let ffmpeg = Arc::clone(&self.ffmpeg_available);
        let pdftoppm = Arc::clone(&self.pdftoppm_available);
        std::thread::spawn(move || {
            let probe = |bin: &str| {
                std::process::Command::new(bin)
                    .arg("-version")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success())
            };
            ffmpeg.store(probe("ffmpeg"), std::sync::atomic::Ordering::Relaxed);
            pdftoppm.store(probe("pdftoppm"), std::sync::atomic::Ordering::Relaxed);
        });
    }

    /// Whether the active pane's preview can offer the text editor via Tab:
    /// Column mode with a text file selected.
    fn active_pane_offers_edit(&self) -> bool {
        self.pane().preview_mode == PreviewMode::Column
            && self
                .selected_visible_entry()
                .is_some_and(|e| !e.is_dir && preview_kind(&e.path) == Some(PreviewKind::Text))
    }

    /// Opens the editor for the active pane's selected text file. All
    /// failures become status messages and leave the preview read-only.
    pub fn open_edit(&mut self) {
        let pane_index = self.active_pane;
        let Some(entry) = self.selected_visible_entry() else {
            return;
        };
        if entry.is_dir || preview_kind(&entry.path) != Some(PreviewKind::Text) {
            return;
        }
        let list_path = entry.path.clone();
        // Canonicalize for I/O: saving renames over this path, and renaming
        // a symlink destroys the link instead of updating its target.
        let fs_path = match std::fs::canonicalize(&list_path) {
            Ok(p) => p,
            Err(e) => {
                self.set_status(format!("open failed: {e}"), true);
                return;
            }
        };
        // One consistent snapshot: metadata is taken on the OPEN HANDLE, so
        // the mtime guard compares against exactly the bytes we read — not
        // a stat from an earlier moment (TOCTOU).
        let file = match std::fs::File::open(&fs_path) {
            Ok(f) => f,
            Err(e) => {
                self.set_status(format!("open failed: {e}"), true);
                return;
            }
        };
        let metadata = match file.metadata() {
            Ok(m) => m,
            Err(e) => {
                self.set_status(format!("open failed: {e}"), true);
                return;
            }
        };
        if metadata.len() > EDIT_MAX_BYTES {
            self.set_status("file too large to edit (> 5 MB)", true);
            return;
        }
        // Cap the read itself (+1 sentinel): a file that grows between the
        // size check and the read can't blow the memory budget.
        let mut bytes = Vec::new();
        let mut reader = file.take(EDIT_MAX_BYTES + 1);
        use std::io::Read as _;
        if let Err(e) = reader.read_to_end(&mut bytes) {
            self.set_status(format!("open failed: {e}"), true);
            return;
        }
        if bytes.len() as u64 > EDIT_MAX_BYTES {
            self.set_status("file too large to edit (> 5 MB)", true);
            return;
        }
        if bytes.contains(&0) {
            self.set_status("binary file — not editable", true);
            return;
        }
        let content = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                self.set_status("non-UTF-8 file — read-only preview only", true);
                return;
            }
        };
        let mtime_at_open = metadata.modified().ok();
        // CRLF files are normalized to LF in the buffer and re-encoded on
        // save: the textarea strips CR on every insert path, so keeping CR
        // in the buffer would produce mixed endings after an edit.
        let crlf = content.contains("\r\n");
        let normalized = if crlf {
            content.replace("\r\n", "\n")
        } else {
            content
        };
        let textarea = ratatui_textarea::TextArea::from(
            normalized
                .split('\n')
                .map(str::to_string)
                .collect::<Vec<_>>(),
        );
        self.edit = Some(EditState {
            pane_index,
            path: list_path,
            fs_path,
            mtime_at_open,
            permissions: metadata.permissions(),
            textarea,
            dirty: false,
            read_only: metadata.permissions().readonly(),
            crlf,
        });
    }

    /// Discards the editor (unsaved changes are lost — documented MVP).
    pub fn close_edit(&mut self) {
        self.edit = None;
    }

    /// Forwards a key event to the editor buffer; reports whether the
    /// buffer changed (drives the dirty marker). Read-only editors accept
    /// movement keys (so the head can be scrolled) but no modifications.
    pub fn edit_input(&mut self, key: ratatui::crossterm::event::KeyEvent) -> bool {
        use ratatui::crossterm::event::KeyCode;
        let Some(edit) = &mut self.edit else {
            return false;
        };
        if edit.read_only {
            return match key.code {
                KeyCode::Up
                | KeyCode::Down
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::PageUp
                | KeyCode::PageDown => edit.textarea.input(key),
                _ => false,
            };
        }
        // The buffer is LF-normalized (CRLF re-encoded on save), so every
        // key — Enter included — flows through unchanged.
        let modified = edit.textarea.input(key);
        if modified {
            edit.dirty = true;
        }
        modified
    }

    /// Writes the edited buffer back atomically (temp + rename), preserving
    /// permissions, guarding against on-disk changes, and refreshing the
    /// pane entry + caches so previews re-read the saved content.
    pub fn save_edit(&mut self) {
        let name = self
            .edit
            .as_ref()
            .map(|e| {
                std::path::Path::new(&e.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let Some(edit) = &mut self.edit else {
            return;
        };
        if edit.read_only {
            self.set_status(format!("{name} is read-only"), true);
            return;
        }
        // Path-identity guard: if the listed path was a symlink that has
        // been retargeted since open, the canonical path no longer matches
        // the one we snappedshotted — refuse rather than write a file the
        // user is no longer looking at.
        match std::fs::canonicalize(&edit.path) {
            Ok(current) if current == edit.fs_path => {}
            _ => {
                self.set_status("file path changed on disk — press Esc and reopen", true);
                return;
            }
        }
        // On-disk change guard: refuse to clobber a file modified elsewhere.
        // Full-precision SystemTime comparison — a seconds-truncated guard
        // misses external edits made within the same second. An unknown
        // open-time mtime skips the guard rather than blocking all saves.
        let current_mtime = std::fs::metadata(&edit.fs_path)
            .ok()
            .and_then(|m| m.modified().ok());
        if let (Some(expected), Some(actual)) = (edit.mtime_at_open, current_mtime) {
            if expected != actual {
                self.set_status("file changed on disk — press Esc and reopen", true);
                return;
            }
        }
        let mut content = edit.textarea.lines().join("\n");
        if edit.crlf {
            content = content.replace('\n', "\r\n");
        }
        // Owned copies for the I/O step: no borrows across the writes.
        let fs_path = edit.fs_path.clone();
        let permissions = edit.permissions.clone();
        let pane_index = edit.pane_index;
        let list_path = edit.path.clone();
        let tmp = std::path::PathBuf::from(format!("{}.ira-tmp", fs_path.display()));
        let write = || -> std::io::Result<()> {
            std::fs::write(&tmp, content.as_bytes())?;
            std::fs::set_permissions(&tmp, permissions)?;
            std::fs::rename(&tmp, &fs_path)
        };
        if let Err(e) = write() {
            let _ = std::fs::remove_file(&tmp);
            self.set_status(format!("save failed: {e}"), true);
            return;
        }
        // Refresh what we know about the file: editor guard, pane entry,
        // and the text cache (keys are mtime/size based; evict this file's
        // entries by path).
        let (new_mtime, new_size) = match std::fs::metadata(&fs_path) {
            Ok(m) => (m.modified().ok(), m.len()),
            Err(_) => (None, 0),
        };
        edit.mtime_at_open = new_mtime;
        edit.dirty = false;
        if let Some(entry) = self.panes[pane_index]
            .files
            .iter_mut()
            .find(|f| f.path == list_path)
        {
            entry.size = new_size;
            entry.modified = new_mtime
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
        }
        self.text_cache.retain(|_, tp| tp.path != list_path);
        self.set_status(format!("Saved {name}"), false);
    }

    /// Renders the head of the active pane's selected TEXT file natively in
    /// the preview column (no protocol, no thumbnail). `None` = loading or
    /// not applicable; the UI shows a placeholder until a later frame.
    pub fn text_preview(&mut self, pane_index: usize) -> Option<&TextPreview> {
        self.pick_up_thumbnails();
        let pane = &self.panes[pane_index];
        if pane.preview_mode != PreviewMode::Column {
            return None;
        }
        let entry = self.selected_visible_entry_for(pane_index)?;
        if entry.is_dir || preview_kind(&entry.path) != Some(PreviewKind::Text) {
            return None;
        }
        let req = ThumbRequest {
            path: entry.path.clone(),
            mtime: entry.modified,
            size: entry.size,
            cols: 0,
            rows: 0,
        };
        let key = req.mem_key();
        if let Some(preview) = self.text_cache.get(&key) {
            return Some(preview);
        }
        if self.picker.is_none()
            || self.thumb_failed.contains(&key)
            || self.thumb_pending.contains(&key)
        {
            return None;
        }
        if self.thumb_jobs_hi.try_send(req).is_ok() {
            self.thumb_pending.insert(key);
        }
        None
    }

    /// Enqueues a prefetch request on the low-priority queue (deduped by
    /// `thumb_pending`; dropped when the queue is full, retried on a later
    /// frame). Never blocks the caller.
    fn try_send_prefetch(&mut self, req: &ThumbRequest) {
        let key = req.mem_key();
        if self.picker.is_none()
            || self.thumb_failed.contains(&key)
            || self.thumb_pending.contains(&key)
            || self.thumb_cache.contains_key(&key)
        {
            return;
        }
        if self.thumb_jobs_lo.try_send(req.clone()).is_ok() {
            self.thumb_pending.insert(key);
        }
    }

    /// Frame start for the thumbnail cache: lowers the effective eviction
    /// cap back to the minimum; the grid renderer raises it again to cover
    /// its working set (visible + prefetched cells).
    pub fn begin_thumb_cache_frame(&mut self) {
        self.thumb_cache_cap = THUMB_CACHE_CAP_MIN;
    }

    /// Raises the effective eviction cap if `min` is larger (never shrinks
    /// within a frame; two grid panes both get their working set covered).
    pub fn raise_thumb_cache_cap(&mut self, min: usize) {
        self.thumb_cache_cap = self.thumb_cache_cap.max(min);
    }

    /// Whether `path` can be previewed at all: images always, videos and
    /// HEIC only when the optional `ffmpeg` runtime dependency is present.
    pub fn preview_supported(&self, path: &str) -> bool {
        use std::sync::atomic::Ordering;
        match preview_kind(path) {
            Some(PreviewKind::Image | PreviewKind::Text) => true,
            Some(PreviewKind::Video | PreviewKind::Heic) => {
                self.ffmpeg_available.load(Ordering::Relaxed)
            }
            Some(PreviewKind::Pdf) => self.pdftoppm_available.load(Ordering::Relaxed),
            None => false,
        }
    }

    /// Dispatches thumbnail jobs for grid cells just outside the visible
    /// window (prefetch): by the time the user scrolls there, the protocols
    /// are usually cached. Deduped by `thumb_pending`, capped by the job
    /// queue; the visible cells are always enqueued first (same frame), so
    /// prefetch never starves them.
    pub fn prefetch_grid_cells(
        &mut self,
        pane_index: usize,
        entries: &[FEntry],
        cols: u16,
        rows: u16,
    ) {
        if self.panes[pane_index].preview_mode != PreviewMode::Grid {
            return;
        }
        for entry in entries {
            if entry.is_dir || !self.preview_supported(&entry.path) {
                continue;
            }
            let req = ThumbRequest {
                path: entry.path.clone(),
                mtime: entry.modified,
                size: entry.size,
                cols,
                rows,
            };
            let _ = self.preview_protocol(&req);
        }
    }

    /// Dispatches thumbnail jobs for the entries near the column preview's
    /// selection: `ahead` rows below and 2 above the cursor in visible
    /// space, so scrolling the list previews the next images instantly.
    pub fn prefetch_column(&mut self, pane_index: usize, ahead: usize) {
        let (mode, area, sel) = {
            let pane = &self.panes[pane_index];
            (pane.preview_mode, pane.preview_area, pane.state.selected())
        };
        if mode != PreviewMode::Column || area.0 == 0 {
            return;
        }
        let sel = sel.unwrap_or(0);
        // Materialize the prefetch candidates (owned) before dispatching:
        // `preview_protocol` takes `&mut self`.
        let candidates: Vec<FEntry> = {
            let vis = self.pane_visible_rows(pane_index);
            let start = sel.saturating_sub(2);
            vis.iter()
                .skip(start)
                .take(ahead + 2 + (sel - start))
                .filter(|(vis_idx, entry)| *vis_idx != sel && !entry.is_dir)
                .map(|(_, e)| (*e).clone())
                .collect()
        };
        for entry in &candidates {
            if !self.preview_supported(&entry.path) {
                continue;
            }
            if preview_kind(&entry.path) == Some(PreviewKind::Text) {
                continue; // cell-native, no thumbnail job
            }
            let req = ThumbRequest {
                path: entry.path.clone(),
                mtime: entry.modified,
                size: entry.size,
                cols: area.0,
                rows: area.1,
            };
            self.try_send_prefetch(&req);
        }
    }

    /// Whether a modal overlay (dialogs centered over the files area) is
    /// open. Graphics-protocol previews (sixel/iTerm2) live in a layer above
    /// the cell grid, so they must be suppressed while a dialog is shown —
    /// clearing cells does not clear the graphic.
    pub fn overlay_covers_preview(&self) -> bool {
        self.confirming.is_some()
            || self.renaming.is_some()
            || self.goto_prompt.is_some()
            || self.new_entry.is_some()
            || self.multi_info.is_some()
            || self.info.is_some()
            || self.deletion_box_visible()
    }

    /// Cycles the image preview presentation of the ACTIVE pane (`v`):
    /// off → column → grid. Modes are per pane — switching panes with Tab
    /// never changes either pane's mode.
    pub fn cycle_preview(&mut self) {
        let next = match self.pane().preview_mode {
            PreviewMode::Off => PreviewMode::Column,
            PreviewMode::Column => PreviewMode::Grid,
            PreviewMode::Grid => PreviewMode::Off,
        };
        self.pane_mut().preview_mode = next;
        let label = match next {
            PreviewMode::Off => "off",
            PreviewMode::Column => "column",
            PreviewMode::Grid => "grid",
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
        let indices = if self.is_searching() {
            self.search_matches()
        } else if pane.filter_query.is_some() {
            pane.filter_indices.clone()
        } else {
            (0..pane.files.len()).collect::<Vec<usize>>()
        };
        indices.get(vis).and_then(|&i| pane.files.get(i))
    }

    /// Builds the preview request for the active pane's selected entry, or
    /// `None` when the preview column has nothing to show an image for
    /// (closed, nothing selected, folder, unsupported format).
    pub fn preview_request(&self) -> Option<ThumbRequest> {
        self.preview_request_for(self.active_pane)
    }

    /// Per-pane variant of [`Self::preview_request`].
    pub fn preview_request_for(&self, pane_index: usize) -> Option<ThumbRequest> {
        let pane = &self.panes[pane_index];
        if pane.preview_mode == PreviewMode::Off {
            return None;
        }
        let entry = self.selected_visible_entry_for(pane_index)?;
        if entry.is_dir
            || !self.preview_supported(&entry.path)
            || preview_kind(&entry.path) == Some(PreviewKind::Text)
        {
            return None;
        }
        Some(ThumbRequest {
            path: entry.path.clone(),
            mtime: entry.modified,
            size: entry.size,
            cols: pane.preview_area.0,
            rows: pane.preview_area.1,
        })
    }

    /// Returns the rendered protocol for `req`, dispatching a background
    /// decode+encode job on first sight. `None` means still loading — the
    /// UI shows a placeholder until a later frame. Draining the result
    /// channel here (not only on ticks) makes a finished thumbnail appear
    /// on the next redraw instead of up to a tick later.
    pub fn preview_protocol(&mut self, req: &ThumbRequest) -> Option<&Protocol> {
        self.pick_up_thumbnails();
        let key = req.mem_key();
        if let Some(protocol) = self.thumb_cache.get(&key) {
            return Some(protocol);
        }
        if self.picker.is_none()
            || self.thumb_failed.contains(&key)
            || self.thumb_pending.contains(&key)
            || req.cols == 0
            || req.rows == 0
        {
            return None;
        }
        // Bounded queues: a full queue drops the request (retried on a
        // later frame via `preview_request`), so scrolling never builds an
        // unbounded backlog.
        if self.thumb_jobs_hi.try_send(req.clone()).is_ok() {
            self.thumb_pending.insert(key);
        }
        None
    }

    /// Collects finished thumbnail jobs into the protocol cache.
    fn pick_up_thumbnails(&mut self) {
        while let Ok(event) = self.thumb_rx.try_recv() {
            match event {
                ThumbEvent::Ready(req, protocol) => {
                    self.thumb_pending.remove(&req.mem_key());
                    self.insert_thumb(req.mem_key(), protocol);
                }
                ThumbEvent::Failed(req) => {
                    self.thumb_pending.remove(&req.mem_key());
                    self.thumb_failed.insert(req.mem_key());
                }
                ThumbEvent::Text {
                    req,
                    content,
                    binary,
                    truncated,
                } => {
                    self.thumb_pending.remove(&req.mem_key());
                    self.text_cache.insert(
                        req.mem_key(),
                        TextPreview {
                            path: req.path.clone(),
                            content,
                            binary,
                            truncated,
                        },
                    );
                }
            }
        }
    }
    /// Inserts a finished protocol with FIFO eviction. Protocols are heavy
    /// (kitty payloads are full escape sequences), so the cache is bounded;
    /// the currently displayed entry survives eviction to avoid a visible
    /// re-decode flicker.
    fn insert_thumb(&mut self, key: String, protocol: Protocol) {
        while self.thumb_order.len() >= self.thumb_cache_cap {
            let Some(oldest) = self.thumb_order.pop_front() else {
                break;
            };
            self.thumb_cache.remove(&oldest);
        }
        self.thumb_cache.insert(key.clone(), protocol);
        self.thumb_order.push_back(key);
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

    pub fn set_folder_from_drives(&mut self, initial_shortcut: usize) {
        let Ok(mut drives) = list_drives() else {
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
    pub fn eject_active_drive(&mut self) {
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

    pub fn enter_folder(&mut self) {
        let Some(idx) = self.pane().state.selected() else {
            return;
        };
        let Some(path) = self.visible_entry(idx).map(|f| f.path.clone()) else {
            return;
        };

        if let Ok(Some(actual_folder)) = get_directory(&path) {
            let pane = self.pane_mut();
            pane.filter_query = None;
            pane.filter_indices.clear();
            pane.pending_select = None;
            pane.folder = Some(actual_folder);
            self.search_query = None;
            self.list_files_from_selected_folder();
            self.pane_mut().state.select(None);
        } else if self.visible_entry(idx).map(|f| f.is_dir) == Some(false) {
            // it's a file, just open it
            if let Err(msg) = open_file(&path) {
                self.set_status(msg, true);
            }
        }
    }

    pub fn out_of_folder(&mut self) {
        let Some(current_path) = self.pane().folder.as_ref().map(|f| f.path.clone()) else {
            return;
        };

        match get_parent_directory(&current_path) {
            Ok(Some(folder)) => {
                let pane = self.pane_mut();
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
        let now = Instant::now();
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
        if self.visible_count() > 0 {
            self.pane_mut().state.select(Some(0));
        } else {
            self.pane_mut().state.select(None);
        }
    }

    pub fn goto_bottom(&mut self) {
        self.reset_scroll_ramp();
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
            // Seed the new pane with the current view so both panes start on
            // the same folder.
            if self.panes[1].folder.is_none() {
                self.panes[1].folder = self.panes[0].folder.clone();
                self.panes[1].files = self.panes[0].files.clone();
                self.panes[1].state.select(None);
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
            if self.edit.is_some() {
                self.edit_focus = true;
                return;
            }
        }
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
        let Some(dir) = self.pane().folder.as_ref().map(|f| f.path.clone()) else {
            self.set_status("No folder open to start a terminal in.", true);
            return;
        };
        for (program, args) in terminal_candidates(&dir) {
            if !which(&program) {
                continue;
            }
            let mut cmd = std::process::Command::new(&program);
            cmd.args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .current_dir(&dir);
            match cmd.spawn() {
                Ok(_) => return, // success is visible: the window opened
                Err(_) => continue,
            }
        }
        self.set_status(
            "No terminal emulator found (tried foot, alacritty, kitty, gnome-terminal, konsole, xfce4-terminal, xterm).",
            true,
        );
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
    pub fn confirm_goto(&mut self) {
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
        let Some(folder) = self.pane().folder.as_ref().map(|f| f.path.clone()) else {
            return;
        };
        if crate::services::clipboard::copy_text(&folder) {
            self.set_status("Folder path copied to clipboard", false);
        } else {
            self.set_status("Failed to copy the folder path to the clipboard.", true);
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
    pub fn confirm_new_entry(&mut self) {
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
        let any_unselected = indices.iter().any(|&i| !self.pane().selected[i]);
        for &i in &indices {
            self.pane_mut().selected[i] = any_unselected;
        }
    }

    /// Inverts the multi-selection within the visible set.
    /// Bound to Super+I.
    pub fn invert_selection(&mut self) {
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
    pub fn commit_rename(&mut self) {
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
            started: Instant::now(),
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
            started: Instant::now(),
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
                started: Instant::now(),
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
            match event {
                InfoEvent::Progress {
                    path,
                    bytes,
                    items,
                    on_disk,
                } => {
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
                            dialog.pending = false;
                            dialog.lines = lines;
                        }
                    }
                }
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
            started_at: Instant::now(),
            control: JobControl::new(),
        });
        let job = self.jobs.last().unwrap();
        spawn_job(job, self.job_tx.clone());

        self.transfer_dest = Some(TransferDestSync {
            dest_dir: dest.clone(),
            reveal_path: reveal.to_string_lossy().into_owned(),
            last_refresh: Instant::now(),
        });

        // The selection is handled; drop it and focus the Copy Board.
        self.pane_mut().selected.fill(false);
        self.copy_board = true;
        self.board_focused = true;
        self.copy_board_state.select(Some(self.jobs.len() - 1));
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
        self.deleting_paths = confirm.paths.iter().cloned().collect();
        let tx = self.job_tx.clone();
        let control = spawn_delete_job(confirm.paths.clone(), tx);
        self.deletion = Some(DeletionState {
            total: confirm.paths.len(),
            done: 0,
            current: None,
            started: Instant::now(),
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
            match event {
                JobEvent::Started { id, total_bytes } => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        j.total_bytes = total_bytes;
                    }
                }
                JobEvent::Progress {
                    id,
                    copied_bytes,
                    current,
                } => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        j.copied_bytes = copied_bytes;
                        j.current = current;
                    }
                }
                JobEvent::Done { id } => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        j.status = JobStatus::Done;
                        self.transfer_dest = None;
                        // Land the destination pane's cursor on the last
                        // item of the batch so the new content is visible
                        // without hunting for it.
                        if let Some(last) = j.paths.last() {
                            let dest_item = std::path::Path::new(&j.dest_dir)
                                .join(std::path::Path::new(last).file_name().unwrap_or_default());
                            if let Some(pane) = self
                                .panes
                                .iter_mut()
                                .find(|p| p.folder.as_ref().is_some_and(|f| f.path == j.dest_dir))
                            {
                                pane.pending_select =
                                    Some(dest_item.to_string_lossy().into_owned());
                            }
                        }
                    }
                    self.refresh_after_job();
                }
                JobEvent::Cancelled { id } => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        j.status = JobStatus::Cancelled;
                    }
                    self.refresh_after_job();
                }
                JobEvent::Failed { id, error } => {
                    if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
                        j.status = JobStatus::Failed(error);
                    }
                }
                JobEvent::DeleteProgress {
                    done,
                    total,
                    current,
                } => {
                    if let Some(d) = self.deletion.as_mut() {
                        d.done = done;
                        d.total = total;
                        d.current = Some(current.clone());
                    }
                    self.deleting_paths.remove(&current);
                }
                JobEvent::DeleteDone { cancelled, failed } => {
                    self.deletion = None;
                    self.deleting_paths.clear();
                    self.deletion_box_hidden = false;
                    if !cancelled {
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
        // The destination folder appears once the first item starts copying.
        if std::fs::metadata(&sync.dest_dir).is_err() {
            return;
        }
        if sync.last_refresh.elapsed() < Duration::from_secs(1) {
            return;
        }
        for i in 0..self.panes.len() {
            let viewing = self.panes[i].folder.as_ref().is_some_and(|f| {
                f.path == sync.dest_dir || f.path.starts_with(&format!("{}/", sync.dest_dir))
            });
            if !viewing {
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
        if let Some(s) = &mut self.transfer_dest {
            s.last_refresh = Instant::now();
        }
    }

    /// Whether fuzzy search within the current folder is active.
    pub fn is_searching(&self) -> bool {
        self.search_query.is_some()
    }

    /// Indices into the active pane's files matching the current query, best match first.
    fn search_matches(&self) -> Vec<usize> {
        let query = self.search_query.as_deref().unwrap_or("");
        let labels: Vec<String> = self.pane().files.iter().map(|f| f.label.clone()).collect();
        fuzzy_indices(&labels, query)
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
        self.visible_indices().get(visible_idx).copied()
    }

    pub fn visible_count(&self) -> usize {
        self.visible_indices().len()
    }

    /// Maps a visible-list index to the underlying file entry.
    fn visible_entry(&self, visible_idx: usize) -> Option<&FEntry> {
        let files = &self.pane().files;
        self.visible_indices()
            .get(visible_idx)
            .and_then(|&i| files.get(i))
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
                pane.filter_indices = fuzzy_indices(&labels, &q);
                pane.filter_query = Some(q);
            }
            _ => {
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
    fn load_bookmarks(&mut self) {
        let pairs = read_bookmarks();
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
            write_bookmarks(bookmarks);
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
    pub fn restore_state(&mut self) {
        let state = match &self.state_path {
            Some(p) => load_state_from(p),
            None => load_state(),
        };
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
            sizes: self.size_entries(),
        };
        match &self.state_path {
            Some(p) => save_state_to(p, &state),
            None => save_state(&state),
        }
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::file_info::DirSize;

    fn entry(path: &str) -> FEntry {
        FEntry {
            path: path.to_string(),
            label: path.rsplit('/').next().unwrap_or(path).to_string(),
            is_dir: false,
            size: 0,
            modified: None,
        }
    }
    use crate::services::state::{load_state_from, save_state_to};

    fn dialog_for(path: &str) -> InfoDialog {
        InfoDialog {
            lines: vec!["Name: x".to_string()],
            path: path.to_string(),
            pending: true,
            started: Instant::now(),
        }
    }

    fn partial(path: &str, bytes: u64, items: u64) -> (String, SizeInfo) {
        (
            path.to_string(),
            SizeInfo {
                bytes,
                items,
                on_disk: 0,
                complete: false,
                updated: SystemTime::now(),
            },
        )
    }

    #[test]
    fn progress_updates_cache_and_done_finalizes() {
        let mut app = App::default();
        app.state_path =
            Some(std::env::temp_dir().join(format!("ira-test-done-{}", std::process::id())));
        app.info = Some(dialog_for("/big"));
        let (p, si) = partial("/big", 512, 100);
        app.size_cache.insert(p, si);
        let handle = WalkHandle::new();
        app.size_walks.insert(
            "/big".to_string(),
            WalkSlot {
                handle,
                started: Instant::now(),
            },
        );

        app.info_tx
            .send(InfoEvent::Progress {
                path: "/big".into(),
                bytes: 1024,
                items: 200,
                on_disk: 4096,
            })
            .unwrap();
        app.tick();
        let si = app.size_info("/big").unwrap();
        assert!(!si.complete && si.bytes == 1024 && si.items == 200);
        assert!(app.size_walk_started("/big").is_some());

        app.info_tx
            .send(InfoEvent::Done {
                path: "/big".into(),
                size: DirSize {
                    bytes: 4096,
                    items: 800,
                    on_disk: 8192,
                },
            })
            .unwrap();
        app.tick();
        let si = app.size_info("/big").unwrap();
        assert!(si.complete && si.bytes == 4096 && si.items == 800);
        assert!(app.size_walk_started("/big").is_none(), "walk slot removed");
    }

    #[test]
    fn done_inserts_final_size_line_into_open_dialog() {
        let mut app = App::default();
        app.state_path =
            Some(std::env::temp_dir().join(format!("ira-test-done2-{}", std::process::id())));
        app.info = Some(dialog_for("/big"));
        app.info_tx
            .send(InfoEvent::Meta {
                path: "/big".into(),
                lines: vec!["Name: big".into(), "Added: x".into()],
            })
            .unwrap();
        app.info_tx
            .send(InfoEvent::Done {
                path: "/big".into(),
                size: DirSize {
                    bytes: 4096,
                    items: 800,
                    on_disk: 8192,
                },
            })
            .unwrap();
        app.tick();
        let d = app.info.as_ref().unwrap();
        assert!(!d.pending);
        let size_line = d.lines.iter().find(|l| l.starts_with("Size:")).unwrap();
        assert!(
            size_line.contains("800 items") && size_line.contains("4.0 KiB data / 8.0 KiB on disk"),
            "{size_line:?}"
        );
    }

    #[test]
    fn meta_for_other_paths_is_ignored() {
        let mut app = App::default();
        app.info = Some(dialog_for("/big"));
        app.info_tx
            .send(InfoEvent::Meta {
                path: "/other".into(),
                lines: vec!["Name: other".into()],
            })
            .unwrap();
        app.tick();
        let d = app.info.as_ref().unwrap();
        assert!(d.pending, "stale Meta must not fill the dialog");
        assert_eq!(d.lines, vec!["Name: x".to_string()]);
    }

    #[test]
    fn close_info_keeps_the_background_walk_running() {
        let mut app = App::default();
        app.info = Some(dialog_for("/big"));
        let handle = WalkHandle::new();
        app.size_walks.insert(
            "/big".to_string(),
            WalkSlot {
                handle: handle.clone(),
                started: Instant::now(),
            },
        );
        app.close_info();
        assert!(app.info.is_none());
        assert!(!handle.cancelled(), "dismissal must NOT stop the walk");
        assert!(app.size_walk_started("/big").is_some());
    }

    #[test]
    fn cancel_dialog_size_walk_stops_measurement() {
        let mut app = App::default();
        app.info = Some(dialog_for("/big"));
        let handle = WalkHandle::new();
        app.size_walks.insert(
            "/big".to_string(),
            WalkSlot {
                handle: handle.clone(),
                started: Instant::now(),
            },
        );
        app.cancel_dialog_size_walk();
        assert!(handle.cancelled(), "x must stop the walk");
        assert!(app.size_walk_started("/big").is_none());
    }
    #[test]
    fn events_after_dialog_closed_are_dropped() {
        let mut app = App::default();
        app.info_tx
            .send(InfoEvent::Meta {
                path: "/big".into(),
                lines: vec!["Name: big".into()],
            })
            .unwrap();
        app.tick(); // no dialog open: must not panic nor reopen one
        assert!(app.info.is_none());
    }

    #[test]
    fn recalculate_dialog_size_drops_cache_and_restarts_walk() {
        let mut app = App::default();
        app.info = Some(dialog_for("/big"));
        let (p, si) = partial("/big", 512, 100);
        app.size_cache.insert(p, si);
        let handle = WalkHandle::new();
        app.size_walks.insert(
            "/big".to_string(),
            WalkSlot {
                handle,
                started: Instant::now(),
            },
        );

        app.recalculate_dialog_size();

        assert!(
            app.size_info("/big").is_none(),
            "cached measurement dropped"
        );
        assert!(
            app.size_walk_started("/big").is_some(),
            "fresh walk started"
        );
        let d = app.info.as_ref().unwrap();
        assert!(d.pending, "dialog back to pending fast lines");
        assert_eq!(d.lines[0], "Name: big");
    }

    #[test]
    fn size_cache_survives_a_simulated_restart() {
        let mut app = App::default();
        app.size_cache.insert(
            "/big".to_string(),
            SizeInfo {
                bytes: 4096,
                items: 800,
                on_disk: 8192,
                complete: true,
                updated: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            },
        );
        app.size_cache.insert(
            "/wip".to_string(),
            SizeInfo {
                bytes: 1,
                items: 1,
                on_disk: 0,
                complete: false,
                updated: SystemTime::UNIX_EPOCH,
            },
        );

        // "Save": complete measurements only go to disk.
        let entries = app.size_entries();
        assert_eq!(entries.len(), 1, "partials must not persist");
        assert_eq!(entries[0].path, "/big");

        // A fresh app restores them (simulated restart via restore_sizes,
        // the same path App::new takes through restore_state).
        let mut app2 = App::default();
        app2.restore_sizes(entries.clone());
        let si = app2.size_info("/big").unwrap();
        assert!(si.complete && si.bytes == 4096 && si.items == 800 && si.on_disk == 8192);
        assert_eq!(
            si.updated,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
        );
        assert!(app2.size_info("/wip").is_none());

        // Full file roundtrip through an explicit path (no touching the
        // real config file).
        let file = std::env::temp_dir().join(format!("ira-test-state-{}", std::process::id()));
        save_state_to(
            &file,
            &SessionState {
                split: app.split,
                active_pane: app.active_pane,
                show_hidden: app.show_hidden,
                left: app.panes[0].folder.clone(),
                right: app.panes[1].folder.clone(),
                preview: [
                    app.panes[0].preview_mode.as_u8(),
                    app.panes[1].preview_mode.as_u8(),
                ],
                sizes: entries,
            },
        );
        let loaded = load_state_from(&file);
        let _ = std::fs::remove_file(&file);
        let mut app3 = App::default();
        app3.restore_sizes(loaded.sizes);
        let si = app3.size_info("/big").unwrap();
        assert!(si.complete && si.bytes == 4096 && si.items == 800);
        assert_eq!(
            si.updated,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
        );
    }
    #[test]
    fn stale_listing_chunks_from_an_older_generation_are_dropped() {
        let mut app = App::default();
        // Simulate: pane starts listing gen 1, user navigates, gen 2 starts.
        app.panes[0].listing_generation = 2;
        app.panes[0].files.clear();

        // A late chunk from gen 1 must be dropped entirely (this is the
        // "folder lists itself" bug: interleaved chunks of two runs).
        app.file_list_tx
            .send((0, vec![entry("/stale/old")], false, 1))
            .unwrap();
        app.tick();
        assert!(
            app.panes[0].files.is_empty(),
            "stale-generation chunks must not appear"
        );

        // A chunk from the current generation lands normally.
        app.file_list_tx
            .send((0, vec![entry("/new/a")], false, 2))
            .unwrap();
        app.tick();
        assert_eq!(app.panes[0].files.len(), 1);

        // A gen-2 done replaces the streamed prefix.
        app.file_list_tx
            .send((0, vec![entry("/new/a"), entry("/new/b")], true, 2))
            .unwrap();
        app.tick();
        assert!(app.file_list_settled(0));
        assert_eq!(app.panes[0].files.len(), 2);

        // A late done from gen 1 is still dropped.
        app.file_list_tx
            .send((0, vec![entry("/stale/z")], true, 1))
            .unwrap();
        app.tick();
        assert_eq!(app.panes[0].files.len(), 2);
    }

    #[test]
    fn small_folders_list_synchronously_big_folders_stream() {
        let base = std::env::temp_dir().join(format!("ira_sync_async_{}", std::process::id()));
        let small = base.join("small");
        let big = base.join("big");
        std::fs::create_dir_all(&small).unwrap();
        std::fs::create_dir_all(&big).unwrap();
        std::fs::write(small.join("a.txt"), "x").unwrap();
        std::fs::write(small.join("b.txt"), "x").unwrap();
        for i in 0..LISTING_CHUNK + 5 {
            std::fs::write(big.join(format!("f{i}")), "x").unwrap();
        }

        let folder_of = |p: &std::path::Path| {
            Folder::new("t".to_string(), p.to_string_lossy().into_owned(), '#')
        };

        // Small folder: bounded sync read — settled immediately, sorted, no
        // worker involved (listing_settled true right after the call).
        let mut app = App::default();
        app.panes[0].folder = Some(folder_of(&small));
        app.list_files_from_selected_folder();
        assert!(
            app.file_list_settled(0),
            "small folder must list synchronously"
        );
        let labels: Vec<String> = app.panes[0].files.iter().map(|f| f.label.clone()).collect();
        assert_eq!(labels, vec!["a.txt", "b.txt"]);

        // Big folder: pane is cleared for streaming, NOT settled, and the
        // sorted list arrives only after ticks.
        app.panes[0].folder = Some(folder_of(&big));
        app.list_files_from_selected_folder();
        assert!(
            !app.file_list_settled(0),
            "big folder must stream in background"
        );
        let mut got = false;
        for _ in 0..250 {
            app.tick();
            if app.file_list_settled(0) {
                got = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(got, "big folder must settle via the background worker");
        assert_eq!(app.panes[0].files.len(), LISTING_CHUNK + 5);

        let _ = std::fs::remove_dir_all(&base);
    }
    #[test]
    fn any_key_dismisses_the_error_dialog() {
        let mut app = App::default();
        app.set_status("Failed to eject /dev/sdd2: target is busy", true);
        assert!(app.status.is_some());

        // clear_status is what the handler calls on any key.
        app.clear_status();
        assert!(app.status.is_none());
    }
}

/// Terminal emulators to try for `0`, in preference order, with the args
/// that make them open in `dir`. Pure so tests can inspect the table.
pub fn terminal_candidates(dir: &str) -> Vec<(String, Vec<String>)> {
    vec![
        (
            "foot".into(),
            vec!["--working-directory".into(), dir.into()],
        ),
        (
            "alacritty".into(),
            vec!["--working-directory".into(), dir.into()],
        ),
        ("kitty".into(), vec!["--directory".into(), dir.into()]),
        (
            "gnome-terminal".into(),
            vec![format!("--working-directory={dir}")],
        ),
        ("konsole".into(), vec!["--workdir".into(), dir.into()]),
        (
            "xfce4-terminal".into(),
            vec!["--working-directory".into(), dir.into()],
        ),
        ("xterm".into(), vec![]),
    ]
}

/// True when `program` is an executable file found on `PATH`
/// (existence on the path is treated as sufficient).
fn which(program: &str) -> bool {
    let Ok(path_var) = std::env::var("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| {
        let candidate = dir.join(program);
        candidate.is_file()
    })
}

/// Joins Unicode characters into a `String`.
fn chars_to_string(chars: &[char]) -> String {
    let mut s = String::new();
    for c in chars {
        s.push(*c);
    }
    s
}

/// Opens a file with the system default application without blocking the TUI.
///
/// On Linux, `gio open` is preferred over `xdg-open`: xdg-open (>= 1.2) does
/// not honor `Terminal=true` desktop entries, so terminal-based editors (e.g.
/// Neovim) launch without a controlling terminal and appear to do nothing.
/// `gio open` handles terminal apps correctly.
fn open_file(path: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        use std::process::{Command, Stdio};
        let launched = Command::new("gio")
            .args(["open", path])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok();
        if launched {
            return Ok(());
        }
    }

    that_detached(path).map_err(|err| format!("Failed to open '{path}': {err}"))
}

#[test]
fn matching_drive_picks_longest_mount_prefix() {
    let mut drives = vec![
        Folder::new("root".to_string(), "/run/media/vlad".to_string(), '1'),
        Folder::new(
            "usb".to_string(),
            "/run/media/vlad/USB STICK".to_string(),
            '2',
        ),
        Folder::new("unmounted".to_string(), String::new(), '3'),
    ];
    // The drive with a device set and the longest matching mount point wins.
    drives[0].device = Some("/dev/sda1".to_string());
    drives[1].device = Some("/dev/sdb1".to_string());
    drives[2].device = Some("/dev/sdz1".to_string());

    let hit = matching_drive(&drives, "/run/media/vlad/USB STICK/photos").unwrap();
    assert_eq!(hit.label, "usb");

    // A folder outside every mount point has no drive.
    assert!(matching_drive(&drives, "/home/vlad").is_none());
    // An empty mount point never matches by prefix.
    let none = vec![Folder {
        path: String::new(),
        label: "x".to_string(),
        shortcut: '1',
        device: Some("/dev/x".to_string()),
    }];
    assert!(matching_drive(&none, "/anything").is_none());
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    use crate::handler::handle_key_events;
    use image::DynamicImage;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// Writes a small PNG and returns its path.
    fn png_at(name: &str) -> String {
        let dir = std::env::temp_dir().join("ira_preview_tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_fn(64, 32, |x, y| {
            image::Rgb([x as u8, y as u8, 0])
        }));
        img.save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        path.to_string_lossy().into_owned()
    }

    fn app_with_selection(path: &str) -> App {
        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].preview_area = (38, 10);
        app.panes[0].files = vec![FEntry {
            path: path.to_string(),
            label: "img.png".to_string(),
            is_dir: false,
            size: 0,
            modified: None,
        }];
        app.panes[0].state.select(Some(0));
        app
    }

    #[test]
    fn cycle_preview_walks_modes_per_pane() {
        let mut app = App::default();
        assert_eq!(app.panes[0].preview_mode, PreviewMode::Off);
        app.cycle_preview();
        assert_eq!(app.panes[0].preview_mode, PreviewMode::Column);
        app.cycle_preview();
        assert_eq!(app.panes[0].preview_mode, PreviewMode::Grid);
        app.cycle_preview();
        assert_eq!(app.panes[0].preview_mode, PreviewMode::Off);
    }

    #[test]
    fn grid_working_set_is_covered_by_the_cache_cap() {
        // The flashing bug: at 81 visible cells the working set (visible +
        // one prefetched screen above and below = 3 * per_screen) exceeded
        // the fixed cap, so prefetch evicted visible entries and they
        // re-decoded every frame.
        let paths: Vec<String> = (0..12).map(|i| png_at(&format!("cap{i}.png"))).collect();
        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        app.panes[0].preview_mode = PreviewMode::Grid;
        app.panes[0].folder = Some(Folder::new(
            "cap".into(),
            std::env::temp_dir().to_string_lossy().into_owned(),
            '1',
        ));
        app.panes[0].files = paths
            .iter()
            .map(|p| FEntry {
                path: p.clone(),
                label: p.rsplit('/').next().unwrap_or(p).to_string(),
                is_dir: false,
                size: 0,
                modified: None,
            })
            .collect();
        app.panes[0].listing_settled = true;
        app.panes[0].state.select(Some(0));

        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| crate::components::tab1_files_ui::render(f, &mut app, f.area(), 0, true))
            .unwrap();
        let per_screen = 5 * 4; // 78/14 cols x 22/5 rows on an 80x24 backend
        assert!(
            app.thumb_cache_cap >= 3 * per_screen + 16,
            "cache cap {} does not cover grid working set {}",
            app.thumb_cache_cap,
            3 * per_screen + 16
        );
    }

    #[test]
    fn failed_thumbnails_stay_blacklisted_for_the_session() {
        let mut app = app_with_selection("/nonexistent/missing.png");
        let req = app.preview_request().unwrap();
        assert!(app.preview_protocol(&req).is_none());
        for _ in 0..500 {
            let _ = app.preview_protocol(&req);
            if app.thumb_failed.contains(&req.mem_key()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            app.thumb_failed.contains(&req.mem_key()),
            "failure was not blacklisted"
        );
        // No cooldown retry: the broken file must never re-dispatch.
        for _ in 0..20 {
            let _ = app.preview_protocol(&req);
        }
        assert!(
            !app.thumb_pending.contains(&req.mem_key()),
            "failed file re-dispatched"
        );
    }

    #[test]
    fn preview_modes_are_independent_per_pane() {
        let mut app = App::default();
        app.split = true; // Tab moves focus between panes only when split
        app.cycle_preview(); // pane 0 -> Column
        app.switch_pane(); // focus pane 1
        app.cycle_preview();
        app.cycle_preview(); // pane 1 -> Grid
                             // Switching focus never altered the other pane's mode.
        assert_eq!(app.panes[0].preview_mode, PreviewMode::Column);
        assert_eq!(app.panes[1].preview_mode, PreviewMode::Grid);
    }

    #[test]
    fn preview_supported_gates_video_on_ffmpeg_probe() {
        use std::sync::atomic::AtomicBool;
        let mut app = App::default();
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].preview_area = (38, 10);
        app.panes[0].files = vec![FEntry {
            path: "/tmp/clip.mp4".into(),
            label: "clip.mp4".into(),
            is_dir: false,
            size: 0,
            modified: None,
        }];
        app.panes[0].state.select(Some(0));

        // Probe pending / ffmpeg absent: video is not previewable.
        assert!(!app.preview_supported("/tmp/clip.mp4"));
        assert!(app.preview_request().is_none());

        // ffmpeg detected: video becomes previewable like any image.
        app.ffmpeg_available = Arc::new(AtomicBool::new(true));
        assert!(app.preview_supported("/tmp/clip.mp4"));
        assert!(app.preview_request().is_some());

        // Images and text never depend on the probe.
        assert!(app.preview_supported("/tmp/x.png"));
        assert!(app.preview_supported("/tmp/x.txt"));
        // PDF is gated by its own probe (pdftoppm), independent of ffmpeg.
        assert!(!app.preview_supported("/tmp/notes.pdf"));
        app.pdftoppm_available = Arc::new(AtomicBool::new(true));
        assert!(app.preview_supported("/tmp/notes.pdf"));
    }
    #[test]
    fn preview_request_resolves_the_visible_row_not_files_index() {
        // Two entries; the filter shows them in reverse order, so visible
        // index 0 is `files[1]`. `state.selected()` is a visible index.
        let png_a = png_at("filter_a.png");
        let png_b = png_at("filter_b.png");
        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].preview_area = (38, 10);
        app.panes[0].files = vec![
            FEntry {
                path: png_a.clone(),
                label: "filter_a.png".into(),
                is_dir: false,
                size: 0,
                modified: None,
            },
            FEntry {
                path: png_b.clone(),
                label: "filter_b.png".into(),
                is_dir: false,
                size: 0,
                modified: None,
            },
        ];
        app.panes[0].filter_query = Some("png".into());
        app.panes[0].filter_indices = vec![1, 0];
        app.panes[0].state.select(Some(0));

        let req = app.preview_request().unwrap();
        assert_eq!(req.path, png_b, "preview resolved the wrong visible row");
    }

    #[test]
    fn preview_request_gates_on_kind_and_visibility() {
        let png = png_at("gate.png");
        let mut app = app_with_selection(&png);
        assert!(app.preview_request().is_some());

        // Folders and non-image extensions never produce a request.
        app.panes[0].files[0].is_dir = true;
        assert!(app.preview_request().is_none());
        app.panes[0].files[0].is_dir = false;
        app.panes[0].files[0].path = "/tmp/notes.txt".into();
        assert!(app.preview_request().is_none());

        // Closed preview: no request even for a valid image.
        app.panes[0].files[0].path = png;
        app.panes[0].preview_mode = PreviewMode::Off;
        assert!(app.preview_request().is_none());
    }

    #[test]
    fn preview_protocol_dispatches_and_becomes_ready() {
        let png = png_at("roundtrip.png");
        let mut app = app_with_selection(&png);
        let req = app.preview_request().unwrap();

        // First call dispatches a worker; "loading" until it finishes.
        assert!(app.preview_protocol(&req).is_none());
        let mut ready = false;
        for _ in 0..500 {
            if app.preview_protocol(&req).is_some() {
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready, "thumbnail never became ready");
    }

    #[test]
    fn failed_decode_does_not_stick_pending() {
        let mut app = app_with_selection("/nonexistent/missing.png");
        let req = app.preview_request().unwrap();
        assert!(app.preview_protocol(&req).is_none());
        // preview_protocol drains finished jobs; failure must clear pending
        // and keep reporting "loading" (None) without re-dispatching.
        let mut settled = false;
        for _ in 0..500 {
            let _ = app.preview_protocol(&req);
            if app.thumb_pending.is_empty() {
                settled = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(settled, "failed job stayed pending");
        assert!(app.preview_protocol(&req).is_none());
    }

    #[test]
    fn queued_requests_all_resolve_through_the_pool() {
        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        let reqs: Vec<ThumbRequest> = (0..6)
            .map(|i| ThumbRequest {
                path: png_at(&format!("pool{i}.png")),
                mtime: None,
                size: 0,
                cols: 38,
                rows: 10,
            })
            .collect();
        for req in &reqs {
            let _ = app.preview_protocol(req);
        }
        // The pool drains the queue even though only ≤4 workers exist.
        let mut done = 0;
        for _ in 0..1000 {
            for r in &reqs {
                let _ = app.preview_protocol(r);
            }
            done = reqs
                .iter()
                .filter(|r| app.thumb_cache.contains_key(&r.mem_key()))
                .count();
            if done == reqs.len() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(done, reqs.len(), "queued requests did not all resolve");
    }

    #[test]
    fn text_preview_roundtrips_through_the_pool() {
        use std::fs;
        let dir = std::env::temp_dir().join("ira_preview_tests");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        fs::write(&path, "first line\nsecond line\n").unwrap();

        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].preview_area = (38, 10);
        app.panes[0].files = vec![FEntry {
            path: path.to_string_lossy().into_owned(),
            label: "note.txt".into(),
            is_dir: false,
            size: fs::metadata(&path).unwrap().len(),
            modified: None,
        }];
        app.panes[0].state.select(Some(0));

        // Text has no protocol request; the cell-native path fetches it.
        assert!(app.preview_request_for(0).is_none());
        let mut ready = false;
        for _ in 0..500 {
            if app.text_preview(0).is_some() {
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready, "text preview never became ready");
        let preview = app.text_preview(0).unwrap();
        assert!(preview.content.starts_with("first line"));
        assert!(!preview.binary && !preview.truncated);
    }

    #[test]
    fn editor_opens_saves_and_invalidates_caches() {
        use std::fs;
        let dir = std::env::temp_dir().join("ira_preview_tests");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("editable.txt");
        fs::write(&path, "hello\n").unwrap();

        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].preview_area = (38, 10);
        app.panes[0].files = vec![FEntry {
            path: path.to_string_lossy().into_owned(),
            label: "editable.txt".into(),
            is_dir: false,
            size: fs::metadata(&path).unwrap().len(),
            modified: None,
        }];
        app.panes[0].state.select(Some(0));

        // Tab opens the editor on the focused pane.
        app.switch_pane();
        assert!(app.edit_focus);
        assert!(app.edit.is_some());
        // Trailing newline is preserved losslessly as an empty last line —
        // join("\n") reproduces the exact original bytes on save.
        assert_eq!(
            app.edit.as_ref().unwrap().textarea.lines(),
            vec!["hello".to_string(), String::new()]
        );

        // Typing mutates the buffer and marks it dirty.
        let key = ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Char('X'),
            ratatui::crossterm::event::KeyModifiers::empty(),
        );
        assert!(app.edit_input(key));
        assert!(app.edit.as_ref().unwrap().dirty);

        // Save writes the buffer to disk, clears dirty, and refreshes the
        // pane entry's mtime/size.
        app.save_edit();
        assert!(!app.edit.as_ref().unwrap().dirty);
        assert_eq!(fs::read_to_string(&path).unwrap(), "Xhello\n");
        let entry = &app.panes[0].files[0];
        assert_eq!(entry.size, fs::metadata(&path).unwrap().len());

        // The text cache was invalidated: a fresh preview re-reads the
        // saved content through the worker.
        let mut ready = false;
        for _ in 0..500 {
            if let Some(p) = app.text_preview(0) {
                assert!(p.content.starts_with("Xhello"));
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready, "saved content never re-read");
    }

    #[test]
    fn s_key_saves_without_inserting_and_esc_discards() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use std::fs;
        let dir = std::env::temp_dir().join("ira_preview_tests");
        let path = dir.join("routing.txt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "line\n").unwrap();

        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].preview_area = (38, 10);
        app.panes[0].files = vec![FEntry {
            path: path.to_string_lossy().into_owned(),
            label: "routing.txt".into(),
            is_dir: false,
            size: 5,
            modified: None,
        }];
        app.panes[0].state.select(Some(0));
        app.switch_pane(); // Tab offers the editor and focuses it
        assert!(app.edit_focus);

        // `q` inserts into the buffer instead of quitting...
        handle_key_events(
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty()),
            &mut app,
        )
        .unwrap();
        assert!(app.running, "q must not quit in edit mode");
        assert_eq!(
            app.edit.as_ref().unwrap().textarea.lines(),
            vec!["qline".to_string(), String::new()]
        );

        // ...`s` saves WITHOUT inserting an `s`...
        handle_key_events(
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::empty()),
            &mut app,
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "qline\n");
        let lines_after_save = app.edit.as_ref().unwrap().textarea.lines().clone();
        assert_eq!(lines_after_save, vec!["qline".to_string(), String::new()]);

        // ...and Esc exits edit mode, leaving the file as saved.
        handle_key_events(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()), &mut app).unwrap();
        assert!(app.edit.is_none() && !app.edit_focus);
        assert_eq!(fs::read_to_string(&path).unwrap(), "qline\n");
    }

    #[test]
    fn read_only_files_open_without_save() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join("ira_preview_tests");
        let path = dir.join("locked.txt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "readonly\n").unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(perms.mode() & !0o200);
        fs::set_permissions(&path, perms.clone()).unwrap();

        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].preview_area = (38, 10);
        app.panes[0].files = vec![FEntry {
            path: path.to_string_lossy().into_owned(),
            label: "locked.txt".into(),
            is_dir: false,
            size: 9,
            modified: None,
        }];
        app.panes[0].state.select(Some(0));
        app.open_edit();
        assert!(app.edit.as_ref().unwrap().read_only);

        // Saving is refused and the file is untouched.
        app.save_edit();
        assert_eq!(fs::read_to_string(&path).unwrap(), "readonly\n");
        assert!(app.status.as_ref().is_some_and(|s| s.is_error));

        // Restore permissions so the temp dir stays reusable.
        perms.set_mode(perms.mode() | 0o200);
        fs::set_permissions(&path, perms).unwrap();
    }

    #[test]
    fn preview_focus_is_only_offered_for_editable_text() {
        use std::fs;
        let dir = std::env::temp_dir().join("ira_preview_tests");
        let png = png_at("nofocus.png");
        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].preview_area = (38, 10);
        app.panes[0].files = vec![FEntry {
            path: png.clone(),
            label: "nofocus.png".into(),
            is_dir: false,
            size: fs::metadata(&png).unwrap().len(),
            modified: None,
        }];
        app.panes[0].state.select(Some(0));

        app.switch_pane();
        assert!(!app.edit_focus, "image preview must not offer the editor");
        assert!(app.edit.is_none());
    }

    #[test]
    fn tab_while_editing_discards_and_leaves_without_reopening() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use std::fs;
        let dir = std::env::temp_dir().join("ira_preview_tests");
        let path = dir.join("trap.txt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "keep\n").unwrap();

        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].preview_area = (38, 10);
        app.panes[0].files = vec![FEntry {
            path: path.to_string_lossy().into_owned(),
            label: "trap.txt".into(),
            is_dir: false,
            size: 5,
            modified: None,
        }];
        app.panes[0].state.select(Some(0));

        app.switch_pane();
        assert!(app.edit_focus);
        // Dirty the buffer, then Tab: the edit must be DISCARDED and the
        // editor must NOT immediately reopen on the same file (the
        // reopen trap made Tab unusable while editing).
        handle_key_events(
            KeyEvent::new(KeyCode::Char('X'), KeyModifiers::empty()),
            &mut app,
        )
        .unwrap();
        app.switch_pane();
        assert!(!app.edit_focus);
        assert!(app.edit.is_none(), "editor reopened after Tab");
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep\n");
        // The next Tab re-offers the editor (normal cycle restored).
        app.switch_pane();
        assert!(app.edit_focus);
    }

    #[test]
    fn save_refuses_after_external_modification() {
        use std::fs;
        let dir = std::env::temp_dir().join("ira_preview_tests");
        let path = dir.join("clobber.txt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "mine\n").unwrap();

        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].preview_area = (38, 10);
        app.panes[0].files = vec![FEntry {
            path: path.to_string_lossy().into_owned(),
            label: "clobber.txt".into(),
            is_dir: false,
            size: 5,
            modified: None,
        }];
        app.panes[0].state.select(Some(0));
        app.switch_pane();

        // Another process overwrites the file AFTER we opened it. The
        // full-precision mtime guard must refuse the save even within the
        // same second.
        fs::write(&path, "theirs\n").unwrap();
        app.save_edit();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "theirs\n",
            "save clobbered an external modification"
        );
        assert!(app.status.as_ref().is_some_and(|s| s.is_error));
    }

    #[test]
    fn paste_reaches_the_editor() {
        use std::fs;
        let dir = std::env::temp_dir().join("ira_preview_tests");
        let path = dir.join("paste.txt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "start\n").unwrap();

        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].preview_area = (38, 10);
        app.panes[0].files = vec![FEntry {
            path: path.to_string_lossy().into_owned(),
            label: "paste.txt".into(),
            is_dir: false,
            size: 6,
            modified: None,
        }];
        app.panes[0].state.select(Some(0));
        app.switch_pane();

        app.handle_paste("PASTED");
        assert!(app.edit.as_ref().unwrap().dirty);
        assert_eq!(
            app.edit.as_ref().unwrap().textarea.lines(),
            vec!["PASTEDstart".to_string(), String::new()]
        );
    }

    #[test]
    fn ctrl_a_does_not_toggle_selection_while_editing() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use std::fs;
        let dir = std::env::temp_dir().join("ira_preview_tests");
        let path = dir.join("ctrl.txt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "buf\n").unwrap();

        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].preview_area = (38, 10);
        app.panes[0].files = vec![FEntry {
            path: path.to_string_lossy().into_owned(),
            label: "ctrl.txt".into(),
            is_dir: false,
            size: 4,
            modified: None,
        }];
        app.panes[0].state.select(Some(0));
        app.switch_pane();

        handle_key_events(
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            &mut app,
        )
        .unwrap();
        assert!(
            app.panes[0].selected.iter().all(|s| !*s),
            "Ctrl+A leaked into pane selection while editing"
        );
        // The keystroke reached the textarea as a movement (Ctrl+A = line
        // start, a no-op at (0,0)) — buffer unchanged, selection untouched.
        assert_eq!(
            app.edit.as_ref().unwrap().textarea.lines(),
            vec!["buf".to_string(), String::new()]
        );
    }

    #[test]
    fn crlf_files_keep_their_endings() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use std::fs;
        let dir = std::env::temp_dir().join("ira_preview_tests");
        let path = dir.join("crlf.txt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "a\r\nb\r\n").unwrap();

        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].preview_area = (38, 10);
        app.panes[0].files = vec![FEntry {
            path: path.to_string_lossy().into_owned(),
            label: "crlf.txt".into(),
            is_dir: false,
            size: fs::metadata(&path).unwrap().len(),
            modified: None,
        }];
        app.panes[0].state.select(Some(0));
        app.switch_pane();
        assert!(app.edit.as_ref().unwrap().crlf);

        // Enter at the top inserts the file's own CRLF ending.
        handle_key_events(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            &mut app,
        )
        .unwrap();
        app.save_edit();
        assert_eq!(fs::read(&path).unwrap(), b"\r\na\r\nb\r\n");
    }

    #[cfg(unix)]
    #[test]
    fn saving_through_a_symlink_updates_the_target() {
        use std::fs;
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join("ira_preview_tests");
        let target = dir.join("symlink_target.txt");
        let link = dir.join("symlink_link.txt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&target, "target\n").unwrap();
        let _ = fs::remove_file(&link);
        symlink(&target, &link).unwrap();

        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].preview_area = (38, 10);
        app.panes[0].files = vec![FEntry {
            path: link.to_string_lossy().into_owned(),
            label: "symlink_link.txt".into(),
            is_dir: false,
            size: 7,
            modified: None,
        }];
        app.panes[0].state.select(Some(0));
        app.switch_pane();

        handle_key_events(
            KeyEvent::new(KeyCode::Char('X'), KeyModifiers::empty()),
            &mut app,
        )
        .unwrap();
        app.save_edit();

        // The link survives and points at the updated target.
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(fs::read_to_string(&target).unwrap(), "Xtarget\n");

        // N3 regression: retarget the link AFTER opening the editor — the
        // save must refuse instead of writing through the stale path.
        let target2 = dir.join("symlink_target2.txt");
        fs::write(&target2, "new\n").unwrap();
        let _ = fs::remove_file(&link);
        symlink(&target2, &link).unwrap();
        app.save_edit();
        assert_eq!(
            fs::read_to_string(&target2).unwrap(),
            "new\n",
            "save wrote through a retargeted symlink"
        );
        assert!(app.status.as_ref().is_some_and(|s| s.is_error));
    }

    #[test]
    fn modal_overlay_suppresses_dispatch_and_render() {
        let png = png_at("overlay.png");
        let mut app = app_with_selection(&png);
        app.confirming = Some(Confirm {
            action: ConfirmAction::Delete,
            policy: OverwritePolicy::default(),
            label: "x".into(),
            paths: vec![],
            dest_dir: None,
        });
        assert!(app.overlay_covers_preview());

        let backend = ratatui::backend::TestBackend::new(50, 20);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| crate::components::preview_ui::render(f, &mut app, f.area(), 0))
            .unwrap();
        assert!(
            app.thumb_pending.is_empty(),
            "dispatched a job while a modal overlay was open"
        );
    }

    #[test]
    fn grid_mode_dispatches_visible_cells() {
        let paths: Vec<String> = (0..12).map(|i| png_at(&format!("grid{i}.png"))).collect();
        let mut app = App::default();
        app.set_picker(ratatui_image::picker::Picker::halfblocks());
        app.panes[0].preview_mode = PreviewMode::Grid;
        app.panes[0].folder = Some(Folder::new(
            "grid".into(),
            std::env::temp_dir().to_string_lossy().into_owned(),
            '1',
        ));
        app.panes[0].files = paths
            .iter()
            .map(|p| FEntry {
                path: p.clone(),
                label: p.rsplit('/').next().unwrap_or(p).to_string(),
                is_dir: false,
                size: 0,
                modified: None,
            })
            .collect();
        app.panes[0].listing_settled = true;
        app.panes[0].state.select(Some(0));

        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| crate::components::tab1_files_ui::render(f, &mut app, f.area(), 0, true))
            .unwrap();
        assert!(
            !app.thumb_pending.is_empty(),
            "grid mode dispatched no thumbnails"
        );
    }
}
