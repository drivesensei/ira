use std::error;

use open::that;
use ratatui::widgets::ListState;

use crate::{
    domain::data::Folder,
    services::{
        bookmarks::list_bookmarks,
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
            bookmarks: Some(list_bookmarks()),
            tab1_state: ListState::default(),
            tab1_files: Vec::new(),
        }
    }
}

impl App {
    /// Constructs a new instance of [`App`].
    pub fn new() -> Self {
        let mut default = Self::default();
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

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
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
}
