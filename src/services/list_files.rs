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
                    // file_type() is free (d_type); no stat per entry.
                    let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    files.push(FEntry {
                        path: entry.path().to_string_lossy().into_owned(),
                        label: label.to_string(),
                        is_dir,
                    });
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
                    batch.push(FEntry {
                        path: entry.path().to_string_lossy().into_owned(),
                        label: label.to_string(),
                        is_dir,
                    });
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
mod tests {
    use super::*;

    #[test]
    fn bounded_listing_reports_complete_for_small_folders() {
        let dir = std::env::temp_dir().join(format!("ira_bounded_small_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..10 {
            std::fs::write(dir.join(format!("f{i}.txt")), "x").unwrap();
        }

        let (files, complete) = list_files_bounded(dir.to_str().unwrap(), LISTING_CHUNK, false)
            .expect("read_dir must succeed");
        assert!(complete, "a 10-file folder fits the 512 cap");
        assert_eq!(files.len(), 10);

        // Hidden filtering respects the flag.
        std::fs::write(dir.join(".dot"), "x").unwrap();
        let (files, _) = list_files_bounded(dir.to_str().unwrap(), LISTING_CHUNK, false).unwrap();
        assert!(files.iter().all(|f| !f.label.starts_with('.')));
        let (files, _) = list_files_bounded(dir.to_str().unwrap(), LISTING_CHUNK, true).unwrap();
        assert!(files.iter().any(|f| f.label == ".dot"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bounded_listing_reports_partial_at_the_cap() {
        let dir = std::env::temp_dir().join(format!("ira_bounded_big_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..LISTING_CHUNK + 10 {
            std::fs::write(dir.join(format!("f{i}.txt")), "x").unwrap();
        }

        let (files, complete) = list_files_bounded(dir.to_str().unwrap(), LISTING_CHUNK, false)
            .expect("read_dir must succeed");
        assert!(!complete, "600-entry folder exceeds the cap");
        assert_eq!(files.len(), LISTING_CHUNK);

        // Exactly-at-cap boundary: 512 files = complete.
        let dir2 = std::env::temp_dir().join(format!("ira_bounded_exact_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir2);
        std::fs::create_dir_all(&dir2).unwrap();
        for i in 0..LISTING_CHUNK {
            std::fs::write(dir2.join(format!("g{i}.txt")), "x").unwrap();
        }
        let (_, complete) =
            list_files_bounded(dir2.to_str().unwrap(), LISTING_CHUNK, false).unwrap();
        assert!(complete, "exactly LISTING_CHUNK entries still fit");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn chunked_listing_batches_entries() {
        let dir = std::env::temp_dir().join(format!("ira_chunked_listing_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..1100 {
            std::fs::write(dir.join(format!("f{i:04}.txt")), b"x").unwrap();
        }

        let mut batches: Vec<Vec<FEntry>> = Vec::new();
        list_files_chunked(dir.to_str().unwrap(), LISTING_CHUNK, &mut |b| {
            batches.push(b)
        })
        .unwrap();

        let sizes: Vec<usize> = batches.iter().map(|b| b.len()).collect();
        assert_eq!(sizes, vec![512, 512, 76], "batch sizes: {sizes:?}");
        let total: usize = batches.iter().map(|b| b.len()).sum();
        assert_eq!(total, 1100);
        let labels: std::collections::HashSet<&str> =
            batches.iter().flatten().map(|e| e.label.as_str()).collect();
        assert_eq!(labels.len(), 1100, "every entry present exactly once");
        assert!(labels.contains("f0000.txt") && labels.contains("f1099.txt"));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
