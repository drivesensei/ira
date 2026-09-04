use std::time::{Duration, Instant};

use ira::app::App;
use ira::domain::data::Folder;
use ira::services::list_files::FEntry;
use ira::services::transfer::JobStatus;

fn entry(path: &str) -> FEntry {
    FEntry {
        path: path.to_string(),
        label: std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string()),
        is_dir: false,
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

    app.copy_to_other_pane();
    assert_eq!(app.jobs.len(), 2);
    assert!(app.copy_board && app.board_has_focus());

    wait_for_jobs(&mut app);

    assert!(dst.join("a.txt").exists());
    assert!(dst.join("b.txt").exists());
    assert!(!dst.join("c.txt").exists());
    assert!(matches!(app.jobs[0].status, JobStatus::Done));
    assert!(matches!(app.jobs[1].status, JobStatus::Done));

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

    // Confirm path: y deletes the file.
    app.request_delete();
    assert_eq!(app.confirming.as_ref().unwrap().paths.len(), 1);
    app.confirm_delete();
    assert!(app.confirming.is_none());
    assert!(!victim.exists());

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
