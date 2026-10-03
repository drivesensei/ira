#[test]
fn transfer_reveals_folder_live_in_destination_pane() {
    let base = std::env::temp_dir().join(format!("ira_live_dest_{}", std::process::id()));
    let src = base.join("src");
    let dst = base.join("dst");
    std::fs::create_dir_all(src.join("photos")).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    // Make the copy take long enough to observe the live reveal.
    for i in 0..2000 {
        std::fs::write(
            src.join("photos").join(format!("p{i:04}.jpg")),
            vec![0u8; 100 * 1024],
        )
        .unwrap();
    }

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("src".into(), src.to_str().unwrap().into(), '#'));
    app.panes[1].folder = Some(Folder::new("dst".into(), dst.to_str().unwrap().into(), '#'));
    app.panes[0].files = vec![FEntry {
        path: src.join("photos").to_str().unwrap().to_string(),
        label: "photos".to_string(),
        is_dir: true,
        size: 0,
        modified: None,
    }];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));

    app.request_copy();
    app.confirm_pending();
    assert!(app.transfer_dest.is_some(), "live sync armed");

    // Deterministic stall: let the folder be created, then pause the worker
    // mid-copy (it parks at a 256KB chunk gate with partial content).
    std::thread::sleep(Duration::from_millis(80));
    app.jobs[0].control.set_paused(true);

    // The reveal must happen while the transfer is stalled: the destination
    // listing refresh (1s cadence) shows the folder and the cursor lands on
    // it via the reveal target.
    let mut revealed_while_stalled = false;
    for _ in 0..300 {
        app.tick();
        let idx = app.panes[1].state.selected();
        if idx
            .and_then(|i| app.panes[1].files.get(i))
            .is_some_and(|f| f.label == "photos")
        {
            revealed_while_stalled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        revealed_while_stalled,
        "folder must be revealed live while the transfer is paused"
    );

    // Resume: the copy completes, live sync clears, folder present.
    app.jobs[0].control.set_paused(false);
    for _ in 0..4000 {
        app.tick();
        if app.transfer_dest.is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        app.transfer_dest.is_none(),
        "live sync cleared on completion"
    );
    assert!(app.panes[1].files.iter().any(|f| f.label == "photos"));

    let _ = std::fs::remove_dir_all(&base);
}


#[test]
fn navigating_the_destination_pane_during_a_transfer_is_not_yanked_back() {
    let base = std::env::temp_dir().join(format!("ira_no_yank_{}", std::process::id()));
    let src = base.join("src");
    let dst = base.join("dst");
    std::fs::create_dir_all(src.join("photos")).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    // A source big enough that "photos" streams in while we browse.
    for i in 0..2000 {
        std::fs::write(
            src.join("photos").join(format!("p{i:04}.jpg")),
            vec![0u8; 100 * 1024],
        )
        .unwrap();
    }
    // Pre-existing destination entries to scroll onto.
    std::fs::write(dst.join("aaa.txt"), "A").unwrap();
    std::fs::write(dst.join("bbb.txt"), "B").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("src".into(), src.to_str().unwrap().into(), '#'));
    app.panes[1].folder = Some(Folder::new("dst".into(), dst.to_str().unwrap().into(), '#'));
    app.panes[0].files = vec![FEntry {
        path: src.join("photos").to_str().unwrap().to_string(),
        label: "photos".to_string(),
        is_dir: true,
        size: 0,
        modified: None,
    }];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));

    app.request_copy();
    app.confirm_pending();

    // Stall the worker so the transfer stays live while we navigate.
    std::thread::sleep(Duration::from_millis(80));
    app.jobs[0].control.set_paused(true);
    assert!(
        app.transfer_dest.is_some(),
        "the transfer must still be live: a finished copy would not re-list"
    );

    // Wait for the live reveal to land the cursor on the incoming folder.
    let mut revealed = false;
    for _ in 0..300 {
        app.tick();
        if app.panes[1]
            .state
            .selected()
            .and_then(|i| app.panes[1].files.get(i))
            .is_some_and(|f| f.label == "photos")
        {
            revealed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(revealed, "incoming folder is revealed live");

    // The user takes the cursor over: focus the destination pane and jump to
    // the top of the list.
    app.active_pane = 1;
    app.goto_top();
    let label_at = |app: &App| {
        app.panes[1]
            .state
            .selected()
            .and_then(|i| app.panes[1].files.get(i))
            .map(|f| f.label.clone())
    };
    assert_eq!(label_at(&app).as_deref(), Some("aaa.txt"));
    let generation = app.panes[1].listing_generation;

    // Once the user drives the pane, the live refresh must leave it alone:
    // no re-list (which would reset the scroll window and re-apply a stale
    // cursor target) and no cursor move.
    for _ in 0..700 {
        app.tick();
        assert_eq!(
            app.panes[1].listing_generation, generation,
            "the browsed pane must not be re-listed under the user"
        );
        assert_eq!(
            label_at(&app).as_deref(),
            Some("aaa.txt"),
            "the cursor must not be yanked back to the incoming item"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    app.jobs[0].control.set_paused(false);
    wait_for_jobs(&mut app);
    assert_eq!(
        label_at(&app).as_deref(),
        Some("aaa.txt"),
        "completing the transfer must not move the user's cursor either"
    );
    let _ = std::fs::remove_dir_all(&base);
}

