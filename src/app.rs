use std::error;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use open::that_detached;
use ratatui::widgets::ListState;

use crate::{
    domain::data::Folder,
    services::{
        file_info::build_info,
        bookmarks::{next_free_shortcut, read_bookmarks, write_bookmarks},
        drives::{list_drives, mount_drive},
        folders::list_common_folders,
        list_files::{list_files, FEntry},
        state::{load_state, save_state, SessionState},
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

/// Read-only metadata dialog for a file or folder.
pub struct InfoDialog {
    pub lines: Vec<String>,
}

/// A pending destructive action awaiting confirmation.
pub struct Confirm {
    /// Prompt label, e.g. `'report.pdf'` or `3 items`.
    pub label: String,
    /// Full paths of the items the action would affect.
    pub paths: Vec<String>,
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

    job_tx: mpsc::Sender<JobEvent>,
    job_rx: mpsc::Receiver<JobEvent>,
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
    file_list_tx: mpsc::Sender<(usize, Vec<FEntry>)>,
    file_list_rx: mpsc::Receiver<(usize, Vec<FEntry>)>,
}

impl Default for App {
    fn default() -> Self {
        let (job_tx, job_rx) = mpsc::channel();
        let (file_list_tx, file_list_rx) = mpsc::channel();
        Self {
            running: true,
            size: (1024, 768),
            drives: None,
            folders: Some(list_common_folders()),
            bookmarks: Some(Vec::new()),
            panes: [Pane::default(), Pane::default()],
            active_pane: 0,
            split: false,
            search_query: None,
            show_hidden: false,
            jobs: Vec::new(),
            copy_board: false,
            board_focused: false,
            copy_board_state: ListState::default(),
            confirming: None,
            renaming: None,
            info: None,
            job_tx,
            job_rx,
            next_job_id: 0,
            drive_cache: Arc::new(Mutex::new(Vec::new())),
            drive_generation: Arc::new(Mutex::new(0)),
            seen_drive_generation: 0,
            drives_running: false,
            file_list_tx,
            file_list_rx,
        }
    }
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
        // Load the initial file listings off the render thread so a cold
        // (spin-up) drive can't delay the first frame; results arrive on tick.
        default.request_pane_listing(0);
        default.request_pane_listing(1);

        default
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

    /// Handles the tick event of the terminal.
    pub fn tick(&mut self) {
        self.drain_jobs();
        self.refresh_drives();
        self.pick_up_pane_listings();
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

    fn list_files_for_pane(&mut self, pane_index: usize) {
        let path = self.panes[pane_index].folder.as_ref().map(|f| f.path.clone());
        let Some(path) = path else {
            return;
        };
        if let Ok(mut files) = list_files(&path) {
            files.sort_by(|a, b| a.label.cmp(&b.label));
            if !self.show_hidden {
                files.retain(|f| !f.label.starts_with('.'));
            }
            let len = files.len();
            self.panes[pane_index].files = files;
            self.panes[pane_index].selected = vec![false; len];
        }
    }

    /// Loads a pane's initial file list on a worker thread; the result is
    /// delivered on `tick()` via [`App::pick_up_pane_listings`]. This keeps
    /// slow directories (cold spin-up HDDs, network shares) off the render
    /// path at startup.
    fn request_pane_listing(&self, pane_index: usize) {
        let Some(path) = self.panes[pane_index].folder.as_ref().map(|f| f.path.clone()) else {
            return;
        };
        let show_hidden = self.show_hidden;
        let tx = self.file_list_tx.clone();
        thread::spawn(move || {
            if let Ok(mut files) = list_files(&path) {
                files.sort_by(|a, b| a.label.cmp(&b.label));
                if !show_hidden {
                    files.retain(|f| !f.label.starts_with('.'));
                }
                let _ = tx.send((pane_index, files));
            }
        });
    }

    fn pick_up_pane_listings(&mut self) {
        while let Ok((pane_index, files)) = self.file_list_rx.try_recv() {
            if pane_index < self.panes.len() {
                let len = files.len();
                self.panes[pane_index].files = files;
                self.panes[pane_index].selected = vec![false; len];
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
                    eprintln!("Failed to mount {device}: {err}");
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
                    open_file(&path);
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
            eprintln!("Cannot rename: '{}' already exists.", prompt.original);
            return;
        }
        match std::fs::rename(&src, &dst) {
            Ok(_) => {}
            Err(e) => eprintln!("Failed to rename: {e}"),
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

    pub fn rename_cursor_right(&mut self) {
        if let Some(p) = &mut self.renaming {
            p.cursor = (p.cursor + 1).min(p.text.len());
        }
    }

    // ---- Info dialog ----

    /// Opens the metadata dialog for the entry under the cursor. Bound to `?`.
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
        let lines = build_info(entry);
        self.info = Some(InfoDialog { lines });
    }

    pub fn close_info(&mut self) {
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
            eprintln!("The other pane has no folder to copy into.");
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
                eprintln!("Cannot copy/move a folder into itself.");
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
            self.copy_board_state.select(if self.jobs.is_empty() {
                None
            } else {
                Some(0)
            });
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
                eprintln!("Failed to delete '{path}': {err}");
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
                JobEvent::Progress { id, copied_bytes, current } => {
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
                    eprintln!("No available bookmark shortcut");
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
        // File lists are populated asynchronously from `App::new` via
        // `request_pane_listing`, so a slow drive doesn't block startup.
    }

    /// Persists the current session state (split layout and pane folders).
    pub fn persist_state(&self) {
        save_state(&SessionState {
            split: self.split,
            active_pane: self.active_pane,
            left: self.panes[0].folder.clone(),
            right: self.panes[1].folder.clone(),
        });
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
fn open_file(path: &str) {
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
            return;
        }
    }

    if let Err(err) = that_detached(path) {
        eprintln!("Failed to open '{path}': {err}");
    }
}
