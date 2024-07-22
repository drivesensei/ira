use std::error;
use std::io::Result as IOResult;

use crate::services::{
    drives::{list_drives, Drive},
    list_files::{list_files, FEntry},
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

    pub current_drive: Option<Drive>,
    pub drives: Option<Vec<Drive>>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            running: true,
            size: (1024, 768),
            current_drive: None,
            drives: None,
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
                    default.current_drive = Some(first_drive.clone());
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

    pub fn list_drives(&mut self) -> &Option<Vec<Drive>> {
        if let Ok(drives) = list_drives() {
            self.drives = Some(drives);
        }
        &self.drives
    }

    pub fn list_files_from_selected_folder(&mut self) -> IOResult<Vec<FEntry>> {
        if let Some(current_drive) = &self.current_drive {
            let mut files = list_files(&current_drive.path)?;
            files.sort_by(|a, b| a.label.cmp(&b.label));
            Ok(files)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No directory set",
            ))
        }
    }

    pub fn set_initial_drive_and_folder(&mut self, initial_shortcut: usize) {
        if let Ok(app_drives) = list_drives() {
            let count = app_drives.len();

            if count > 0 {
                if let Some(selected_drive) = app_drives.get(initial_shortcut) {
                    self.current_drive = Some(selected_drive.clone());
                }
            }

            self.drives = Some(app_drives);
        }
    }

    pub fn get_drive_shortcuts(&self) -> Vec<usize> {
        let mut result: Vec<usize> = Vec::new();
        if let Some(drives) = &self.drives {
            for d in drives {
                result.push(d.shortcut);
            }
        }
        result
    }
}
