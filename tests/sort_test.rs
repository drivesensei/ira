//! Integration tests for the `,` sort-cycle (Name → Size → Modified → Kind).
//!
//! Each test builds a sandbox folder on disk with known sizes and mtimes
//! (set explicitly with `File::set_modified`, no extra deps), lists it
//! through the real `App` listing path, then drives `App::cycle_sort` and
//! asserts on the resulting file order, cursor position, selection flags,
//! status notice, and filter view.

use ira::services::list_files::FEntry;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use ira::app::App;
use ira::domain::data::Folder;

/// A temp folder removed on drop, so failed asserts don't leave litter.
struct Sandbox {
    dir: PathBuf,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn sandbox(name: &str) -> Sandbox {
    let dir = std::env::temp_dir().join(format!("ira_sort_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Sandbox { dir }
}
fn find<'a>(app: &'a App, label: &str) -> &'a FEntry {
    app.panes[0]
        .files
        .iter()
        .find(|f| f.label == label)
        .unwrap_or_else(|| panic!("{label} must be listed"))
}

fn set_mtime(path: &Path, epoch_secs: i64) {
    let f = std::fs::File::open(path).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_secs(epoch_secs as u64))
        .unwrap();
}

/// Builds an `App` whose active pane lists the sandbox folder through the
/// real sync fast path (small folders list fully and synchronously).
fn app_for(dir: &Path) -> App {
    let mut app = App::default();
    app.panes[0].folder = Some(Folder::new(
        "sandbox".to_string(),
        dir.to_string_lossy().into_owned(),
        '1',
    ));
    app.list_files_from_selected_folder();
    assert!(!app.panes[0].files.is_empty(), "sandbox folder must list");
    app
}

fn labels(app: &App) -> Vec<&str> {
    app.panes[0]
        .files
        .iter()
        .map(|f| f.label.as_str())
        .collect()
}

/// Path of the file under the cursor, mapping a filtered/search cursor row
/// through the visible rows like the UI does.
fn cursor_path(app: &App) -> String {
    let pane = &app.panes[0];
    let file_index = if pane.filter_query.is_some() {
        pane.filter_indices[pane.state.selected().unwrap()]
    } else {
        pane.state.selected().unwrap()
    };
    pane.files[file_index].path.clone()
}

fn status_text(app: &App) -> &str {
    app.status
        .as_ref()
        .expect("a notice must be showing")
        .text
        .as_str()
}

/// Creates the shared cycle fixture: three files of distinct sizes/mtimes
/// plus one directory, all with explicitly set modification times.
fn cycle_fixture(dir: &Path) {
    std::fs::write(dir.join("a_small"), vec![0u8; 10]).unwrap();
    std::fs::write(dir.join("b_big"), vec![0u8; 5120]).unwrap();
    std::fs::write(dir.join("c_mid"), vec![0u8; 300]).unwrap();
    std::fs::create_dir_all(dir.join("zdir")).unwrap();
    set_mtime(&dir.join("a_small"), 1_000_000);
    set_mtime(&dir.join("b_big"), 2_000_000);
    set_mtime(&dir.join("c_mid"), 3_000_000);
    set_mtime(&dir.join("zdir"), 500_000);
}

#[test]
fn listing_captures_real_size_and_modified() {
    let sb = sandbox("meta");
    std::fs::write(sb.dir.join("small.txt"), vec![0u8; 10]).unwrap();
    std::fs::write(sb.dir.join("mid.txt"), vec![0u8; 300]).unwrap();
    std::fs::write(sb.dir.join("big.txt"), vec![0u8; 5120]).unwrap();
    set_mtime(&sb.dir.join("small.txt"), 1_000_000);
    set_mtime(&sb.dir.join("mid.txt"), 3_000_000);
    set_mtime(&sb.dir.join("big.txt"), 2_000_000);

    let app = app_for(&sb.dir);
    assert_eq!(find(&app, "small.txt").size, 10);
    assert_eq!(find(&app, "mid.txt").size, 300);
    assert_eq!(find(&app, "big.txt").size, 5120);
    assert_eq!(find(&app, "small.txt").modified, Some(1_000_000));
    assert_eq!(find(&app, "mid.txt").modified, Some(3_000_000));
    assert_eq!(find(&app, "big.txt").modified, Some(2_000_000));
}

#[test]
fn comma_cycles_sort_modes_and_reorders_files() {
    let sb = sandbox("cycle");
    cycle_fixture(&sb.dir);
    let mut app = app_for(&sb.dir);

    // Initial listing: name ascending (the listing's default).
    assert_eq!(app.panes[0].sort_mode, 0);
    assert_eq!(labels(&app), vec!["a_small", "b_big", "c_mid", "zdir"]);

    // 1st press: Size — files largest first, directory last.
    app.cycle_sort();
    assert_eq!(app.panes[0].sort_mode, 1);
    assert_eq!(labels(&app), vec!["b_big", "c_mid", "a_small", "zdir"]);
    assert_eq!(status_text(&app), "Sorted by size (largest first)");

    // 2nd press: Modified — newest first (zdir's 500k is oldest).
    app.cycle_sort();
    assert_eq!(app.panes[0].sort_mode, 2);
    assert_eq!(labels(&app), vec!["c_mid", "b_big", "a_small", "zdir"]);
    assert_eq!(status_text(&app), "Sorted by last modified (newest first)");

    // 3rd press: Kind — directories first, then files, both alphabetical.
    app.cycle_sort();
    assert_eq!(app.panes[0].sort_mode, 3);
    assert_eq!(labels(&app), vec!["zdir", "a_small", "b_big", "c_mid"]);
    assert_eq!(status_text(&app), "Sorted by kind");

    // 4th press: back to Name.
    app.cycle_sort();
    assert_eq!(app.panes[0].sort_mode, 0);
    assert_eq!(labels(&app), vec!["a_small", "b_big", "c_mid", "zdir"]);
    assert_eq!(status_text(&app), "Sorted by name");

    // 5th press: wraps around to Size again.
    app.cycle_sort();
    assert_eq!(app.panes[0].sort_mode, 1);
    assert_eq!(labels(&app), vec!["b_big", "c_mid", "a_small", "zdir"]);
}

