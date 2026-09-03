#[derive(Debug, Clone)]
pub struct Folder {
    pub label: String,
    pub path: String,
    pub shortcut: char,
    /// Block device path (e.g. "/dev/sdb1") for drives; `None` for ordinary folders/bookmarks.
    pub device: Option<String>,
}

impl Folder {
    pub fn new(label: String, path: String, shortcut: char) -> Self {
        Self {
            label,
            path,
            shortcut,
            device: None,
        }
    }
}
