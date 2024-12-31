use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub label: String,
    pub path: String,
    pub shortcut: char,
}

impl Folder {
    pub fn new(label: String, path: String, shortcut: char) -> Self {
        Self {
            label,
            path,
            shortcut,
        }
    }
}
