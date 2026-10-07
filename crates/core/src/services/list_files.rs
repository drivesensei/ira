#[derive(Debug, Clone)]
pub struct FEntry {
    pub path: String,
    pub label: String,
    /// Whether this entry is a directory (drives the folder/file icon).
    pub is_dir: bool,
    /// File size in bytes; `0` for directories (they group last in size sort).
    pub size: u64,
    /// Last-modified time as epoch seconds; `None` when unavailable
    /// (stat failed or clock before the Unix epoch).
    pub modified: Option<i64>,
}

/// Stat helper shared by every listing path: sorting by size/modified needs
/// per-entry metadata, so we pay one extra stat per entry here (background
/// and bounded paths only — this runs off the UI thread or on small folders).
///
/// `d_type_is_dir` is the readdir hint (`DirEntry::file_type`), which does
/// *not* follow symlinks. The metadata we take anyway does, and `is_dir`
/// decides the icon, the preview classification and every `preview_kind`
/// guard — so a symlink to a directory must be a directory here, or it is
/// fed to the image pipeline and sticks on a placeholder.
fn entry_with_meta(path: &std::path::Path, label: &str, d_type_is_dir: bool) -> FEntry {
    let (size, modified, is_dir) = match std::fs::metadata(path) {
        Ok(md) => {
            let modified = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
            let is_dir = md.is_dir();
            (if is_dir { 0 } else { md.len() }, modified, is_dir)
        }
        // Stat failed (dangling symlink, permission): keep the readdir hint.
        Err(_) => (0, None, d_type_is_dir),
    };
    FEntry {
        path: path.to_string_lossy().into_owned(),
        label: label.to_string(),
        is_dir,
        size,
        modified,
    }
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
                    drives.push(entry_with_meta(&path, label, is_dir));
                }
            }
            Err(e) => println!("Error reading entry: {}", e),
        }
    }

    Ok(drives)
}

/// Batch size for streaming directory reads: enough rows to fill several
/// screens at once, small enough that the first paint lands within a frame
/// or two even on slow drives.
pub const LISTING_CHUNK: usize = 512;

/// Reads at most `max_entries` entries, telling the caller whether the
/// whole directory fit. Used to pick the sync vs streaming listing path:
/// `complete == true` means the folder is small and was read in full
/// (bounded by a few ms of readdir); `false` means a bigger tree that must
/// go through the background streaming worker.
pub fn list_files_bounded(
    path: &str,
    max_entries: usize,
    show_hidden: bool,
) -> std::io::Result<(Vec<FEntry>, bool)> {
    let mut entries = std::fs::read_dir(path)?;
    let mut files: Vec<FEntry> = Vec::with_capacity(max_entries);
    let mut complete = true;
    while let Some(entry) = entries.next() {
        match entry {
            Ok(entry) => {
                if let Some(label) = entry.file_name().to_str() {
                    if !show_hidden && label.starts_with('.') {
                        continue;
                    }
                    // file_type() is free (d_type); the metadata stat is the
                    // one extra syscall per entry sorting needs.
                    let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    let path = entry.path();
                    files.push(entry_with_meta(&path, label, is_dir));
                    if files.len() >= max_entries {
                        // Peek one more: None = we actually hit EOF, so
                        // the folder fits the cap exactly and is complete.
                        complete = matches!(entries.next(), None | Some(Err(_)));
                        break;
                    }
                }
            }
            Err(e) => println!("Error reading entry: {}", e),
        }
    }
    Ok((files, complete))
}

/// Streams directory entries as they are read, invoking `on_chunk` with each
/// accumulated batch of ~`chunk_size` entries (plus a final short batch).
/// Entries are NOT sorted — the caller decides ordering. The existing
/// [`list_files`] stays for callers that want the whole list at once.
pub fn list_files_chunked(
    path: &str,
    chunk_size: usize,
    on_chunk: &mut dyn FnMut(Vec<FEntry>),
) -> std::io::Result<()> {
    let entries = std::fs::read_dir(path)?;

    let mut batch: Vec<FEntry> = Vec::with_capacity(chunk_size);
    for entry in entries {
        match entry {
            Ok(entry) => {
                if let Some(label) = entry.file_name().to_str() {
                    // DirEntry::file_type() on Linux uses the readdir d_type,
                    // so this is cheap (no extra stat syscall for most entries).
                    let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    let path = entry.path();
                    batch.push(entry_with_meta(&path, label, is_dir));
                    if batch.len() >= chunk_size {
                        on_chunk(std::mem::take(&mut batch));
                    }
                }
            }
            Err(e) => println!("Error reading entry: {}", e),
        }
    }

    if !batch.is_empty() {
        on_chunk(batch);
    }
    Ok(())
}

#[cfg(test)]
#[path = "list_files_tests.rs"]
mod tests;
