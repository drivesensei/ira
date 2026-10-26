use super::*;
#[cfg(unix)]
#[test]
fn included_unsupported_neighbors_fail_folder_and_file_before_any_pane_mutation() {
    use std::os::unix::ffi::OsStringExt;
    let _lane = TEST_LANE.lock().unwrap();
    let (mut app, root) = settlement_tests::app_fixture();
    let chosen = root.join("chosen");
    std::fs::write(&chosen, b"chosen").unwrap();
    std::fs::write(
        root.join(std::ffi::OsString::from_vec(vec![b'z', 255])),
        b"unsupported",
    )
    .unwrap();
    for (path, kind) in [
        (root.clone(), ExistingPathKind::Folder),
        (chosen.clone(), ExistingPathKind::File),
    ] {
        app.clear_status();
        let before = app.operation_state();
        let generation = app.panes[0].listing_generation;
        let navigation = app.navigation_generation[0];
        settlement_tests::ready(&mut app, path, kind);
        app.drain_existing_paths();
        let receipt = app.take_existing_path_receipts().pop().unwrap();
        assert!(receipt.result.unwrap_err().contains("unsupported"));
        assert_eq!(app.panes[0].folder, before.panes[0].folder);
        assert!(same_entries(&app.panes[0].files, &before.panes[0].files));
        assert_eq!(app.panes[0].selected, before.panes[0].selected);
        assert_eq!(app.panes[0].state, before.panes[0].state);
        assert_eq!(app.panes[0].filter_query, before.panes[0].filter_query);
        assert_eq!(app.panes[0].listing_generation, generation);
        assert_eq!(app.navigation_generation[0], navigation);
        assert!(app.host_requests.is_empty());
        assert!(app.edit.is_none());
    }
}
#[cfg(unix)]
#[test]
fn raw_hidden_unsupported_neighbor_is_excluded_only_when_hidden_is_false() {
    use std::os::unix::ffi::OsStringExt;
    let root = tests::fixture();
    let selected = root.join("chosen");
    std::fs::write(&selected, b"chosen").unwrap();
    let raw = root.join(std::ffi::OsString::from_vec(vec![b'.', 255]));
    std::fs::write(&raw, b"hidden").unwrap();
    for (path, kind) in [
        (root.clone(), ExistingPathKind::Folder),
        (selected, ExistingPathKind::File),
    ] {
        let loaded = load(path.clone(), kind, false, 0).unwrap();
        assert_eq!(loaded.files.len(), 1);
        assert!(load(path, kind, true, 0)
            .unwrap_err()
            .contains("unsupported"));
    }
    assert!(load(raw, ExistingPathKind::File, false, 0)
        .unwrap_err()
        .contains("non-UTF8"));
}
