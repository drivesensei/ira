/// Capacity of a drive. `available` is `None` when it cannot be known without
/// mounting (e.g. an unmounted Linux partition).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskSpace {
    pub total: u64,
    pub available: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub label: String,
    pub path: String,
    pub shortcut: char,
    /// Block device path (e.g. "/dev/sdb1") for drives; `None` for ordinary folders/bookmarks.
    pub device: Option<String>,
    /// Total/available space for drives; `None` for folders/bookmarks or when unknown.
    pub space: Option<DiskSpace>,
}

impl Folder {
    pub fn new(label: String, path: String, shortcut: char) -> Self {
        Self {
            label,
            path,
            shortcut,
            device: None,
            space: None,
        }
    }
}
