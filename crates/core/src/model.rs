//! UI-neutral state extracted from the frozen terminal oracle.
use crate::cursor::CursorState as ListState;
use crate::domain::data::Folder;
use crate::services::{
    list_files::FEntry,
    transfer::{JobControl, OverwritePolicy},
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
#[derive(Debug, Clone, Default)]
pub struct Pane {
    /// Actor-owned projection version; cursor-only changes do not advance it.
    pub(crate) projection_generation: u64,
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
    /// True once the user moved the cursor in this pane (`↑`/`↓`, `z`/`x`).
    /// While a transfer streams into a folder this pane shows, the live
    /// re-listing then skips this pane entirely, so neither the cursor nor
    /// the scroll window is disturbed mid-browse. Reset when a new transfer
    /// arms, so the incoming item is revealed again.
    pub user_navigated: bool,
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
    /// List with size and relative-modified columns for the files; folders
    /// carry no size (recursive measurement is not triggered here). The
    /// default: a fresh session opens with useful columns, not a bare list.
    #[default]
    Details,
    /// Side column rendering the selected entry.
    Column,
    /// Thumbnail grid replacing the file list.
    Grid,
    /// No preview. Last in the cycle.
    Off,
}

impl PreviewMode {
    /// In persisted representation (state file `preview<N>=`): 0=off,
    /// 1=column, 2=grid, 3=details. Cycle order is details -> column -> grid
    /// -> off (see [`App::cycle_preview`]).
    pub fn as_u8(self) -> u8 {
        match self {
            PreviewMode::Off => 0,
            PreviewMode::Column => 1,
            PreviewMode::Grid => 2,
            PreviewMode::Details => 3,
        }
    }

    /// Inverse of [`PreviewMode::as_u8`]; unknown values read as off.
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => PreviewMode::Column,
            2 => PreviewMode::Grid,
            3 => PreviewMode::Details,
            _ => PreviewMode::Off,
        }
    }
}

/// An in-place rename being edited in a modal text box.
#[derive(Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub content: String,
    pub document_id: u64,
    pub edit_revision: u64,
    pub last_saved_revision: Option<u64>,
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
#[derive(Clone, PartialEq, Eq)]
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

/// Stable entry identity for mouse/menu actions; stale generations are rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryTarget {
    pub pane: usize,
    pub path: PathBuf,
    pub listing_generation: u64,
}
#[derive(Debug, Clone)]
pub struct OpenEditorRequest {
    pub window_generation: u64,
    pub document_id: u64,
    pub target: EntryTarget,
    pub document_generation: u64,
    pub focus_generation: u64,
}
pub use crate::editor::SaveSnapshot;
/// Host-only interactions, drained without executing native work in a view.
#[derive(Debug, Clone)]
pub enum HostRequest {
    CopyText(String),
    Reveal {
        target: PathBuf,
        is_dir: bool,
        cwd: PathBuf,
    },
    Terminal(PathBuf),
    OpenFile(PathBuf),
    RefreshDrives,
    InvalidatePreview(PathBuf),
    EditorKey {
        document_id: u64,
        key: crate::input::KeyEvent,
    },
    EditorPaste {
        document_id: u64,
        text: String,
    },
    SaveEditor(crate::editor::SaveSnapshot),
    OpenEditor(OpenEditorRequest),
}
