use std::collections::HashMap;
use std::error;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use open::that_detached;
use ratatui::widgets::ListState;

use crate::{
    domain::data::Folder,
    services::{
        bookmarks::{next_free_shortcut, read_bookmarks, write_bookmarks},
        drives::{eject_drive, list_drives, mount_drive},
        file_info::{
            build_info_fast, build_info_full, dir_size, size_line_final, InfoEvent, SizeInfo,
            WalkHandle,
        },
        folders::list_common_folders,
        list_files::{list_files_bounded, list_files_chunked, FEntry, LISTING_CHUNK},
        state::{load_state, save_state, save_state_to, SessionState, SizeEntry},
        transfer::{spawn_job, Job, JobControl, JobEvent, JobKind, JobStatus},
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
    /// `false` while a chunked listing is still streaming; `true` once the
    /// final sorted pass replaced the streamed prefix (or the listing
    /// errored). Test/UI hook for "listing is complete".
    pub listing_settled: bool,
    /// Bumped on every listing request; results carrying an older
    /// generation are dropped, so overlapping listings never interleave
    /// (the "folder lists itself" bug).
    pub listing_generation: u64,
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

/// A pending destructive action awaiting confirmation.
pub struct Confirm {
    /// Prompt label, e.g. `'report.pdf'` or `3 items`.
    pub label: String,
    /// Full paths of the items the action would affect.
    pub paths: Vec<String>,
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

    /// Transient status/error message shown in the bottom bar until it
    /// expires (or the next action replaces it).
    pub status: Option<Status>,

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
    /// State file location override; `None` = the real `~/.config/ira/state`.
    /// Integration tests set this so their walks never touch user config.
    pub state_path: Option<PathBuf>,
}

impl Default for App {
    fn default() -> Self {
        let (job_tx, job_rx) = mpsc::channel();
        let (file_list_tx, file_list_rx) = mpsc::channel();
        let (info_tx, info_rx) = mpsc::channel();
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
            info: None,
            status: None,
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
            info_tx,
            info_rx,
        }
    }
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

    /// Lists `pane_index`'s folder asynchronously (chunked streaming +
    /// sorted final pass) so huge/slow folders never block the UI.
    fn list_files_for_pane(&mut self, pane_index: usize) {
        if pane_index >= self.panes.len() {
            return;
        }
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
                pane.state.select(None);
                pane.listing_settled = true;
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
                pane.listing_settled = true;
            } else {
                pane.listing_settled = false;
                pane.selected
                    .extend(std::iter::repeat_n(false, files.len()));
                pane.files.extend(files);
            }
        }
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
        self.pane_mut().folder = Some(Folder {
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
        self.pane_mut().folder = Some(selected);
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

        match get_directory(&path) {
            Ok(some_folder) => {
                if let Some(actual_folder) = some_folder {
                    self.pane_mut().folder = Some(actual_folder);
                    self.search_query = None;
                    self.list_files_from_selected_folder();
                    self.pane_mut().state.select(None);
                } else {
                    // it's a file, just open it
                    if let Err(msg) = open_file(&path) {
                        self.set_status(msg, true);
                    }
                }
            }
            Err(_) => {}
        }
    }

    pub fn out_of_folder(&mut self) {
        let Some(current_path) = self.pane().folder.as_ref().map(|f| f.path.clone()) else {
            return;
        };

        match get_parent_directory(&current_path) {
            Ok(Some(folder)) => {
                self.pane_mut().folder = Some(folder);
                self.search_query = None;
                self.list_files_from_selected_folder();
                self.pane_mut().state.select(None);
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
        let next = match self.pane().state.selected() {
            Some(i) => Some((i + 1).min(count - 1)),
            None => Some(0),
        };
        self.pane_mut().state.select(next);
    }

    pub fn prev_item(&mut self) {
        match self.pane().state.selected() {
            Some(i) if i > 0 => self.pane_mut().state.select(Some(i - 1)),
            Some(_) => {}
            None => {
                if self.visible_count() > 0 {
                    self.pane_mut().state.select(Some(0));
                }
            }
        }
    }

    pub fn goto_top(&mut self) {
        if self.visible_count() > 0 {
            self.pane_mut().state.select(Some(0));
        } else {
            self.pane_mut().state.select(None);
        }
    }

    pub fn goto_bottom(&mut self) {
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

    // ---- Copy / move between panes (async, via the Copy Board) ----

    pub fn copy_to_other_pane(&mut self) {
        self.start_transfer(JobKind::Copy);
    }

    pub fn move_to_other_pane(&mut self) {
        self.start_transfer(JobKind::Move);
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
    /// Bound to Super+A.
    pub fn toggle_select_all(&mut self) {
        let pane = self.pane_mut();
        let all_selected = !pane.selected.is_empty() && pane.selected.iter().all(|&s| s);
        pane.selected.fill(!all_selected);
    }

    /// Inverts the multi-selection. Bound to Super+I.
    pub fn invert_selection(&mut self) {
        for s in &mut self.pane_mut().selected {
            *s = !*s;
        }
    }

    /// Toggles whether hidden entries (dotfiles) are listed. Bound to `.`.
    pub fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.list_files_for_pane(0);
        self.list_files_for_pane(1);
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

    fn start_transfer(&mut self, kind: JobKind) {
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
        let mut spawned = false;
        for src in sources {
            let src_path = std::path::Path::new(&src);
            if dest_path.starts_with(src_path) {
                self.set_status("Cannot copy/move a folder into itself.", true);
                continue;
            }
            let label = src_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "item".to_string());

            let id = self.next_job_id;
            self.next_job_id += 1;
            let control = JobControl::new();
            self.jobs.push(Job {
                id,
                kind,
                source: src,
                dest_dir: dest.clone(),
                label,
                total_bytes: None,
                copied_bytes: 0,
                current: String::new(),
                status: JobStatus::Running,
                started_at: Instant::now(),
                control: control.clone(),
            });
            let job = self.jobs.last().unwrap();
            spawn_job(job, self.job_tx.clone());
            spawned = true;
        }
        if spawned {
            // The selected entries are now handled; drop the selection and
            // focus the Copy Board on the newest job.
            self.pane_mut().selected.fill(false);
            self.copy_board = true;
            self.board_focused = true;
            self.copy_board_state.select(Some(self.jobs.len() - 1));
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
        self.confirming = Some(Confirm { label, paths });
    }

    /// Confirms the pending deletion and removes the files/directories.
    pub fn confirm_delete(&mut self) {
        let Some(confirm) = self.confirming.take() else {
            return;
        };
        for path in &confirm.paths {
            let result = std::fs::symlink_metadata(path).ok().map(|meta| {
                if meta.is_dir() {
                    std::fs::remove_dir_all(path)
                } else {
                    std::fs::remove_file(path)
                }
            });
            if let Some(Err(err)) = result {
                self.set_status(format!("Failed to delete '{path}': {err}"), true);
            }
        }
        self.pane_mut().selected.fill(false);
        self.refresh_after_job();
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
                    self.refresh_after_job();
                }
            }
        }
    }

    /// Re-lists both panes so completed/cancelled transfers are reflected.
    fn refresh_after_job(&mut self) {
        self.list_files_for_pane(0);
        self.list_files_for_pane(1);
    }

    /// Whether fuzzy search within the current folder is active.
    pub fn is_searching(&self) -> bool {
        self.search_query.is_some()
    }

    /// Indices into the active pane's files matching the current query, best match first.
    fn search_matches(&self) -> Vec<usize> {
        let query = self.search_query.as_deref().unwrap_or("");
        let files = &self.pane().files;
        if query.is_empty() {
            return (0..files.len()).collect();
        }
        let mut scored: Vec<(u32, usize)> = files
            .iter()
            .enumerate()
            .filter_map(|(i, f)| fuzzy_score(query, &f.label).map(|s| (s, i)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, i)| i).collect()
    }

    /// The currently visible entries of the active pane (filtered by search),
    /// as `(file_index, entry)` pairs so callers can map rows back to the
    /// source file list (e.g. for multi-select).
    pub fn visible_rows(&self) -> Vec<(usize, &FEntry)> {
        let files = &self.pane().files;
        if self.is_searching() {
            self.search_matches()
                .into_iter()
                .filter_map(|i| files.get(i).map(|f| (i, f)))
                .collect()
        } else {
            files.iter().enumerate().collect()
        }
    }

    /// Maps a visible-list index to the underlying `files` index.
    fn visible_file_index(&self, visible_idx: usize) -> Option<usize> {
        if self.is_searching() {
            self.search_matches().get(visible_idx).copied()
        } else {
            (visible_idx < self.pane().files.len()).then_some(visible_idx)
        }
    }

    fn visible_count(&self) -> usize {
        if self.is_searching() {
            self.search_matches().len()
        } else {
            self.pane().files.len()
        }
    }

    /// Maps a visible-list index to the underlying file entry.
    fn visible_entry(&self, visible_idx: usize) -> Option<&FEntry> {
        let files = &self.pane().files;
        if self.is_searching() {
            self.search_matches()
                .get(visible_idx)
                .and_then(|&i| files.get(i))
        } else {
            files.get(visible_idx)
        }
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
        // Keep the selected match highlighted after leaving search mode by
        // mapping the visible index back to the full list.
        let full_index = self
            .pane()
            .state
            .selected()
            .and_then(|visible_idx| self.search_matches().get(visible_idx).copied());
        self.search_query = None;
        if let Some(i) = full_index {
            self.pane_mut().state.select(Some(i));
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
    fn restore_state(&mut self) {
        let state = load_state();
        self.split = state.split;
        self.active_pane = state.active_pane.min(1);
        if let Some(left) = state.left {
            self.panes[0].folder = Some(left);
        }
        if let Some(right) = state.right {
            self.panes[1].folder = Some(right);
        }
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
            left: self.panes[0].folder.clone(),
            right: self.panes[1].folder.clone(),
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
                left: app.panes[0].folder.clone(),
                right: app.panes[1].folder.clone(),
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