#[test]
fn cursor_and_selection_follow_the_same_file_across_sorts() {
    let sb = sandbox("cursor");
    cycle_fixture(&sb.dir);
    let mut app = app_for(&sb.dir);

    // Put the cursor on b_big and mark it multi-selected.
    let big_path = app.panes[0]
        .files
        .iter()
        .find(|f| f.label == "b_big")
        .unwrap()
        .path
        .clone();
    let idx = app.panes[0]
        .files
        .iter()
        .position(|f| f.path == big_path)
        .unwrap();
    app.panes[0].state.select(Some(idx));
    app.panes[0].selected[idx] = true;

    // Through a full cycle, the cursor and the selection flag stay on b_big.
    for mode in [1, 2, 3, 0] {
        app.cycle_sort();
        assert_eq!(app.panes[0].sort_mode, mode);
        assert_eq!(cursor_path(&app), big_path, "cursor must follow b_big");
        let selected_labels: Vec<&str> = app.panes[0]
            .files
            .iter()
            .enumerate()
            .filter(|(i, _)| app.panes[0].selected[*i])
            .map(|(_, f)| f.label.as_str())
            .collect();
        assert_eq!(
            selected_labels,
            vec!["b_big"],
            "selection flag must travel with b_big"
        );
    }
}

#[test]
fn sort_cycle_recomputes_the_confirmed_filter_view() {
    let sb = sandbox("filter");
    std::fs::write(sb.dir.join("a.txt"), vec![0u8; 10]).unwrap();
    std::fs::write(sb.dir.join("b.txt"), vec![0u8; 5120]).unwrap();
    std::fs::write(sb.dir.join("c.txt"), vec![0u8; 300]).unwrap();
    std::fs::write(sb.dir.join("notes.md"), vec![0u8; 1]).unwrap();
    let mut app = app_for(&sb.dir);

    // Confirm a filter matching only the .txt files (any stored match order
    // is acceptable — cycle_sort recomputes it, so seed it with one).
    let txt_indices: Vec<usize> = app.panes[0]
        .files
        .iter()
        .enumerate()
        .filter(|(_, f)| f.label.ends_with(".txt"))
        .map(|(i, _)| i)
        .collect();
    app.panes[0].filter_query = Some("txt".to_string());
    app.panes[0].filter_indices = txt_indices.clone();
    app.panes[0].state.select(Some(0));

    // Cursor sits on a.txt (row 0 of the seeded filtered view).
    let a_path = app.panes[0]
        .files
        .iter()
        .find(|f| f.label == "a.txt")
        .unwrap()
        .path
        .clone();
    assert_eq!(cursor_path(&app), a_path);

    for mode in [1, 2, 3, 0] {
        app.cycle_sort();
        assert_eq!(app.panes[0].sort_mode, mode);

        // The filtered view still maps to exactly the .txt files.
        let visible_paths: Vec<&str> = app.panes[0]
            .filter_indices
            .iter()
            .map(|&i| app.panes[0].files[i].label.as_str())
            .collect();
        let mut sorted = visible_paths.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec!["a.txt", "b.txt", "c.txt"]);

        // And the cursor is still on a.txt at its new visible row.
        assert_eq!(cursor_path(&app), a_path, "mode {mode}");
    }
}

#[test]
fn unknown_modified_times_sort_last() {
    let sb = sandbox("nomtime");
    std::fs::write(sb.dir.join("known"), vec![0u8; 1]).unwrap();
    set_mtime(&sb.dir.join("known"), 2_000_000);

    // Build entries directly: one with a timestamp, one without.
    let mut app = App::default();
    let known = sb.dir.join("known").to_string_lossy().into_owned();
    let ghost = sb.dir.join("ghost").to_string_lossy().into_owned();
    app.panes[0].files = vec![
        ira::services::list_files::FEntry {
            path: ghost.clone(),
            label: "ghost".to_string(),
            is_dir: false,
            size: 1,
            modified: None,
        },
        ira::services::list_files::FEntry {
            path: known.clone(),
            label: "known".to_string(),
            is_dir: false,
            size: 1,
            modified: Some(2_000_000),
        },
    ];
    app.panes[0].selected = vec![false, false];

    app.cycle_sort(); // → size (equal sizes, label tiebreak)
    assert_eq!(labels(&app), vec!["ghost", "known"]);
    app.cycle_sort(); // → modified: known (newest) first, ghost (None) last
    assert_eq!(app.panes[0].sort_mode, 2);
    assert_eq!(labels(&app), vec!["known", "ghost"]);
}

#[test]
fn cycle_sort_on_an_empty_pane_is_a_safe_noop() {
    let mut app = App::default();
    app.cycle_sort();
    assert_eq!(app.panes[0].sort_mode, 1);
    assert_eq!(app.panes[0].state.selected(), None);
    assert_eq!(status_text(&app), "Sorted by size (largest first)");
}
