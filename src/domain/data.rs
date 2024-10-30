#[derive(Debug, Clone)]
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
