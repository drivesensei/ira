#[derive(Debug, Clone)]
pub struct FEntry {
    pub path: String,
    pub label: String,
    /// Whether this entry is a directory (drives the folder/file icon).
    pub is_dir: bool,
}

pub fn list_files(path: &str) -> Result<Vec<FEntry>, std::io::Error> {
    let mut drives = Vec::new();
    let entries = std::fs::read_dir(path)?;

    for entry in entries {
        match entry {
            Ok(entry) => {
                if let Some(label) = entry.file_name().to_str() {
                    let path = entry.path();
                    // DirEntry::file_type() on Linux uses the readdir d_type,
                    // so this is cheap (no extra stat syscall for most entries).
                    let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    drives.push(FEntry {
                        path: path.to_string_lossy().into_owned(),
                        label: label.to_string(),
                        is_dir,
                    });
                }
            }
            Err(e) => println!("Error reading entry: {}", e),
        }
    }

    Ok(drives)
}
