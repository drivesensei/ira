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
    let (_, complete) = list_files_bounded(dir2.to_str().unwrap(), LISTING_CHUNK, false).unwrap();
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

/// A symlink to a directory must be a directory here: `DirEntry::file_type`
/// is the readdir hint and does not follow links, while every `is_dir`
/// guard downstream (icons, preview classification, the preview request)
/// assumes the target's kind. Without this a directory named `*.png`
/// takes a decode job and never resolves.
#[cfg(unix)]
#[test]
fn symlink_to_a_directory_is_listed_as_a_directory() {
    let root = std::env::temp_dir().join(format!("ira-symlink-{}", std::process::id()));
    let real = root.join("real-dir");
    std::fs::create_dir_all(&real).unwrap();
    let file = root.join("real.png");
    std::fs::write(&file, b"not really a png").unwrap();
    std::os::unix::fs::symlink(&real, root.join("album.png")).unwrap();
    std::os::unix::fs::symlink(&file, root.join("link.png")).unwrap();

    let entries = list_files(root.to_str().unwrap()).unwrap();
    let by_label = |label: &str| {
        entries
            .iter()
            .find(|e| e.label == label)
            .cloned()
            .unwrap_or_else(|| panic!("missing {label}"))
    };
    let dir_link = by_label("album.png");
    assert!(dir_link.is_dir, "a symlinked directory is a directory");
    assert_eq!(dir_link.size, 0, "directories carry no size");
    let file_link = by_label("link.png");
    assert!(!file_link.is_dir, "a symlinked file stays a file");
    assert_eq!(file_link.size, 16, "size comes from the target");

    std::fs::remove_dir_all(&root).unwrap();
}
