use std::{
    error,
    fs::File,
    io::{BufReader, Write},
};

use cli_log::error;
use open::that;
use ratatui::widgets::ListState;

use crate::{
    domain::data::Folder,
    services::{
        config_dir::get_config_file_path,
        drives::list_drives,
        folders::list_common_folders,
        list_files::{list_files, FEntry},
    },
    utils::is_dir::{get_directory, get_parent_directory},
};

/// Application result type.
pub type AppResult<T> = std::result::Result<T, Box<dyn error::Error>>;

/// Application.
#[derive(Debug)]
pub struct App {
    /// Is the application running?
    pub running: bool,

    /// size checks
    pub size: (u16, u16),

    pub drives: Option<Vec<Folder>>,

    pub folders: Option<Vec<Folder>>,
    pub bookmarks: Option<Vec<Folder>>,

    pub tab1_folder: Option<Folder>,
    pub tab2_folder: Option<Folder>,

    pub tab1_state: ListState,

    pub tab1_files: Vec<FEntry>,

    pub bookmarks_shortcuts: Vec<char>,

    pub bookmarks_shortcut_idx: u8,
}

impl Default for App {
    fn default() -> Self {
        Self {
            running: true,
            size: (1024, 768),
            tab1_folder: None,
            tab2_folder: None,
            drives: None,
            folders: Some(list_common_folders()),
            bookmarks: Some(Vec::new()),
            tab1_state: ListState::default(),
            tab1_files: Vec::new(),
            bookmarks_shortcuts: vec!['o', 'p', 'h', 'j', 'k', 'l'],
            bookmarks_shortcut_idx: 0,
        }
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        let mut default = Self::default();

        // Load bookmarks
        if let Err(e) = default.load_bookmarks() {
            error!("Failed to load bookmarks: {}", e);
        }

        // Load drives
        if let Ok(app_drives) = list_drives() {
            let count = app_drives.len();

            if count > 0 {
                if let Some(first_drive) = app_drives.get(0) {
                    default.tab1_folder = Some(first_drive.clone());
                    default.list_files_from_selected_folder();
                }
            }

            default.drives = Some(app_drives);
        }

        default
    }

    /// Handles the tick event of the terminal.
    pub fn tick(&mut self) {
        // if self.current_drive.is_none() {
        //     self.set_initial_drive_and_folder(0);
        // }
    }

    pub fn exit_hooks(&mut self) {
        if let Err(e) = self.save_bookmarks() {
            error!("Failed to save bookmarks: {}", e);
        }
    }

    pub fn save_bookmarks(&self) -> AppResult<()> {
        if let Some(bookmarks) = &self.bookmarks {
            if let Some(path) = get_config_file_path() {
                let serialized = serde_json::to_string_pretty(bookmarks)?;
                let mut file = File::create(&path)?;
                file.write_all(serialized.as_bytes())?;
            } else {
                error!("Configuration path not found. Bookmarks not saved.");
            }
        }
        Ok(())
    }

