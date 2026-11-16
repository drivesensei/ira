use super::*;
pub(super) fn fixture() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ira-existing-path-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&path).unwrap();
    path // Retained; no cleanup while workers/receipts may exist.
}
#[test]
fn folder_and_file_keep_spelling_sort_and_exact_target() {
    let root = fixture();
    let chosen = root.join("Unicode 文😀 file.txt");
    std::fs::write(&chosen, b"chosen").unwrap();
    std::fs::write(root.join("a.txt"), b"a").unwrap();
    let file = load(chosen.clone(), ExistingPathKind::File, false, 1).unwrap();
    assert_eq!(file.folder, root);
    assert_eq!(file.target, Some(chosen.clone()));
    assert_eq!(file.files[0].path, chosen.to_str().unwrap());
    let folder = load(root.clone(), ExistingPathKind::Folder, false, 0).unwrap();
    assert_eq!(folder.folder, root);
    assert!(folder.target.is_none());
}
#[test]
fn hidden_removed_and_wrong_kind_never_fall_back_or_create() {
    let root = fixture();
    let hidden = root.join(".hidden");
    std::fs::write(&hidden, b"x").unwrap();
    std::fs::write(root.join("visible"), b"v").unwrap();
    assert!(load(hidden.clone(), ExistingPathKind::File, false, 0).is_err());
    assert_eq!(
        load(hidden.clone(), ExistingPathKind::File, true, 0)
            .unwrap()
            .target,
        Some(hidden)
    );
    assert!(load(root.clone(), ExistingPathKind::File, true, 0).is_err());
    let absent = root.join("not-created");
    assert!(load(absent.clone(), ExistingPathKind::File, true, 0).is_err());
    assert!(!absent.exists());
    assert!(load(root.join("visible"), ExistingPathKind::Folder, true, 0).is_err());
}
#[test]
fn partial_directory_error_is_a_failed_result_not_a_successful_prefix() {
    let root = fixture();
    std::fs::write(root.join("visible"), b"v").unwrap();
    let real = std::fs::read_dir(&root).unwrap().next().unwrap();
    let result = read_entries(
        vec![
            real,
            Err(std::io::Error::other("injected readdir error after prefix")),
        ],
        true,
    );
    assert!(result
        .unwrap_err()
        .contains("Partial directory listing failed"));
}
#[cfg(unix)]
#[test]
fn directory_symlink_is_folder_and_non_utf8_is_rejected() {
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    let root = fixture();
    let directory = root.join("directory");
    std::fs::create_dir(&directory).unwrap();
    let link = root.join("link");
    symlink(&directory, &link).unwrap();
    assert_eq!(
        load(link.clone(), ExistingPathKind::Folder, true, 0)
            .unwrap()
            .folder,
        link
    );
    let dangling = root.join("dangling");
    symlink(root.join("absent"), &dangling).unwrap();
    assert!(load(dangling, ExistingPathKind::Folder, true, 0).is_err());
    let raw = root.join(std::ffi::OsString::from_vec(vec![255]));
    assert!(load(raw, ExistingPathKind::File, true, 0)
        .unwrap_err()
        .contains("non-UTF8"));
}
#[test]
fn scoped_receipt_selects_exact_file_and_stale_scope_cannot_mutate() {
    let _lane = TEST_LANE.lock().unwrap();
    let root = fixture();
    let chosen = root.join("chosen");
    std::fs::write(&chosen, b"c").unwrap();
    std::fs::write(root.join("other"), b"o").unwrap();
    let mut app = App::default();
    app.state_path = Some(root.join("state"));
    app.bookmarks_path = Some(root.join("bookmarks"));
    app.panes[0].listing_settled = true;
    app.window_generation = 7;
    let scope = app.existing_path_scope(0).unwrap();
    app.request_existing_path(0, chosen.clone(), ExistingPathKind::File, scope.clone(), 1)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.drain_existing_paths();
        if app.existing_paths.pending.is_none() {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    let receipt = app.take_existing_path_receipts().pop().unwrap();
    assert!(receipt.result.is_ok());
    assert_eq!(receipt.exact_target, Some(chosen.clone()));
    assert_eq!(
        app.selected_visible_entry().unwrap().path,
        chosen.to_str().unwrap()
    );
    assert!(app.host_requests.is_empty());
    assert!(app.edit.is_none());
    let hidden = root.join(".hidden-selected");
    std::fs::write(&hidden, b"hidden").unwrap();
    let original_cursor = app.panes[0].state.selected();
    let original_rows = app.panes[0].files.len();
    let error_scope = app.existing_path_scope(0).unwrap();
    app.request_existing_path(0, hidden.clone(), ExistingPathKind::File, error_scope, 11)
        .unwrap();
    loop {
        app.drain_existing_paths();
        if app.existing_paths.pending.is_none() {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    let unavailable = app.take_existing_path_receipts().pop().unwrap();
    assert!(unavailable.result.is_err());
    assert_eq!(unavailable.exact_target, Some(hidden));
    assert_eq!(app.panes[0].state.selected(), original_cursor);
    assert_eq!(app.panes[0].files.len(), original_rows);
    assert!(app.status.as_ref().unwrap().is_error);
    app.clear_status();
    app.focus_generation = app.focus_generation.wrapping_add(1);
    let before = app.panes[0].folder.clone();
    let current = app.existing_path_scope(0).unwrap();
    app.request_existing_path(0, root.clone(), ExistingPathKind::Folder, current, 2)
        .unwrap();
    app.invalidate_existing_path_requests();
    loop {
        app.drain_existing_paths();
        if app.existing_paths.pending.is_none() {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert_eq!(app.panes[0].folder, before);
    assert!(app.take_existing_path_receipts().is_empty());
}

#[test]
fn physical_worker_admission_survives_invalidated_and_dropped_app_until_real_reply_drop() {
    let _lane = TEST_LANE.lock().unwrap();
    let root = fixture();
    let release = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let entered = Arc::new(AtomicBool::new(false));
    let mut old = App::default();
    old.state_path = Some(root.join("old-state"));
    old.bookmarks_path = Some(root.join("old-bookmarks"));
    old.window_generation = 7;
    old.panes[0].listing_settled = true;
    let held = release.clone();
    let started = entered.clone();
    old.existing_paths.before_load = Some(Arc::new(move || {
        started.store(true, Ordering::Release);
        let (lock, ready) = &*held;
        let mut released = lock.lock().unwrap();
        while !*released {
            released = ready.wait(released).unwrap();
        }
    }));
    let scope = old.existing_path_scope(0).unwrap();
    old.request_existing_path(0, root.clone(), ExistingPathKind::Folder, scope, 1)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !entered.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    old.cancel_pending_work();
    drop(old);
    let mut next = App::default();
    next.state_path = Some(root.join("new-state"));
    next.bookmarks_path = Some(root.join("new-bookmarks"));
    next.window_generation = 8;
    next.panes[0].listing_settled = true;
    let scope = next.existing_path_scope(0).unwrap();
    assert!(next
        .request_existing_path(0, root.clone(), ExistingPathKind::Folder, scope.clone(), 2)
        .unwrap_err()
        .contains("physical worker"));
    {
        let (lock, ready) = &*release;
        *lock.lock().unwrap() = true;
        ready.notify_one();
    }
    while EXISTING_PATH_BUSY.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    next.request_existing_path(0, root, ExistingPathKind::Folder, scope, 3)
        .unwrap();
    loop {
        next.drain_existing_paths();
        if next.existing_paths.pending.is_none() {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(next
        .take_existing_path_receipts()
        .pop()
        .unwrap()
        .result
        .is_ok());
}
