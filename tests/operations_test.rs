use std::time::{Duration, Instant};

use ira::app::{App, ConfirmAction};
use ira::domain::data::Folder;
use ira::services::list_files::FEntry;
use ira::services::transfer::{JobStatus, OverwritePolicy};

fn entry(path: &str) -> FEntry {
    FEntry {
        path: path.to_string(),
        label: std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string()),
        is_dir: false,
        size: 0,
        modified: None,
    }
}

/// Polls the job channel (as the main loop's tick does) until all jobs settle.
fn wait_for_jobs(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.tick();
        let pending = app.jobs.iter().any(|j| {
            matches!(
                j.status,
                JobStatus::Running | JobStatus::Paused | JobStatus::Queued
            )
        });
        if !pending {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "transfer hung: {:?}",
            app.jobs
                .iter()
                .map(|j| j.status.clone())
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn space_toggles_selection_and_moves_cursor() {
    let mut app = App::default();
    app.panes[0].files = vec![
        entry("/tmp/a.txt"),
        entry("/tmp/b.txt"),
        entry("/tmp/c.txt"),
    ];
    app.panes[0].selected = vec![false, false, false];
    app.panes[0].state.select(Some(0));

    app.toggle_select_current(); // select a.txt, cursor -> 1
    app.toggle_select_current(); // select b.txt, cursor -> 2
    assert_eq!(app.panes[0].selected, vec![true, true, false]);
    assert_eq!(app.panes[0].state.selected(), Some(2));

    // Back to b.txt and toggle it off.
    app.panes[0].state.select(Some(1));
    app.toggle_select_current(); // deselect b.txt, cursor -> 2
    assert_eq!(app.panes[0].selected, vec![true, false, false]);
    assert_eq!(app.panes[0].state.selected(), Some(2));
}

#[test]
fn select_all_and_invert() {
    let mut app = App::default();
    app.panes[0].files = vec![
        entry("/tmp/a.txt"),
        entry("/tmp/b.txt"),
        entry("/tmp/c.txt"),
    ];
    app.panes[0].selected = vec![false, false, false];

    // Super+A: nothing selected -> select all.
    app.toggle_select_all();
    assert_eq!(app.panes[0].selected, vec![true, true, true]);

    // Super+A again: everything selected -> clear all.
    app.toggle_select_all();
    assert_eq!(app.panes[0].selected, vec![false, false, false]);

    // Super+I: invert.
    app.panes[0].selected = vec![true, false, true];
    app.invert_selection();
    assert_eq!(app.panes[0].selected, vec![false, true, false]);
}

#[test]
fn list_files_marks_directories() {
    let base = std::env::temp_dir().join(format!("ira_icons_{}", std::process::id()));
    std::fs::create_dir_all(base.join("sub")).unwrap();
    std::fs::write(base.join("file.txt"), "x").unwrap();

    let entries = ira::services::list_files::list_files(base.to_str().unwrap()).unwrap();
    let sub = entries.iter().find(|e| e.label == "sub").unwrap();
    let file = entries.iter().find(|e| e.label == "file.txt").unwrap();
    assert!(sub.is_dir, "directory entries must be marked is_dir");
    assert!(!file.is_dir, "file entries must not be marked is_dir");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn hidden_files_toggle() {
    let base = std::env::temp_dir().join(format!("ira_hidden_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join(".secret"), "s").unwrap();
    std::fs::write(base.join("visible.txt"), "v").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));

    // Listing is now async (chunked streaming + sorted final pass); wait
    // until the pane's settled (sorted) listing arrives before asserting.
    let mut wait_settled = |app: &mut App| {
        for _ in 0..250 {
            app.tick();
            if app.file_list_settled(0) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    };
    let labels =
        |app: &App| -> Vec<String> { app.panes[0].files.iter().map(|f| f.label.clone()).collect() };

    // Default hides dotfiles.
    app.list_files_from_selected_folder();
    assert!(wait_settled(&mut app), "listing should arrive");
    assert_eq!(labels(&app), vec!["visible.txt"]);

    // `.` toggles them on.
    app.toggle_hidden();
    assert!(wait_settled(&mut app), "re-listing should arrive");
    assert_eq!(labels(&app), vec![".secret", "visible.txt"]);

    // And back off.
    app.toggle_hidden();
    assert!(wait_settled(&mut app));
    assert_eq!(app.panes[0].files.len(), 1);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn copy_uses_all_selected_entries_and_focuses_board() {
    let base = std::env::temp_dir().join(format!("ira_sel_copy_{}", std::process::id()));
    let src = base.join("src");
    let dst = base.join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(src.join("a.txt"), "A").unwrap();
    std::fs::write(src.join("b.txt"), "B").unwrap();
    std::fs::write(src.join("c.txt"), "C").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("src".into(), src.to_str().unwrap().into(), '#'));
    app.panes[1].folder = Some(Folder::new("dst".into(), dst.to_str().unwrap().into(), '#'));
    app.panes[0].files = vec![
        entry(src.join("a.txt").to_str().unwrap()),
        entry(src.join("b.txt").to_str().unwrap()),
        entry(src.join("c.txt").to_str().unwrap()),
    ];
    app.panes[0].selected = vec![true, true, false]; // a.txt + b.txt
    app.panes[0].state.select(Some(0));

    // `c` now asks first: nothing runs until confirmed.
    app.request_copy();
    assert!(app.jobs.is_empty(), "no transfer before confirmation");
    let confirm = app.confirming.as_ref().expect("confirmation dialog");
    assert!(matches!(confirm.action, ConfirmAction::Copy));
    assert_eq!(confirm.paths.len(), 2);
    assert_eq!(
        confirm.dest_dir.as_deref(),
        Some(dst.to_str().unwrap()),
        "re-request must not stack dialogs"
    );
    assert_eq!(confirm.dest_dir.as_deref(), Some(dst.to_str().unwrap()));

    // Confirming spawns ONE batch job for the whole selection and focuses
    // the board.
    app.confirm_pending();
    assert_eq!(app.jobs.len(), 1, "one batch job, not one per file");
    assert_eq!(app.jobs[0].paths.len(), 2);
    assert!(app.copy_board && app.board_has_focus());

    wait_for_jobs(&mut app);

    assert!(dst.join("a.txt").exists());
    assert!(dst.join("b.txt").exists());
    assert!(!dst.join("c.txt").exists());
    assert!(matches!(app.jobs[0].status, JobStatus::Done));

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn move_requires_confirmation_and_cancel_keeps_files() {
    let base = std::env::temp_dir().join(format!("ira_move_cfm_{}", std::process::id()));
    let src = base.join("src");
    let dst = base.join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(src.join("m.txt"), "M").unwrap();

    let mut app = App::default();
    app.split = true;
    app.panes[0].folder = Some(Folder::new("src".into(), src.to_str().unwrap().into(), '#'));
    app.panes[1].folder = Some(Folder::new("dst".into(), dst.to_str().unwrap().into(), '#'));
    app.panes[0].files = vec![entry(src.join("m.txt").to_str().unwrap())];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));

    app.request_move();
    let confirm = app.confirming.as_ref().expect("confirmation dialog");
    assert!(matches!(confirm.action, ConfirmAction::Move));
    assert_eq!(confirm.paths.len(), 1);

    // `n` cancels: file untouched, no jobs.
    app.cancel_confirm();
    assert!(app.confirming.is_none());
    assert!(app.jobs.is_empty());
    assert!(src.join("m.txt").exists());

    // Confirming moves the file.
    app.request_move();
    app.confirm_pending();
    assert_eq!(app.jobs.len(), 1);
    assert!(matches!(app.jobs[0].status, JobStatus::Running));

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn request_copy_blocks_folder_into_itself() {
    let base = std::env::temp_dir().join(format!("ira_self_{}", std::process::id()));
    let src = base.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("a.txt"), "A").unwrap();

    let mut app = App::default();
    app.split = true;
    // Browsing the parent; cursor on the `src` folder. Copying `src` into
    // `src` (the other pane) must be rejected.
    app.panes[0].folder = Some(Folder::new(
        "base".into(),
        base.to_str().unwrap().into(),
        '#',
    ));
    app.panes[1].folder = Some(Folder::new("src".into(), src.to_str().unwrap().into(), '#'));
    let dir_entry = FEntry {
        path: src.to_str().unwrap().to_string(),
        label: "src".to_string(),
        is_dir: true,
        size: 0,
        modified: None,
    };
    app.panes[0].files = vec![dir_entry];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));

    app.request_copy();
    assert!(
        app.confirming.is_none(),
        "self-copy must be rejected upfront"
    );
    assert!(app.status.as_ref().unwrap().is_error);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn scroll_accelerates_while_key_is_held() {
    let mut app = App::default();
    app.panes[0].files = (0..100).map(|i| entry(&format!("/f{i}"))).collect();
    app.panes[0].selected = vec![false; 100];

    // Rapid (within-window) next_item calls ramp the step size.
    // Call 1 only selects row 0; calls 2-6 step 1 (idx 1..5); calls 7-8
    // ramp to step 2 (7, 9).
    for _ in 0..8 {
        app.next_item();
    }
    let sel = app.panes[0].state.selected().unwrap();
    assert_eq!(sel, 9, "8 rapid downs must land at index 9, got {sel}");

    // Direction change resets the ramp: two rapid ups step 1 each.
    app.prev_item();
    app.prev_item();
    let sel = app.panes[0].state.selected().unwrap();
    assert_eq!(sel, 7, "ups after a direction change step 1: got {sel}");

    // Jump navigation resets the ramp.
    app.goto_top();
    let sel = app.panes[0].state.selected().unwrap();
    assert_eq!(sel, 0);
    app.next_item();
    assert_eq!(app.panes[0].state.selected(), Some(1));
}

#[test]
fn scroll_step_is_capped() {
    let mut app = App::default();
    app.panes[0].files = (0..2000).map(|i| entry(&format!("/f{i}"))).collect();
    app.panes[0].selected = vec![false; 2000];
    // Call 1 selects row 0; then steps ramp 1,1,1,1,1,2,2,2,2,2,3,...
    // After 54 rapid calls the cursor sits at 233 (step capped at 6 from
    // repeat 30 onward).
    for _ in 0..54 {
        app.next_item();
    }
    assert_eq!(app.panes[0].state.selected(), Some(233));
}

#[test]
fn delete_progress_dialog_is_dismissable() {
    let base = std::env::temp_dir().join(format!("ira_del_ui_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let victim = base.join("v.txt");
    std::fs::write(&victim, "d").unwrap();

    let mut app = App::default();
    app.panes[0].files = vec![entry(victim.to_str().unwrap())];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));
    app.request_delete();
    app.confirm_delete();
    assert!(
        app.deletion_box_visible(),
        "progress dialog visible initially"
    );

    // Any key hides it (handler sets this): deletion continues regardless.
    app.deletion_box_hidden = true;
    assert!(!app.deletion_box_visible());
    for _ in 0..250 {
        app.tick();
        if app.deletion.is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(app.deletion.is_none());
    assert!(!victim.exists());
    // After completion the box state resets.
    assert!(app.deletion_box_visible() == false);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn delete_requires_confirmation_and_removes_on_confirm() {
    let base = std::env::temp_dir().join(format!("ira_del_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let victim = base.join("victim.txt");
    std::fs::write(&victim, "data").unwrap();

    let mut app = App::default();
    app.panes[0].files = vec![entry(victim.to_str().unwrap())];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));

    // Cancel path: prompt shown, Esc/n cancels, file stays.
    app.request_delete();
    assert!(app.confirming.is_some());
    app.cancel_confirm();
    assert!(app.confirming.is_none());
    assert!(victim.exists());

    // Confirm path: y spawns the background delete worker; the file is
    // removed asynchronously and progress flows through tick().
    app.request_delete();
    assert_eq!(app.confirming.as_ref().unwrap().paths.len(), 1);
    app.confirm_delete();
    assert!(app.confirming.is_none());
    assert!(app.deletion.is_some(), "deletion must run in background");
    let mut done = false;
    for _ in 0..250 {
        app.tick();
        if app.deletion.is_none() {
            done = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(done, "background deletion should finish");
    assert!(!victim.exists());
    assert!(app.deleting_started(victim.to_str().unwrap()).is_none());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn rename_file_in_place() {
    let base = std::env::temp_dir().join(format!("ira_rename_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let old = base.join("doc");
    std::fs::write(&old, "data").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));
    app.panes[0].files = vec![entry(old.to_str().unwrap())];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));

    app.start_rename();
    assert!(app.renaming.is_some());
    app.rename_insert('x'); // "doc" -> "docx"
    app.commit_rename();

    assert!(app.renaming.is_none());
    assert!(!base.join("doc").exists());
    assert!(base.join("docx").exists());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn rename_cancel_keeps_original_name() {
    let base = std::env::temp_dir().join(format!("ira_rename_cancel_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let old = base.join("keep.txt");
    std::fs::write(&old, "x").unwrap();

    let mut app = App::default();
    app.panes[0].files = vec![entry(old.to_str().unwrap())];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));

    app.start_rename();
    app.rename_insert('y');
    app.cancel_rename();
    assert!(app.renaming.is_none());
    assert!(base.join("keep.txt").exists());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn rename_supports_cursor_and_backspace() {
    let mut app = App::default();
    app.panes[0].files = vec![entry("/tmp/abc.txt")];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));

    app.start_rename(); // text "abc.txt" (7 chars), cursor at 7
    app.rename_cursor_left(); // 6
    app.rename_cursor_left(); // 5
    app.rename_backspace(); // remove chars[4], cursor 4
    app.rename_insert('T'); // insert at 4, cursor 5

    let p = app.renaming.as_ref().unwrap();
    assert_eq!(p.text.iter().collect::<String>(), "abc.Txt");
    assert_eq!(p.cursor, 5);
}

#[test]
fn info_dialog_shows_metadata() {
    let base = std::env::temp_dir().join(format!("ira_info_{}", std::process::id()));
    std::fs::create_dir_all(base.join("sub")).unwrap();
    let f = base.join("photo.png");
    std::fs::write(&f, "1234567890123").unwrap(); // 13 bytes

    let mut app = App::default();
    // Never touch the real user config from tests.
    app.state_path =
        Some(std::env::temp_dir().join(format!("ira_state_test_{}", std::process::id())));
    app.panes[0].files = vec![entry(f.to_str().unwrap())];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));

    app.show_info();
    // The dialog opens instantly with the no-filesystem fast lines; the
    // worker's `Ready` event replaces them with the full set.
    assert!(app.info.as_ref().unwrap().pending);
    let fast = app.info.as_ref().unwrap().lines.clone();
    assert!(fast.iter().any(|l| l.starts_with("Name: photo.png")));
    assert!(fast.iter().any(|l| l.starts_with("Kind: Image")));
    assert!(
        !fast.iter().any(|l| l.starts_with("Size:")),
        "no fs lines yet"
    );
    let mut ready = false;
    for _ in 0..250 {
        app.tick();
        if !app.info.as_ref().unwrap().pending {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    app.close_info();
    assert!(app.info.is_none());

    // Folder entry shows folder kind and a size measured by a background
    // walk that keeps running after the dialog is dismissed. The dialog
    // opens instantly with fast lines; `tick()` applies the worker's Meta
    // event and then the walk's final size line.
    let mut dir_entry = entry(base.join("sub").to_str().unwrap());
    dir_entry.is_dir = true;
    app.panes[0].files = vec![dir_entry];
    app.panes[0].state.select(Some(0));
    app.show_info();
    let lines = app.info.as_ref().unwrap().lines.clone();
    assert!(lines.iter().any(|l| l.starts_with("Kind: Folder")));
    assert!(
        !lines.iter().any(|l| l.starts_with("Size:")),
        "no static Size line while walking"
    );
    let mut measured = false;
    for _ in 0..500 {
        app.tick();
        let d = app.info.as_ref().unwrap();
        if !d.pending && d.lines.iter().any(|l| l.starts_with("Size:")) {
            measured = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(measured, "background folder-size walk should report back");
    let lines = app.info.as_ref().unwrap().lines.clone();
    let size_line = lines.iter().find(|l| l.starts_with("Size:")).unwrap();
    assert!(
        size_line.contains("0 B") && size_line.contains("0 items"),
        "empty folder measures to 0 bytes: {size_line:?}"
    );
    // The completed measurement is cached and annotates the file list.
    let si = app.size_info(base.join("sub").to_str().unwrap()).unwrap();
    assert!(si.complete && si.bytes == 0 && si.items == 0);

    // Dismissing the dialog and re-querying shows the cached size again
    // (no new walk for a completed folder).
    app.close_info();
    app.show_info();
    let mut ready = false;
    for _ in 0..250 {
        app.tick();
        let d = app.info.as_ref().unwrap();
        if !d.pending && d.lines.iter().any(|l| l.starts_with("Size:")) {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(ready, "cached size should reappear after re-query");
}

#[test]
fn delete_keys_depend_on_platform() {
    use ira::handler::is_delete_key;
    use ratatui::crossterm::event::KeyCode;

    // Del triggers the delete flow everywhere.
    assert!(is_delete_key(KeyCode::Delete));

    // Backspace joins it only on macOS (the key labeled "delete" there).
    #[cfg(target_os = "macos")]
    assert!(is_delete_key(KeyCode::Backspace));

    #[cfg(not(target_os = "macos"))]
    assert!(
        !is_delete_key(KeyCode::Backspace),
        "on Win/Linux backspace stays free"
    );
}

#[test]
fn backspace_opens_delete_confirmation_on_macos() {
    use ira::app::App;
    use ira::domain::data::Folder;
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_bksp_del_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let victim = base.join("victim.txt");
    std::fs::write(&victim, "d").unwrap();

    let mut app = App::default();
    app.panes[0].files = vec![FEntry {
        path: victim.to_str().unwrap().to_string(),
        label: "victim.txt".to_string(),
        is_dir: false,
        size: 0,
        modified: None,
    }];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));

    handle_key_events(
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()),
        &mut app,
    )
    .unwrap();

    #[cfg(target_os = "macos")]
    assert!(
        app.confirming.is_some(),
        "macOS: backspace opens the delete confirmation"
    );
    #[cfg(not(target_os = "macos"))]
    assert!(
        app.confirming.is_none(),
        "other platforms: backspace stays free"
    );
    assert!(victim.exists(), "nothing deleted without confirmation");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn confirmed_search_becomes_sticky_filter() {
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_filter_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("alpha.txt"), "a").unwrap();
    std::fs::write(base.join("beta.txt"), "b").unwrap();
    std::fs::write(base.join("gamma.txt"), "g").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));
    app.panes[0].files = vec![
        entry(base.join("alpha.txt").to_str().unwrap()),
        entry(base.join("beta.txt").to_str().unwrap()),
        entry(base.join("gamma.txt").to_str().unwrap()),
    ];
    app.panes[0].selected = vec![false, false, false];
    app.panes[0].state.select(Some(0));
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());

    // `/` + "beta" + Enter: the filter becomes the pane's working view.
    handle_key_events(key(KeyCode::Char('/')), &mut app).unwrap();
    for ch in "beta".chars() {
        handle_key_events(key(KeyCode::Char(ch)), &mut app).unwrap();
    }
    handle_key_events(key(KeyCode::Enter), &mut app).unwrap();

    assert!(!app.is_searching(), "typing mode ends on Enter");
    assert_eq!(app.visible_count(), 1, "only beta matches 'beta'");
    assert_eq!(app.panes[0].filter_query.as_deref(), Some("beta"));

    // Actions operate on the filtered view: space selects beta (underlying
    // files index 1), copy sources = beta only.
    handle_key_events(key(KeyCode::Char(' ')), &mut app).unwrap();
    assert_eq!(app.panes[0].selected, vec![false, true, false]);

    // Esc clears the filter: everything visible again, cursor parked on the
    // previously filtered entry (beta, underlying index 1).
    handle_key_events(key(KeyCode::Esc), &mut app).unwrap();
    assert!(app.panes[0].filter_query.is_none());
    assert_eq!(app.visible_count(), 3);
    assert_eq!(app.panes[0].state.selected(), Some(1));

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn esc_while_typing_returns_to_confirmed_filter() {
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_filter2_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    for name in ["alpha.txt", "beta.txt", "gamma.txt"] {
        std::fs::write(base.join(name), "x").unwrap();
    }

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));
    app.panes[0].files = vec![
        entry(base.join("alpha.txt").to_str().unwrap()),
        entry(base.join("beta.txt").to_str().unwrap()),
        entry(base.join("gamma.txt").to_str().unwrap()),
    ];
    app.panes[0].selected = vec![false, false, false];
    app.panes[0].state.select(Some(0));
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());

    // Confirm "beta" as the filter...
    handle_key_events(key(KeyCode::Char('/')), &mut app).unwrap();
    for ch in "beta".chars() {
        handle_key_events(key(KeyCode::Char(ch)), &mut app).unwrap();
    }
    handle_key_events(key(KeyCode::Enter), &mut app).unwrap();
    assert_eq!(app.visible_count(), 1);

    // ...then `/` starts a NEW search over the full folder; Esc while
    // typing returns to the previously confirmed filter.
    handle_key_events(key(KeyCode::Char('/')), &mut app).unwrap();
    assert_eq!(app.visible_count(), 3, "typing searches the full folder");
    handle_key_events(key(KeyCode::Esc), &mut app).unwrap();
    assert!(!app.is_searching());
    assert_eq!(app.panes[0].filter_query.as_deref(), Some("beta"));
    assert_eq!(app.visible_count(), 1);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn filter_reapplies_after_listing_refresh_and_survives_deletion() {
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_filter3_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("beta.txt"), "b").unwrap();
    std::fs::write(base.join("beta2.txt"), "b").unwrap();
    std::fs::write(base.join("gamma.txt"), "g").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));
    app.panes[0].files = vec![
        entry(base.join("beta.txt").to_str().unwrap()),
        entry(base.join("beta2.txt").to_str().unwrap()),
        entry(base.join("gamma.txt").to_str().unwrap()),
    ];
    app.panes[0].selected = vec![false, false, false];
    app.panes[0].state.select(Some(0));
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());

    // Filter "beta" -> two matches.
    handle_key_events(key(KeyCode::Char('/')), &mut app).unwrap();
    for ch in "beta".chars() {
        handle_key_events(key(KeyCode::Char(ch)), &mut app).unwrap();
    }
    handle_key_events(key(KeyCode::Enter), &mut app).unwrap();
    assert_eq!(app.visible_count(), 2);

    // Select both matches and confirm deletion; the background job deletes
    // them and the refresh re-applies the filter to the new listing.
    app.toggle_select_all();
    assert_eq!(
        app.panes[0].selected,
        vec![true, true, false],
        "select-all must operate on the filtered set"
    );
    app.request_delete();
    let paths = app.confirming.as_ref().unwrap().paths.clone();
    app.confirm_delete();
    for _ in 0..250 {
        app.tick();
        if app.deletion.is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!base.join("beta.txt").exists());
    assert!(!base.join("beta2.txt").exists());
    assert!(base.join("gamma.txt").exists());
    assert_eq!(app.panes[0].filter_query.as_deref(), Some("beta"));
    assert_eq!(app.visible_count(), 0, "no beta files remain");

    // Esc: full folder again.
    handle_key_events(key(KeyCode::Esc), &mut app).unwrap();
    assert_eq!(app.visible_count(), 1);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn filter_reapplies_after_hidden_toggle() {
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_filter4_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("beta.txt"), "b").unwrap();
    std::fs::write(base.join(".beta-hidden"), "h").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));
    // Seed only the visible file (hidden files are excluded by default).
    app.panes[0].files = vec![entry(base.join("beta.txt").to_str().unwrap())];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());

    handle_key_events(key(KeyCode::Char('/')), &mut app).unwrap();
    for ch in "beta".chars() {
        handle_key_events(key(KeyCode::Char(ch)), &mut app).unwrap();
    }
    handle_key_events(key(KeyCode::Enter), &mut app).unwrap();
    assert_eq!(app.visible_count(), 1);

    // `.` re-lists with hidden files; the filter re-applies to the new list
    // and the hidden beta file joins the filtered view.
    handle_key_events(key(KeyCode::Char('.')), &mut app).unwrap();
    for _ in 0..100 {
        app.tick();
        if app.file_list_settled(0) && app.visible_count() == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(app.visible_count(), 2, "hidden beta file joins the filter");
    assert_eq!(
        app.panes[0].filter_query.as_deref(),
        Some("beta"),
        "filter must survive the refresh"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn new_entry_extension_decides_kind() {
    use ira::app::App;
    use ira::domain::data::Folder;
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_new_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());
    let type_text = |app: &mut App, text: &str| {
        for ch in text.chars() {
            app.new_entry_insert(ch);
        }
    };

    // Folder: no extension.
    app.start_new_entry();
    type_text(&mut app, "newfolder");
    app.confirm_new_entry();
    assert!(app.new_entry.is_none(), "dialog closes on success");
    assert!(base.join("newfolder").is_dir());

    // File: extension present.
    app.start_new_entry();
    type_text(&mut app, "notes.txt");
    app.confirm_new_entry();
    assert!(base.join("notes.txt").is_file());
    let content = std::fs::read(base.join("notes.txt")).unwrap();
    assert!(content.is_empty(), "new file starts empty");

    // Multi-dot name is a file.
    app.start_new_entry();
    type_text(&mut app, "data.v2.json");
    app.confirm_new_entry();
    assert!(base.join("data.v2.json").is_file());

    // Leading-dot name is a folder (no visible extension).
    app.start_new_entry();
    type_text(&mut app, ".config");
    app.confirm_new_entry();
    assert!(base.join(".config").is_dir());

    // Trailing dot is a folder (empty suffix).
    app.start_new_entry();
    type_text(&mut app, "backup.");
    app.confirm_new_entry();
    assert!(base.join("backup.").is_dir());

    // Existing name is rejected with an error status and nothing changes.
    app.start_new_entry();
    type_text(&mut app, "notes.txt");
    app.confirm_new_entry();
    let st = app.status.as_ref().expect("existing name must error");
    assert!(st.is_error && st.text.contains("already exists"));

    // Nested path under a plain name: parent folders are created.
    app.start_new_entry();
    type_text(&mut app, "a/b");
    app.confirm_new_entry();
    assert!(base.join("a").is_dir());
    assert!(base.join("a/b").is_dir());

    // Empty name is ignored (dialog stays open).
    app.cancel_new_entry();
    app.start_new_entry();
    app.confirm_new_entry();
    assert!(app.new_entry.is_some(), "empty name keeps the dialog open");
    app.cancel_new_entry();

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn new_entry_selects_created_item_and_works_via_handler() {
    use ira::app::App;
    use ira::domain::data::Folder;
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_new2_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));
    app.list_files_from_selected_folder();
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());

    // `n` opens the dialog (normal mode routing).
    handle_key_events(key(KeyCode::Char('n')), &mut app).unwrap();
    assert!(app.new_entry.is_some(), "n must open the create dialog");

    // Esc cancels.
    handle_key_events(key(KeyCode::Esc), &mut app).unwrap();
    assert!(app.new_entry.is_none());

    // Create a file and wait for the (async) refresh to select it.
    handle_key_events(key(KeyCode::Char('n')), &mut app).unwrap();
    for ch in "created.txt".chars() {
        handle_key_events(key(KeyCode::Char(ch)), &mut app).unwrap();
    }
    handle_key_events(key(KeyCode::Enter), &mut app).unwrap();

    let mut selected_path = None;
    for _ in 0..100 {
        app.tick();
        let idx = app.panes[0].state.selected();
        if let (Some(i), true) = (idx, app.file_list_settled(0)) {
            selected_path = app.panes[0].files.get(i).map(|f| f.path.clone());
            if selected_path
                .as_deref()
                .is_some_and(|p| p.ends_with("created.txt"))
            {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let p = selected_path.expect("created file must be selected after refresh");
    assert!(p.ends_with("created.txt"), "got {p:?}");

    // Rename must still work (Backspace in rename editor unaffected).
    handle_key_events(key(KeyCode::Enter), &mut app).unwrap();
    assert!(app.renaming.is_some(), "Enter on the entry opens rename");
    handle_key_events(key(KeyCode::Esc), &mut app).unwrap();

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn batch_copy_of_thousands_stays_responsive() {
    let base = std::env::temp_dir().join(format!("ira_stress_{}", std::process::id()));
    let src = base.join("src");
    let dst = base.join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    for i in 0..8000 {
        std::fs::write(src.join(format!("f{i:05}.txt")), "x".repeat(50)).unwrap();
    }

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("src".into(), src.to_str().unwrap().into(), '#'));
    app.panes[1].folder = Some(Folder::new("dst".into(), dst.to_str().unwrap().into(), '#'));
    app.panes[0].files = (0..8000)
        .map(|i| entry(src.join(format!("f{i:05}.txt")).to_str().unwrap()))
        .collect();
    app.panes[0].selected = vec![true; 8000];

    app.request_copy();
    app.confirm_pending();

    // THE fix: one batch job instead of one job per file.
    assert_eq!(app.jobs.len(), 1, "8000 files must be one batch job");
    assert_eq!(app.jobs[0].paths.len(), 8000);

    // The UI thread must stay responsive while the copy runs: tick() (which
    // drains progress events) must never take anywhere near a freeze.
    let mut max_tick_us = 0u128;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        let t0 = std::time::Instant::now();
        app.tick();
        let dt = t0.elapsed().as_micros();
        max_tick_us = max_tick_us.max(dt);
        if matches!(app.jobs[0].status, JobStatus::Done | JobStatus::Failed(_)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        max_tick_us < 50_000,
        "tick must stay responsive: {max_tick_us}µs"
    );
    assert!(
        matches!(app.jobs[0].status, JobStatus::Done),
        "batch must complete"
    );
    assert_eq!(std::fs::read_dir(&dst).unwrap().count(), 8000);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn multi_selection_shows_aggregate_and_sums_walks() {
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{backend::TestBackend, Terminal};

    let base = std::env::temp_dir().join(format!("ira_multi_{}", std::process::id()));
    let d1 = base.join("dir1");
    let d2 = base.join("dir2");
    std::fs::create_dir_all(&d1).unwrap();
    std::fs::create_dir_all(&d2).unwrap();
    std::fs::write(d1.join("f"), "x".repeat(100)).unwrap();
    std::fs::write(d2.join("f"), "x".repeat(50)).unwrap();
    std::fs::write(base.join("file1.txt"), "x".repeat(10)).unwrap();
    std::fs::write(base.join("file2.txt"), "x".repeat(5)).unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));
    let dir1_entry = FEntry {
        path: d1.to_str().unwrap().to_string(),
        label: "dir1".to_string(),
        is_dir: true,
        size: 0,
        modified: None,
    };
    let dir2_entry = FEntry {
        path: d2.to_str().unwrap().to_string(),
        label: "dir2".to_string(),
        is_dir: true,
        size: 0,
        modified: None,
    };
    app.panes[0].files = vec![
        dir1_entry,
        dir2_entry,
        entry(base.join("file1.txt").to_str().unwrap()),
        entry(base.join("file2.txt").to_str().unwrap()),
    ];
    app.panes[0].selected = vec![true, true, true, true];
    app.panes[0].state.select(Some(0));
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());

    app.show_info();

    // Multi dialog: composition of the selection.
    let m = app.multi_info.as_ref().expect("multi dialog must open");
    eprintln!(
        "folders={} files={} paths={:?}",
        m.folders, m.files, m.paths
    );
    assert_eq!(m.folders, 2);
    assert_eq!(m.files, 2);

    // Walks spawned for both folders.
    assert!(app.size_walk_started(d1.to_str().unwrap()).is_some());
    assert!(app.size_walk_started(d2.to_str().unwrap()).is_some());

    // Render: selection line present while calculating.
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal.draw(|f| ira::ui::render(&mut app, f)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("2 folders / 2 files selected"), "{text}");
    assert!(text.contains("calculating"), "{text}");

    // Wait for all walks + file stats to complete, then the aggregate sums.
    let mut complete = false;
    for _ in 0..250 {
        app.tick();
        let (c, bytes, _, _) = app.multi_info_aggregate();
        if c {
            complete = true;
            assert_eq!(bytes, 100 + 50 + 10 + 5);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(complete, "aggregate must complete");

    // Final render: summed sizes, no calculating.
    terminal.draw(|f| ira::ui::render(&mut app, f)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    eprintln!(
        "DBG2 has Info: {} | has selected: {}",
        text.contains("Info"),
        text.contains("selected")
    );
    if let Some(i) = text.find("Info") {
        eprintln!("DBG2 region: {:?}", &text[i..i + 480]);
    }
    assert!(text.contains("165 data"), "{text}");
    assert!(text.contains("(4 items)"), "{text}");
    assert!(!text.contains("calculating"), "{text}");

    // Any key closes the dialog; walks stay cached.
    handle_key_events(key(KeyCode::Esc), &mut app).unwrap();
    assert!(app.multi_info.is_none());
    let cached: u64 = [
        d1.as_path(),
        d2.as_path(),
        base.join("file1.txt").as_path(),
        base.join("file2.txt").as_path(),
    ]
    .iter()
    .filter_map(|p| app.size_info(p.to_str().unwrap()))
    .map(|si| si.bytes)
    .sum();
    assert_eq!(cached, 165, "cache keeps the sums");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn single_selection_still_uses_single_info_dialog() {
    let base = std::env::temp_dir().join(format!("ira_single_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("only.txt"), "x").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));
    app.panes[0].files = vec![entry(base.join("only.txt").to_str().unwrap())];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));

    app.show_info();
    assert!(app.multi_info.is_none(), "single selection = single dialog");
    assert!(app.info.is_some());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn goto_navigates_to_existing_folder_and_selects_existing_file() {
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_goto_{}", std::process::id()));
    let sub = base.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("target.txt"), "x").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new(
        "t".into(),
        std::env::temp_dir().to_string_lossy().into_owned(),
        '#',
    ));
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());

    // `[` opens the dialog; typing accumulates.
    handle_key_events(key(KeyCode::Char('[')), &mut app).unwrap();
    assert!(app.goto_prompt.is_some());
    for ch in sub.to_str().unwrap().chars() {
        handle_key_events(key(KeyCode::Char(ch)), &mut app).unwrap();
    }
    handle_key_events(key(KeyCode::Enter), &mut app).unwrap();

    assert!(app.goto_prompt.is_none(), "dialog closes on success");
    assert_eq!(
        app.panes[0].folder.as_ref().unwrap().path,
        sub.to_str().unwrap()
    );

    // File path: navigates to the containing folder and selects the file.
    let file_path = sub.join("target.txt");
    handle_key_events(key(KeyCode::Char('[')), &mut app).unwrap();
    for ch in file_path.to_str().unwrap().chars() {
        handle_key_events(key(KeyCode::Char(ch)), &mut app).unwrap();
    }
    handle_key_events(key(KeyCode::Enter), &mut app).unwrap();
    assert_eq!(
        app.panes[0].folder.as_ref().unwrap().path,
        sub.to_str().unwrap()
    );
    for _ in 0..100 {
        app.tick();
        if let Some(i) = app.panes[0].state.selected() {
            if app.panes[0]
                .files
                .get(i)
                .is_some_and(|f| f.label == "target.txt")
            {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let sel = app.panes[0].state.selected().unwrap();
    assert_eq!(
        app.panes[0].files.get(sel).unwrap().label,
        "target.txt",
        "the file must be selected after navigation"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn goto_creates_missing_nested_file_and_folder() {
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_goto_create_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new(
        "t".into(),
        base.to_str().unwrap().to_string(),
        '#',
    ));
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());

    // Missing nested file path: creates 2 folders + the file, navigates to
    // the containing folder and selects the file.
    let target = base.join("a/b/hyprland.conf");
    handle_key_events(key(KeyCode::Char('[')), &mut app).unwrap();
    for ch in target.to_str().unwrap().chars() {
        handle_key_events(key(KeyCode::Char(ch)), &mut app).unwrap();
    }
    handle_key_events(key(KeyCode::Enter), &mut app).unwrap();

    assert!(target.is_file(), "nested file must be created");
    assert!(target.parent().unwrap().is_dir());
    assert_eq!(
        app.panes[0].folder.as_ref().unwrap().path,
        target.parent().unwrap().to_string_lossy()
    );

    // Missing nested folder path (no extension): folder created, pane opens
    // inside it.
    let folder = base.join("x/y/z");
    handle_key_events(key(KeyCode::Char('[')), &mut app).unwrap();
    for ch in folder.to_str().unwrap().chars() {
        handle_key_events(key(KeyCode::Char(ch)), &mut app).unwrap();
    }
    handle_key_events(key(KeyCode::Enter), &mut app).unwrap();
    assert!(folder.is_dir());
    assert_eq!(
        app.panes[0].folder.as_ref().unwrap().path,
        folder.to_string_lossy()
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn goto_expands_home_and_relative_paths() {
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_goto_rel_{}", std::process::id()));
    let sub = base.join("sub");
    std::fs::create_dir_all(&sub).unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());

    // Relative path resolves against the pane's folder.
    handle_key_events(key(KeyCode::Char('[')), &mut app).unwrap();
    for ch in "sub".chars() {
        handle_key_events(key(KeyCode::Char(ch)), &mut app).unwrap();
    }
    handle_key_events(key(KeyCode::Enter), &mut app).unwrap();
    assert_eq!(
        app.panes[0].folder.as_ref().unwrap().path,
        sub.to_str().unwrap()
    );

    // Tilde expansion.
    handle_key_events(key(KeyCode::Char('[')), &mut app).unwrap();
    for ch in "~/".chars() {
        handle_key_events(key(KeyCode::Char(ch)), &mut app).unwrap();
    }
    app.confirm_goto();
    let home = dirs_next::home_dir().unwrap();
    assert_eq!(
        app.panes[0].folder.as_ref().unwrap().path,
        home.to_string_lossy()
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn paste_routes_to_goto_prompt() {
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new(
        "t".into(),
        std::env::temp_dir().to_string_lossy().into_owned(),
        '#',
    ));

    handle_key_events(
        KeyEvent::new(KeyCode::Char('['), KeyModifiers::empty()),
        &mut app,
    )
    .unwrap();
    app.handle_paste("/some/pasted/path");
    assert_eq!(app.goto_prompt.as_deref(), Some("/some/pasted/path"));
    // Multi-line paste keeps its content (dialog trims on confirm).
    app.handle_paste("more\nnoise");
    assert!(
        app.goto_prompt
            .as_deref()
            .is_some_and(|p| p.starts_with("/some/pasted/pathmore")),
        "paste appends"
    );
}

#[test]
fn new_entry_supports_nested_paths() {
    use ira::domain::data::Folder;
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_new_nested_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));

    // Nested file: 2 folders + 1 file.
    app.start_new_entry();
    for ch in "xdg-desktop-portal/hyprland/portals.conf".chars() {
        app.new_entry_insert(ch);
    }
    app.confirm_new_entry();
    let created = base.join("xdg-desktop-portal/hyprland/portals.conf");
    assert!(created.is_file(), "nested file must be created");
    assert!(created.parent().unwrap().is_dir());

    // Listing refresh selects the created file and shows its parent folder
    // contents.
    for _ in 0..100 {
        app.tick();
        if app.file_list_settled(0) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(app.panes[0].files.iter().any(|f| f.label == "portals.conf"));

    // After the nested create the pane navigated into the deepest folder;
    // subsequent relative creates anchor there.
    app.start_new_entry();
    for ch in "a/b/c".chars() {
        app.new_entry_insert(ch);
    }
    app.confirm_new_entry();
    assert!(base.join("xdg-desktop-portal/hyprland/a/b/c").is_dir());

    // The nested create left the pane inside a/b/c. Two Lefts go to a/;
    // a new folder created there lands beside the nested chain.
    handle_key_events(
        KeyEvent::new(KeyCode::Left, KeyModifiers::empty()),
        &mut app,
    )
    .unwrap();
    handle_key_events(
        KeyEvent::new(KeyCode::Left, KeyModifiers::empty()),
        &mut app,
    )
    .unwrap();
    app.start_new_entry();
    for ch in "new-gate".chars() {
        app.new_entry_insert(ch);
    }
    app.confirm_new_entry();
    assert!(
        base.join("xdg-desktop-portal/hyprland/a/new-gate").is_dir(),
        "new-gate must be created in the pane's current folder"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn create_never_truncates_existing_sibling() {
    use ira::app::App;
    use ira::domain::data::Folder;

    let base = std::env::temp_dir().join(format!("ira_nosibling_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let sibling = base.join("hyprland-portals.conf");
    std::fs::write(&sibling, "important").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));

    app.start_new_entry();
    for ch in "hyprland-portals.config".chars() {
        app.new_entry_insert(ch);
    }
    app.confirm_new_entry();

    let created = base.join("hyprland-portals.config");
    assert!(created.is_file(), "the new file must be created");
    let created_content = std::fs::read(&created).unwrap();
    assert!(created_content.is_empty(), "the new file must be empty");
    let kept = std::fs::read_to_string(&sibling).unwrap();
    assert_eq!(kept, "important", "the pre-existing sibling must survive");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn goto_never_truncates_existing_file() {
    use ira::app::App;
    use ira::domain::data::Folder;

    let base = std::env::temp_dir().join(format!("ira_gotonotrim_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let conf = base.join("conf");
    std::fs::write(&conf, "keep").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));

    // The exact existing path: must navigate, never truncate.
    app.goto_prompt = Some(conf.to_string_lossy().into_owned());
    app.confirm_goto();

    assert_eq!(
        app.panes[0].folder.as_ref().unwrap().path,
        base.to_str().unwrap(),
        "pane must navigate to the file's folder"
    );
    let sel = app.panes[0].state.selected();
    assert!(sel.is_some(), "the file must be selected after navigation");
    assert_eq!(
        app.panes[0].files.get(sel.unwrap()).unwrap().label,
        "conf",
        "the existing file must be the selected entry"
    );
    let kept = std::fs::read_to_string(&conf).unwrap();
    assert_eq!(kept, "keep", "the existing file must survive untouched");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn goto_never_truncates_on_race() {
    use ira::app::App;
    use ira::domain::data::Folder;
    let base = std::env::temp_dir().join(format!("ira_gotorace_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let conf = base.join("conf");
    std::fs::write(&conf, "keep").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));

    // Two confirmations for the same existing path (the second simulates
    // the entry appearing between the existence check and the create):
    // both must navigate and neither may truncate.
    app.goto_prompt = Some(conf.to_string_lossy().into_owned());
    app.confirm_goto();
    app.goto_prompt = Some(conf.to_string_lossy().into_owned());
    app.confirm_goto();

    assert_eq!(
        app.panes[0].folder.as_ref().unwrap().path,
        base.to_str().unwrap()
    );
    let kept = std::fs::read_to_string(&conf).unwrap();
    assert_eq!(kept, "keep", "the existing file must survive both passes");
    let entries: Vec<_> = std::fs::read_dir(&base)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, vec!["conf".to_string()], "no second file created");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn n_dialog_expands_tilde() {
    use ira::app::App;
    use ira::domain::data::Folder;

    let base = std::env::temp_dir().join(format!("ira_ntilde_base_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let home = dirs_next::home_dir().expect("home dir must resolve");
    let target = home.join(format!("ira-tilde-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&target); // start clean

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));

    app.start_new_entry();
    for ch in format!("~/ira-tilde-test-{}", std::process::id()).chars() {
        app.new_entry_insert(ch);
    }
    app.confirm_new_entry();

    assert!(
        target.is_dir(),
        "the tilde name must expand to the real home, not a literal '~' folder"
    );
    assert!(
        !base.join("~").exists(),
        "no literal '~' folder may be created inside the pane"
    );

    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(&target);
}

#[test]

fn n_dialog_existing_nested_path_errors() {
    use ira::app::App;
    use ira::domain::data::Folder;
    let base = std::env::temp_dir().join(format!("ira_nnested_{}", std::process::id()));
    std::fs::create_dir_all(base.join("x")).unwrap();
    std::fs::write(base.join("x/y"), "precious").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));

    app.start_new_entry();
    for ch in "x/y".chars() {
        app.new_entry_insert(ch);
    }
    app.confirm_new_entry();

    let st = app
        .status
        .as_ref()
        .expect("existing nested path must error");
    assert!(st.is_error && st.text.contains("already exists"));
    let kept = std::fs::read_to_string(base.join("x/y")).unwrap();
    assert_eq!(kept, "precious", "the existing file must be untouched");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn hidden_toggle_persists_across_restart() {
    use ira::domain::data::Folder;

    let base = std::env::temp_dir().join(format!("ira_hidden_persist_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("visible.txt"), "v").unwrap();
    std::fs::write(base.join(".hidden"), "h").unwrap();
    let state_file = base.join("state");

    let mut app = App::default();
    app.state_path = Some(state_file.clone());
    app.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));
    app.list_files_from_selected_folder();

    // Toggle hidden on and persist (toggle_hidden persists immediately).
    app.toggle_hidden();
    assert!(app.show_hidden);
    assert!(state_file.exists(), "state must be written on toggle");

    // Simulate restart: fresh app restores from the same state file.
    let mut app2 = App::default();
    app2.state_path = Some(state_file.clone());
    app2.panes[0].folder = Some(Folder::new("t".into(), base.to_str().unwrap().into(), '#'));
    app2.restore_state();
    assert!(app2.show_hidden, "hidden setting must survive restart");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn out_of_folder_selects_the_folder_you_came_from() {
    use ira::domain::data::Folder;

    // Small folder: the sync fast path applies the listing immediately.
    let base = std::env::temp_dir().join(format!("ira_outof_{}", std::process::id()));
    std::fs::create_dir_all(base.join("child")).unwrap();
    std::fs::write(base.join("sibling.txt"), "s").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new(
        "child".into(),
        base.join("child").to_str().unwrap().into(),
        '#',
    ));
    app.list_files_from_selected_folder();

    app.out_of_folder();

    assert_eq!(
        app.panes[0].folder.as_ref().unwrap().path,
        base.to_str().unwrap(),
        "Left navigates to the parent"
    );
    let sel = app.panes[0]
        .state
        .selected()
        .expect("cursor must land on the folder we came from");
    assert_eq!(
        app.panes[0].files[sel].path,
        base.join("child").to_str().unwrap()
    );
    assert!(app.panes[0].files[sel].is_dir);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn out_of_folder_selects_origin_in_streamed_listing() {
    use ira::domain::data::Folder;

    // Big folder (> LISTING_CHUNK entries): Left must stream the parent in
    // the background and still land the cursor on the child we came from.
    let base = std::env::temp_dir().join(format!("ira_outof_big_{}", std::process::id()));
    std::fs::create_dir_all(base.join("child")).unwrap();
    for i in 0..600 {
        std::fs::write(base.join(format!("f{i:03}.txt")), "x").unwrap();
    }

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new(
        "child".into(),
        base.join("child").to_str().unwrap().into(),
        '#',
    ));
    app.list_files_from_selected_folder();

    app.out_of_folder();

    let deadline = Instant::now() + Duration::from_secs(10);
    while !app.file_list_settled(0) {
        assert!(Instant::now() < deadline, "streamed listing never settled");
        app.tick();
        std::thread::sleep(Duration::from_millis(5));
    }

    let sel = app.panes[0]
        .state
        .selected()
        .expect("cursor must land on the folder we came from after settle");
    assert_eq!(
        app.panes[0].files[sel].path,
        base.join("child").to_str().unwrap()
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn creating_entry_refreshes_other_pane_showing_same_folder() {
    use ira::domain::data::Folder;

    let base = std::env::temp_dir().join(format!("ira_sync2_{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();

    let folder = || {
        Some(Folder::new(
            "same".into(),
            base.to_str().unwrap().into(),
            '#',
        ))
    };
    let mut app = App::default();
    app.panes[0].folder = folder();
    app.panes[1].folder = folder();

    // `n` create in the active pane; the other pane shows the same folder.
    app.start_new_entry();
    for ch in "fresh.txt".chars() {
        app.new_entry_insert(ch);
    }
    app.confirm_new_entry();
    assert!(base.join("fresh.txt").is_file());
    assert!(
        app.panes[0].files.iter().any(|f| f.label == "fresh.txt"),
        "active pane must list the new file"
    );
    assert!(
        app.panes[1].files.iter().any(|f| f.label == "fresh.txt"),
        "other pane on the same folder must refresh too"
    );

    // `[` go-to creation navigates the active pane; same sync applies.
    app.start_goto();
    for ch in "goto_made.md".chars() {
        app.goto_push(&ch.to_string());
    }
    app.confirm_goto();
    assert!(base.join("goto_made.md").is_file());
    assert!(
        app.panes[1].files.iter().any(|f| f.label == "goto_made.md"),
        "other pane on the same folder must refresh after a go-to create"
    );

    // A different folder in the other pane is left alone (no refresh).
    let other_dir = base.join("elsewhere");
    std::fs::create_dir_all(&other_dir).unwrap();
    app.panes[1].folder = Some(Folder::new(
        "elsewhere".into(),
        other_dir.to_str().unwrap().into(),
        '#',
    ));
    app.start_new_entry();
    for ch in "quiet.txt".chars() {
        app.new_entry_insert(ch);
    }
    app.confirm_new_entry();
    assert!(base.join("quiet.txt").is_file());
    assert!(
        !app.panes[1].files.iter().any(|f| f.label == "quiet.txt"),
        "other pane on a different folder must not change"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn overwrite_policy_autorenames_collisions() {
    let base = std::env::temp_dir().join(format!("ira_ovr_ar_{}", std::process::id()));
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::create_dir_all(base.join("dst")).unwrap();
    std::fs::write(base.join("src").join("a.txt"), "NEW").unwrap();
    std::fs::write(base.join("dst").join("a.txt"), "OLD").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new(
        "src".into(),
        base.join("src").to_str().unwrap().into(),
        '#',
    ));
    app.panes[1].folder = Some(Folder::new(
        "dst".into(),
        base.join("dst").to_str().unwrap().into(),
        '#',
    ));
    app.panes[0].files = vec![entry(base.join("src").join("a.txt").to_str().unwrap())];
    app.panes[0].state.select(Some(0));

    app.request_copy();
    app.confirming.as_mut().unwrap().policy = OverwritePolicy::AutoRename;
    app.confirm_pending();
    wait_for_jobs(&mut app);

    // Old untouched, new content in the renamed sibling.
    assert_eq!(
        std::fs::read(base.join("dst").join("a.txt")).unwrap(),
        b"OLD"
    );
    assert_eq!(
        std::fs::read(base.join("dst").join("a (2).txt")).unwrap(),
        b"NEW"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn overwrite_policy_overwrite_replaces_existing() {
    let base = std::env::temp_dir().join(format!("ira_ovr_ow_{}", std::process::id()));
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::create_dir_all(base.join("dst")).unwrap();
    std::fs::write(base.join("src").join("a.txt"), "NEW").unwrap();
    std::fs::write(base.join("dst").join("a.txt"), "OLD").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new(
        "src".into(),
        base.join("src").to_str().unwrap().into(),
        '#',
    ));
    app.panes[1].folder = Some(Folder::new(
        "dst".into(),
        base.join("dst").to_str().unwrap().into(),
        '#',
    ));
    app.panes[0].files = vec![entry(base.join("src").join("a.txt").to_str().unwrap())];
    app.panes[0].state.select(Some(0));

    app.request_copy();
    app.confirming.as_mut().unwrap().policy = OverwritePolicy::Overwrite;
    app.confirm_pending();
    wait_for_jobs(&mut app);

    assert_eq!(
        std::fs::read(base.join("dst").join("a.txt")).unwrap(),
        b"NEW",
        "overwrite replaces the destination content"
    );
    assert!(!base.join("dst").join("a (2).txt").exists());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn overwrite_policy_skip_existing_keeps_destination() {
    let base = std::env::temp_dir().join(format!("ira_ovr_sk_{}", std::process::id()));
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::create_dir_all(base.join("dst")).unwrap();
    std::fs::write(base.join("src").join("a.txt"), "NEW").unwrap();
    std::fs::write(base.join("dst").join("a.txt"), "OLD").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new(
        "src".into(),
        base.join("src").to_str().unwrap().into(),
        '#',
    ));
    app.panes[1].folder = Some(Folder::new(
        "dst".into(),
        base.join("dst").to_str().unwrap().into(),
        '#',
    ));
    app.panes[0].files = vec![entry(base.join("src").join("a.txt").to_str().unwrap())];
    app.panes[0].state.select(Some(0));

    app.request_copy();
    app.confirming.as_mut().unwrap().policy = OverwritePolicy::SkipExisting;
    app.confirm_pending();
    wait_for_jobs(&mut app);

    assert_eq!(
        std::fs::read(base.join("dst").join("a.txt")).unwrap(),
        b"OLD"
    );
    assert!(!base.join("dst").join("a (2).txt").exists());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn policy_cycles_through_dialog_keys() {
    use ira::handler::handle_key_events;
    use ira::services::transfer::OverwritePolicy as P;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_ovr_cyc_{}", std::process::id()));
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::create_dir_all(base.join("dst")).unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new(
        "src".into(),
        base.join("src").to_str().unwrap().into(),
        '#',
    ));
    app.panes[1].folder = Some(Folder::new(
        "dst".into(),
        base.join("dst").to_str().unwrap().into(),
        '#',
    ));
    app.panes[0].files = vec![entry(base.join("src").join("a.txt").to_str().unwrap())];
    app.panes[0].state.select(Some(0));
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());

    app.request_copy();
    let policy = |app: &App| app.confirming.as_ref().unwrap().policy;
    assert_eq!(policy(&app), P::AutoRename, "default is auto-rename");

    handle_key_events(key(KeyCode::Char('o')), &mut app).unwrap();
    assert_eq!(policy(&app), P::Overwrite);
    handle_key_events(key(KeyCode::Char('o')), &mut app).unwrap();
    assert_eq!(policy(&app), P::SkipExisting);
    handle_key_events(key(KeyCode::Char('o')), &mut app).unwrap();
    assert_eq!(policy(&app), P::AutoRename, "cycles back");

    // The policy must survive into the spawned job.
    handle_key_events(key(KeyCode::Char('o')), &mut app).unwrap();
    handle_key_events(key(KeyCode::Enter), &mut app).unwrap();
    assert_eq!(app.jobs[0].overwrite, P::Overwrite);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn goto_navigation_then_select_and_copy_shows_confirm() {
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_goto_flow_{}", std::process::id()));
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::create_dir_all(base.join("dst")).unwrap();
    std::fs::write(base.join("src").join("a.txt"), "NEW").unwrap();
    std::fs::write(base.join("dst").join("a.txt"), "OLD").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new(
        "t".into(),
        std::env::temp_dir().to_string_lossy().into_owned(),
        '#',
    ));
    app.panes[1].folder = Some(Folder::new(
        "dst".into(),
        base.join("dst").to_str().unwrap().into(),
        '#',
    ));
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());

    handle_key_events(key(KeyCode::Char('[')), &mut app).unwrap();
    for ch in base.join("src").to_str().unwrap().chars() {
        handle_key_events(key(KeyCode::Char(ch)), &mut app).unwrap();
    }
    handle_key_events(key(KeyCode::Enter), &mut app).unwrap();
    for _ in 0..200 {
        app.tick();
        if app.file_list_settled(0) && app.visible_count() == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    eprintln!(
        "DBG settled={} cursor={:?} goto={:?} searching={}",
        app.file_list_settled(0),
        app.panes[0].state.selected(),
        app.goto_prompt,
        app.is_searching()
    );
    handle_key_events(key(KeyCode::Char(' ')), &mut app).unwrap();
    assert_eq!(app.panes[0].selected, vec![true]);
    handle_key_events(key(KeyCode::Char('c')), &mut app).unwrap();
    assert!(app.confirming.is_some());
    app.confirm_pending();
    wait_for_jobs(&mut app);
    // Default policy is auto-rename: existing dst/a.txt stays, the copy
    // lands as 'a (2).txt'.
    assert_eq!(
        std::fs::read(base.join("dst").join("a.txt")).unwrap(),
        b"OLD"
    );
    assert_eq!(
        std::fs::read(base.join("dst").join("a (2).txt")).unwrap(),
        b"NEW"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn copied_folder_appears_in_destination_pane_listing() {
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_cfld_{}", std::process::id()));
    let src = base.join("src");
    let dst = base.join("dst");
    std::fs::create_dir_all(src.join("myfolder")).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(src.join("myfolder").join("inner.txt"), "x").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("src".into(), src.to_str().unwrap().into(), '#'));
    app.panes[1].folder = Some(Folder::new("dst".into(), dst.to_str().unwrap().into(), '#'));
    app.panes[0].files = vec![FEntry {
        path: src.join("myfolder").to_str().unwrap().to_string(),
        label: "myfolder".to_string(),
        is_dir: true,
        size: 0,
        modified: None,
    }];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());

    // User flow: space-select the folder, c, confirm.
    handle_key_events(key(KeyCode::Char(' ')), &mut app).unwrap();
    handle_key_events(key(KeyCode::Char('c')), &mut app).unwrap();
    assert!(app.confirming.is_some());
    app.confirm_pending();
    wait_for_jobs(&mut app);

    assert!(dst.join("myfolder").is_dir(), "folder must be copied");
    // THE assertion under test: the destination pane's listing must show it
    // without any manual refresh.
    assert!(
        app.panes[1].files.iter().any(|f| f.label == "myfolder"),
        "copied folder must appear in the destination pane listing automatically"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn moved_folder_appears_in_destination_pane_listing() {
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_mfld_{}", std::process::id()));
    let src = base.join("src");
    let dst = base.join("dst");
    std::fs::create_dir_all(src.join("myfolder")).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(src.join("myfolder").join("inner.txt"), "x").unwrap();

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("src".into(), src.to_str().unwrap().into(), '#'));
    app.panes[1].folder = Some(Folder::new("dst".into(), dst.to_str().unwrap().into(), '#'));
    app.panes[0].files = vec![FEntry {
        path: src.join("myfolder").to_str().unwrap().to_string(),
        label: "myfolder".to_string(),
        is_dir: true,
        size: 0,
        modified: None,
    }];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());

    handle_key_events(key(KeyCode::Char(' ')), &mut app).unwrap();
    handle_key_events(key(KeyCode::Char('m')), &mut app).unwrap();
    app.confirm_pending();
    wait_for_jobs(&mut app);

    assert!(dst.join("myfolder").is_dir());
    assert!(
        app.panes[1].files.iter().any(|f| f.label == "myfolder"),
        "moved folder must appear in the destination pane listing"
    );
    assert!(!src.join("myfolder").exists());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn copied_folder_appears_in_large_destination_async_refresh() {
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_cfld_big_{}", std::process::id()));
    let src = base.join("src");
    let dst = base.join("dst");
    std::fs::create_dir_all(src.join("myfolder")).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(src.join("myfolder").join("inner.txt"), "x").unwrap();
    // Force the ASYNC streaming path: more entries than LISTING_CHUNK.
    for i in 0..700 {
        std::fs::write(dst.join(format!("pad{i:04}.txt")), "x").unwrap();
    }

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("src".into(), src.to_str().unwrap().into(), '#'));
    app.panes[1].folder = Some(Folder::new("dst".into(), dst.to_str().unwrap().into(), '#'));
    app.panes[0].files = vec![FEntry {
        path: src.join("myfolder").to_str().unwrap().to_string(),
        label: "myfolder".to_string(),
        is_dir: true,
        size: 0,
        modified: None,
    }];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());

    handle_key_events(key(KeyCode::Char(' ')), &mut app).unwrap();
    handle_key_events(key(KeyCode::Char('c')), &mut app).unwrap();
    app.confirm_pending();
    wait_for_jobs(&mut app);

    assert!(dst.join("myfolder").is_dir(), "folder must be copied");
    for _ in 0..500 {
        app.tick();
        if app.file_list_settled(1) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        app.panes[1].files.iter().any(|f| f.label == "myfolder"),
        "copied folder must appear in the destination pane listing (async path)"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn copied_folder_is_revealed_mid_stream_in_large_destination() {
    use ira::handler::handle_key_events;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let base = std::env::temp_dir().join(format!("ira_cfld_stream_{}", std::process::id()));
    let src = base.join("src");
    let dst = base.join("dst");
    std::fs::create_dir_all(src.join("myfolder")).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(src.join("myfolder").join("inner.txt"), "x").unwrap();
    for i in 0..700 {
        std::fs::write(dst.join(format!("pad{i:04}.txt")), "x").unwrap();
    }

    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new("src".into(), src.to_str().unwrap().into(), '#'));
    app.panes[1].folder = Some(Folder::new("dst".into(), dst.to_str().unwrap().into(), '#'));
    app.panes[0].files = vec![FEntry {
        path: src.join("myfolder").to_str().unwrap().to_string(),
        label: "myfolder".to_string(),
        is_dir: true,
        size: 0,
        modified: None,
    }];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));
    let key = |code| KeyEvent::new(code, KeyModifiers::empty());

    handle_key_events(key(KeyCode::Char(' ')), &mut app).unwrap();
    handle_key_events(key(KeyCode::Char('c')), &mut app).unwrap();
    app.confirm_pending();

    // While the destination listing streams, the cursor must land on the
    // copied folder as soon as its entry arrives — BEFORE the final sorted
    // pass.
    let mut revealed = false;
    for _ in 0..2000 {
        app.tick();
        let idx = app.panes[1].state.selected();
        if let Some(i) = idx {
            if app.panes[1]
                .files
                .get(i)
                .is_some_and(|f| f.label == "myfolder")
            {
                revealed = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(revealed, "cursor must land on the copied folder mid-stream");

    // And it stays selected after the final sorted pass.
    let mut settled = false;
    for _ in 0..2000 {
        app.tick();
        if app.file_list_settled(1) {
            settled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(settled);
    let idx = app.panes[1].state.selected().unwrap();
    assert_eq!(
        app.panes[1].files.get(idx).unwrap().label,
        "myfolder",
        "cursor stays on the copied folder after the sorted pass"
    );

    let _ = std::fs::remove_dir_all(&base);
}
