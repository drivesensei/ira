use super::*;
#[test]
fn all_four_retained_comparators_cover_sizes_modified_ties_and_directory_order() {
    let make = |label: &str, dir, size, modified| FEntry {
        label: label.into(),
        path: format!("/fixture/{label}"),
        is_dir: dir,
        size,
        modified,
    };
    let records = vec![
        make("z-large", false, 9, Some(2)),
        make("a-small", false, 1, Some(1)),
        make("b-tie", false, 9, Some(2)),
        make("directory", true, 0, None),
        make("文 space", false, 1, None),
    ];
    let orders = [
        vec!["a-small", "b-tie", "directory", "z-large", "文 space"],
        vec!["b-tie", "z-large", "a-small", "文 space", "directory"],
        vec!["b-tie", "z-large", "a-small", "directory", "文 space"],
        vec!["directory", "a-small", "b-tie", "z-large", "文 space"],
    ];
    let (mut app, _) = settlement_tests::app_fixture();
    for (mode, expected) in orders.into_iter().enumerate() {
        let mut entries = records.clone();
        reader::sort_entries(&mut entries, mode);
        app.panes[0].files = records.clone();
        app.panes[0].selected = vec![false; records.len()];
        app.panes[0].sort_mode = (mode + 3) % 4;
        app.cycle_sort();
        assert!(same_entries(&app.panes[0].files, &entries));
        assert_eq!(
            entries.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(),
            expected,
            "mode {mode}"
        );
    }
}
#[test]
fn actual_folder_and_file_receipts_preserve_each_mode_and_exact_target_legacy_listing_stays_alpha()
{
    let _lane = TEST_LANE.lock().unwrap();
    let (mut app, root) = settlement_tests::app_fixture();
    let chosen = root.join("z-large");
    std::fs::write(&chosen, b"large file").unwrap();
    std::fs::write(root.join("a-small"), b"a").unwrap();
    std::fs::write(root.join(".hidden"), b"hidden").unwrap();
    std::fs::create_dir(root.join("directory")).unwrap();
    for mode in 0..4 {
        for (path, kind) in [
            (root.clone(), ExistingPathKind::Folder),
            (chosen.clone(), ExistingPathKind::File),
        ] {
            app.panes[0].sort_mode = mode;
            app.panes[0].filter_query = Some("old filter".into());
            let expected = load(path.clone(), kind, false, mode).unwrap().files;
            settlement_tests::ready(&mut app, path, kind);
            app.drain_existing_paths();
            assert!(app
                .take_existing_path_receipts()
                .pop()
                .unwrap()
                .result
                .is_ok());
            assert!(same_entries(&app.panes[0].files, &expected));
            assert_eq!(app.panes[0].sort_mode, mode);
            assert!(!app.show_hidden);
            assert!(app.panes[0].filter_query.is_none());
            if kind == ExistingPathKind::File {
                assert_eq!(
                    app.selected_visible_entry().unwrap().path,
                    chosen.to_str().unwrap()
                );
                app.cycle_sort();
                assert_eq!(
                    app.selected_visible_entry().unwrap().path,
                    chosen.to_str().unwrap()
                );
                app.panes[0].sort_mode = mode;
            }
        }
        // The unchanged legacy complete listing contract always sorts Alpha.
        app.list_files_for_pane(0);
        let reply = app.bounded_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        app.bounded_tx.send(reply).unwrap();
        app.pick_up_bounded_listings();
        assert_eq!(
            app.panes[0]
                .files
                .iter()
                .map(|e| e.label.as_str())
                .collect::<Vec<_>>(),
            vec!["a-small", "directory", "z-large"]
        );
    }
}