    pub fn load_bookmarks(&mut self) -> AppResult<()> {
        if let Some(path) = get_config_file_path() {
            if path.exists() {
                let file = File::open(&path)?;
                let reader = BufReader::new(file);

                let bookmarks: Vec<Folder> = serde_json::from_reader(reader)?;
                self.bookmarks = Some(bookmarks);
            } else {
                self.bookmarks = Some(Vec::new());
            }
        } else {
            error!("Configuration path not found. Bookmarks not loaded.");
        }
        Ok(())
    }

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.exit_hooks();
        self.running = false;
    }

    pub fn set_terminal_size(&mut self, width: u16, height: u16) {
        self.size = (width, height);
    }

    pub fn should_increase_size(&mut self, width: u16, height: u16) -> bool {
        // width < 107 || height < 27 // these are the approximate to 1024x700 pixels
        width < 90 || height < 15 // TODO: use above line for prod
    }

    pub fn list_drives(&mut self) -> &Option<Vec<Folder>> {
        if let Ok(drives) = list_drives() {
            self.drives = Some(drives);
        }
        &self.drives
    }

    pub fn list_files_from_selected_folder(&mut self) {
        if let Some(current) = &self.tab1_folder {
            if let Ok(mut files) = list_files(&current.path) {
                files.sort_by(|a, b| a.label.cmp(&b.label));
                self.tab1_files = files;
            }
        }
    }

    pub fn get_drive_shortcuts(&self) -> Vec<char> {
        let mut result: Vec<char> = Vec::new();
        if let Some(ref drives) = self.drives {
            for d in drives {
                result.push(d.shortcut.clone());
            }
        }
        result
    }

    pub fn get_common_folders_shortcuts(&self) -> Vec<char> {
        let mut result: Vec<char> = Vec::new();
        if let Some(folders) = &self.folders {
            for f in folders {
                result.push(f.shortcut);
            }
        }
        result
    }

    pub fn set_tab1_folder_from_drives(&mut self, initial_shortcut: usize) {
        if let Ok(app_drives) = list_drives() {
            let count = app_drives.len();

            if count > 0 {
                if let Some(selected_drive) = app_drives.get(initial_shortcut) {
                    self.tab1_folder = Some(selected_drive.clone());
                    self.list_files_from_selected_folder();
                }
            }

            self.drives = Some(app_drives);
        }
    }

    pub fn set_tab1_folder_from_common_folders(&mut self, initial_shortcut: usize) {
        if let Some(app_folders) = &self.folders {
            let count = app_folders.len();

            if count > 0 {
                if let Some(selected_common_folder) = app_folders.get(initial_shortcut) {
                    self.tab1_folder = Some(selected_common_folder.clone());
                    self.list_files_from_selected_folder();
                }
            }
        }
    }

    pub fn set_tab1_folder_from_bookmarks(&mut self, initial_shortcut: usize) {
        if let Some(app_bookmarks) = &self.bookmarks {
            let count = app_bookmarks.len();

            if count > 0 {
                if let Some(selected_bookmark) = app_bookmarks.get(initial_shortcut) {
                    self.tab1_folder = Some(selected_bookmark.clone());
                    self.list_files_from_selected_folder();
                }
            }
        }
    }

    pub fn enter_folder(&mut self) {
        if let Some(idx) = self.tab1_state.selected() {
            if let Some(selected_folder) = self.tab1_files.get(idx) {
                match get_directory(&selected_folder.path) {
                    Ok(some_folder) => {
                        if let Some(actual_folder) = some_folder {
                            self.tab1_folder = Some(actual_folder);
                            self.list_files_from_selected_folder();
                        } else {
                            // it's a file, just open it
                            let _ = that(&selected_folder.path);
                        }
                    }
                    Err(_) => {}
                }
            }
        }
    }

    pub fn out_of_folder(&mut self) {
        if let Some(idx) = self.tab1_state.selected() {
            if let Some(selected_folder) = self.tab1_files.get(idx) {
                match get_parent_directory(&selected_folder.path) {
                    Ok(some_folder) => {
                        if let Some(actual_folder) = some_folder {
                            self.tab1_folder = Some(actual_folder);
                            self.list_files_from_selected_folder();
                        }
                    }
                    Err(_) => {}
                }
            }
        }
    }

    pub fn tab1_next_item(&mut self) {
        self.tab1_state.select_next();
    }

    pub fn tab1_prev_item(&mut self) {
        self.tab1_state.select_previous();
    }

    pub fn tab1_goto_top(&mut self) {
        self.tab1_state.select_first();
    }

    pub fn tab1_goto_bottom(&mut self) {
        self.tab1_state.select_last();
    }

    pub fn add_bookmark(&mut self) {
        if let Some(idx) = self.tab1_state.selected() {
            if let Some(selected_folder) = self.tab1_files.get(idx) {
                if let Ok(Some(actual_folder)) = get_directory(&selected_folder.path) {
                    if let Some(bs) = self.bookmarks.as_mut() {
                        if self.bookmarks_shortcuts.len() > 0 {
                            let shortcut = *self
                                .bookmarks_shortcuts
                                .get(self.bookmarks_shortcut_idx as usize)
                                .unwrap();
                            self.bookmarks_shortcut_idx += 1;
                            bs.push(Folder {
                                label: actual_folder.label,
                                path: actual_folder.path,
                                shortcut,
                            });
                            self.bookmarks = Some(bs.to_vec());
                        }
                    } else {
                        self.bookmarks = Some(vec![Folder {
                            label: actual_folder.label,
                            path: actual_folder.path,
                            shortcut: self.bookmarks_shortcuts.remove(0),
                        }]);
                    }
                }
            }
        }
    }
}
