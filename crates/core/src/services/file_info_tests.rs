use super::*;

fn tree(tmp: &Path, name: &str) -> std::path::PathBuf {
    let root = tmp.join(format!("ira_walk_{}_{}", std::process::id(), name));
    std::fs::create_dir_all(root.join("d")).unwrap();
    std::fs::write(root.join("a"), vec![0u8; 100]).unwrap();
    std::fs::write(root.join("d").join("b"), vec![0u8; 50]).unwrap();
    root
}

#[test]
fn walk_sums_files_recursively() {
    let root = tree(std::env::temp_dir().as_path(), "sums");
    let mut progress = |_bytes: u64, _items: u64, _on_disk: u64| {};
    let size = dir_size(&root, &WalkHandle::new(), &mut progress);
    assert_eq!(size.bytes, 150);
    assert_eq!(size.items, 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn walk_reports_no_progress_below_step() {
    let root = tree(std::env::temp_dir().as_path(), "progress");
    let mut seen: Vec<(u64, u64, u64)> = Vec::new();
    let mut progress = |bytes: u64, items: u64, on_disk: u64| seen.push((bytes, items, on_disk));
    dir_size(&root, &WalkHandle::new(), &mut progress);
    assert!(
        seen.is_empty(),
        "2 files never reach PROGRESS_STEP {seen:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cancelled_walk_measures_nothing() {
    let handle = WalkHandle::new();
    handle.cancel();
    let root = tree(std::env::temp_dir().as_path(), "cancel");
    let mut progress = |_bytes: u64, _items: u64, _on_disk: u64| {};
    let size = dir_size(&root, &handle, &mut progress);
    assert!(handle.cancelled());
    // The walk aborts at the first entry; the walk thread in App checks
    // `handle.cancelled()` and never sends the result.
    assert_eq!(size.bytes, 0);
    assert_eq!(size.items, 0);
    let _ = std::fs::remove_dir_all(&root);
}
