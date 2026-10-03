use ira_core::{cursor::CursorState, services::state, theme::ThemePreset};
#[test]
fn cursor_clear_resets_offset_but_selection_keeps_it() {
    let mut cursor = CursorState::default();
    *cursor.offset_mut() = 7;
    cursor.select(Some(4));
    assert_eq!((cursor.selected(), cursor.offset()), (Some(4), 7));
    cursor.select(None);
    assert_eq!((cursor.selected(), cursor.offset()), (None, 0));
}
#[test]
fn default_session_and_preset_match_legacy_contract() {
    assert_eq!(state::SessionState::default().preview, [3, 3]);
    assert_eq!(
        ThemePreset::parse(" TOKYO_NIGHT ").unwrap().id(),
        "tokyo-night"
    );
}

pub use ira_core::{domain, services, theme};
#[allow(dead_code)]
#[path = "oracle/list_files.rs"]
mod oracle_listing;
#[allow(dead_code)]
#[path = "oracle/state.rs"]
mod oracle_state;
#[allow(dead_code)]
#[path = "oracle/transfer.rs"]
mod oracle_transfer;

fn fixture(name: &str) -> std::path::PathBuf {
    static ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "ira-core-characterize-{}-{name}-{}",
        std::process::id(),
        ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}
#[test]
fn session_codec_matches_frozen_source_bytes_and_aliases() {
    let root = fixture("state");
    let input = root.join("in");
    std::fs::write(&input, "split=1\nactive=1\nhidden=1\npreview0=0\npreview1=2\nleft=/a = b\nright=/c\ntheme= TOKYO_NIGHT \nsize=7\t2\t4096\t1\t99\t/a = b\nunknown=x\n").unwrap();
    let old = oracle_state::load_state_from(&input);
    let new = state::load_state_from(&input);
    oracle_state::save_state_to(&root.join("old"), &old);
    state::save_state_to(&root.join("new"), &new);
    assert_eq!(
        std::fs::read(root.join("old")).unwrap(),
        std::fs::read(root.join("new")).unwrap()
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn bookmark_legacy_lines_tabs_and_bytes_are_preserved() {
    use ira_core::services::bookmarks::*;
    let root = fixture("bookmarks");
    let path = root.join("bookmarks");
    std::fs::write(&path, "\n/legacy/path\n name \t/a = b\tmore\n").unwrap();
    assert_eq!(
        read_bookmarks_from(&path),
        vec![
            ("path".into(), "/legacy/path".into()),
            (" name ".into(), "/a = b\tmore".into())
        ]
    );
    write_bookmarks_to(
        &path,
        &[domain::data::Folder::new(
            " name ".into(),
            "/a = b".into(),
            'o',
        )],
    );
    assert_eq!(std::fs::read(&path).unwrap(), b" name \t/a = b\n");
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn listing_metadata_hidden_flags_and_chunking_match_source() {
    let root = fixture("listing");
    std::fs::create_dir(root.join("dir")).unwrap();
    std::fs::write(root.join("unicode-é"), b"hello").unwrap();
    std::fs::write(root.join(".hidden"), b"x").unwrap();
    let project = |entries: Vec<ira_core::services::list_files::FEntry>| {
        let mut rows: Vec<_> = entries
            .into_iter()
            .map(|e| (e.label, e.is_dir, e.size, e.modified))
            .collect();
        rows.sort();
        rows
    };
    let mut old: Vec<_> = oracle_listing::list_files(root.to_str().unwrap())
        .unwrap()
        .into_iter()
        .map(|e| (e.label, e.is_dir, e.size, e.modified))
        .collect();
    old.sort();
    assert_eq!(
        old,
        project(services::list_files::list_files(root.to_str().unwrap()).unwrap())
    );
    for hidden in [false, true] {
        let (old, oc) =
            oracle_listing::list_files_bounded(root.to_str().unwrap(), 8, hidden).unwrap();
        let (new, nc) =
            services::list_files::list_files_bounded(root.to_str().unwrap(), 8, hidden).unwrap();
        assert_eq!(oc, nc);
        assert_eq!(old.len(), new.len());
    }
    let mut batches = Vec::new();
    services::list_files::list_files_chunked(root.to_str().unwrap(), 2, &mut |b| {
        batches.push(b.len())
    })
    .unwrap();
    assert_eq!(batches, vec![2, 1]);
    std::fs::remove_dir_all(root).unwrap();
}
fn transfer_fixture(oracle: bool, overwrite: bool) -> (bool, bool, bool, bool) {
    let root = fixture("transfer");
    let source = root.join("src");
    let target = root.join("dst");
    std::fs::create_dir_all(source.join("folder")).unwrap();
    std::fs::create_dir_all(target.join("folder")).unwrap();
    std::fs::write(source.join("folder/new"), b"new").unwrap();
    std::fs::write(target.join("folder/existing"), b"old").unwrap();
    let result = if oracle {
        use oracle_transfer::*;
        let (tx, rx) = std::sync::mpsc::channel();
        let job = Job {
            id: 1,
            kind: JobKind::Copy,
            overwrite: if overwrite {
                OverwritePolicy::Overwrite
            } else {
                OverwritePolicy::AutoRename
            },
            paths: vec![source.join("folder").to_str().unwrap().into()],
            dest_dir: target.to_str().unwrap().into(),
            label: String::new(),
            total_bytes: None,
            copied_bytes: 0,
            current: String::new(),
            status: JobStatus::Queued,
            started_at: std::time::Instant::now(),
            control: JobControl::new(),
        };
        spawn_job(&job, tx);
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap() {
                JobEvent::Done { .. } => break true,
                JobEvent::Failed { .. } => break false,
                _ => {}
            }
        }
    } else {
        use services::transfer::*;
        let (tx, rx) = std::sync::mpsc::channel();
        let job = Job {
            id: 1,
            kind: JobKind::Copy,
            overwrite: if overwrite {
                OverwritePolicy::Overwrite
            } else {
                OverwritePolicy::AutoRename
            },
            paths: vec![source.join("folder").to_str().unwrap().into()],
            dest_dir: target.to_str().unwrap().into(),
            label: String::new(),
            total_bytes: None,
            copied_bytes: 0,
            current: String::new(),
            status: JobStatus::Queued,
            started_at: std::time::Instant::now(),
            control: JobControl::new(),
        };
        spawn_job(&job, tx);
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap() {
                JobEvent::Done { .. } => break true,
                JobEvent::Failed { .. } => break false,
                _ => {}
            }
        }
    };
    let outcome = (
        result,
        target.join("folder/existing").exists(),
        target.join("folder (2)/new").exists(),
        source.join("folder/new").exists(),
    );
    std::fs::remove_dir_all(root).unwrap();
    outcome
}
#[test]
fn collision_auto_rename_matches_source_and_preserves_existing() {
    assert_eq!(transfer_fixture(true, false), (true, true, true, true));
    assert_eq!(transfer_fixture(false, false), (true, true, true, true));
}
#[test]
fn characterize_g0016_overwrite_directory_failure_deletes_existing_destination() {
    // Frozen oracle retains the defect; D-003 deliberately fixes both live applications.
    assert_eq!(transfer_fixture(true, true), (false, false, false, true));
    assert_eq!(transfer_fixture(false, true), (false, true, false, true));
}
#[test]
fn core_source_and_manifest_have_no_ui_dependencies_or_unsafe() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    fn visit(dir: &std::path::Path) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                visit(&p)
            } else if p.extension().is_some_and(|x| x == "rs") {
                let t = std::fs::read_to_string(&p).unwrap();
                for banned in ["ratatui", "crossterm", "gpui::", "unsafe {"] {
                    assert!(!t.contains(banned), "{} imports {banned}", p.display());
                }
            }
        }
    }
    visit(&src);
    for banned in ["ratatui", "crossterm", "gpui"] {
        assert!(!include_str!("../Cargo.toml").contains(banned));
    }
}
