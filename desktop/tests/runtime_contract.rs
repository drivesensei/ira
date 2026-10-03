use ira_core::{
    application::App,
    domain::data::Folder,
    input::{Input, KeyCode, KeyEvent, KeyModifiers},
    model::EntryTarget,
    services::list_files::FEntry,
};
use ira_desktop::runtime::{Command, Completion, Envelope, Latest, Runtime, TargetVerb, apply};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "ira-runtime-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn app(&self) -> App {
        let mut app = App::default();
        app.state_path = Some(self.0.join("state"));
        app.window_generation = 7;
        app.panes[0].folder = Some(Folder::new(
            "Fixture".into(),
            self.0.to_string_lossy().into_owned(),
            '#',
        ));
        for name in ["alpha.txt", "beta.txt"] {
            let p = self.0.join(name);
            std::fs::write(&p, b"fixture").unwrap();
            app.panes[0].files.push(FEntry {
                path: p.to_string_lossy().into_owned(),
                label: name.into(),
                is_dir: false,
                size: 7,
                modified: None,
            });
        }
        app.panes[0].selected = vec![false, false];
        app.panes[0].state.select(Some(0));
        app.panes[0].listing_generation = 3;
        app.panes[0].listing_settled = true;
        app
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn key(code: KeyCode) -> Command {
    Command::Input(Input::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}
fn envelope(sequence: u64, command: Command) -> Envelope {
    Envelope {
        sequence,
        window_generation: 7,
        input_generation: None,
        command,
    }
}
#[test]
fn rapid_rename_text_confirm_is_ordered_without_a_repaint() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    apply(&mut app, envelope(1, key(KeyCode::Enter))).unwrap();
    apply(
        &mut app,
        envelope(
            2,
            Command::Draft {
                text: "renamed_文.txt".into(),
                cursor: 13,
            },
        ),
    )
    .unwrap();
    apply(&mut app, envelope(3, key(KeyCode::Enter))).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !fixture.0.join("renamed_文.txt").exists() && Instant::now() < deadline {
        app.tick();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(fixture.0.join("renamed_文.txt").exists());
    assert!(!fixture.0.join("alpha.txt").exists());
    assert!(app.renaming.is_none());
    assert_eq!(app.ack_sequence, 3);
}
#[test]
fn stale_mouse_generation_cannot_select_a_different_file() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    let target = EntryTarget {
        pane: 0,
        path: fixture.0.join("beta.txt"),
        listing_generation: 2,
    };
    assert!(
        apply(
            &mut app,
            envelope(
                1,
                Command::Target {
                    target,
                    verb: TargetVerb::Open
                }
            )
        )
        .is_err()
    );
    assert_eq!(app.panes[0].state.selected(), Some(0));
    assert_eq!(app.ack_sequence, 0);
}
#[test]
fn current_target_resolves_by_path_after_sort() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.panes[0].files.reverse();
    let target = EntryTarget {
        pane: 0,
        path: fixture.0.join("beta.txt"),
        listing_generation: 3,
    };
    apply(
        &mut app,
        envelope(
            1,
            Command::Target {
                target,
                verb: TargetVerb::Focus,
            },
        ),
    )
    .unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(0));
}
#[test]
fn mouse_does_not_bypass_confirmation_priority() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.request_delete();
    let target = EntryTarget {
        pane: 0,
        path: fixture.0.join("beta.txt"),
        listing_generation: 3,
    };
    assert!(
        apply(
            &mut app,
            envelope(
                1,
                Command::Target {
                    target,
                    verb: TargetVerb::Focus
                }
            )
        )
        .is_err()
    );
    assert!(app.confirming.is_some());
    assert_eq!(app.panes[0].state.selected(), Some(0));
}
#[test]
fn reopened_input_rejects_old_draft_generation() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.start_search();
    apply(&mut app, envelope(1, Command::SetFocus(2))).unwrap();
    let mut old = envelope(
        2,
        Command::Draft {
            text: "old".into(),
            cursor: 3,
        },
    );
    old.input_generation = Some((0, 1));
    assert!(apply(&mut app, old).is_err());
    assert_eq!(app.search_query.as_deref(), Some(""));
}
#[test]
fn old_window_commands_do_not_mutate_reopened_session() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.window_generation = 8;
    assert!(apply(&mut app, envelope(1, key(KeyCode::Down))).is_err());
    assert_eq!(app.panes[0].state.selected(), Some(0));
}
#[test]
fn latest_snapshot_coalesces_only_snapshot_values() {
    let latest = Latest::default();
    latest.publish(1);
    latest.publish(2);
    assert_eq!(latest.try_take(), Some(2));
    assert_eq!(latest.try_take(), None);
}
#[test]
fn stop_bypasses_blocked_startup_and_backpressure_never_drops_commands() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let (gate_tx, gate_rx) = mpsc::channel();
    let gate = Arc::new(Mutex::new(gate_rx));
    let mut runtime = Runtime::with_factory(7, move || {
        gate.lock().unwrap().recv().unwrap();
        app
    });
    for _ in 0..300 {
        runtime.enqueue(key(KeyCode::Down), None);
    }
    assert_eq!(runtime.backlog(), 44);
    let started = Instant::now();
    runtime.stop(&[]);
    let stop_latency = started.elapsed();
    assert!(runtime.try_snapshot().is_none());
    gate_tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut closed = false;
    while Instant::now() < deadline {
        if let Some(Completion::Closed) = runtime.try_completion() {
            closed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(closed);
    eprintln!("out-of-band stop returned in {stop_latency:?}; startup was still blocked");
    assert_eq!(runtime.backlog(), 44);
}

#[test]
fn old_sequence_cannot_regress_acknowledgement() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    apply(&mut app, envelope(20, key(KeyCode::Down))).unwrap();
    assert!(apply(&mut app, envelope(10, key(KeyCode::Up))).is_err());
    assert_eq!(app.ack_sequence, 20);
    assert_eq!(app.panes[0].state.selected(), Some(1));
}

#[test]
fn editor_worker_preserves_save_order_and_rejects_closed_drafts() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.open_edit();
    let mut runtime = Runtime::with_factory(7, move || app);
    runtime.enqueue(Command::Input(Input::Tick), None);
    let deadline = Instant::now() + Duration::from_secs(3);
    let snapshot = loop {
        if let Some(publication) = runtime.try_snapshot()
            && publication.snapshot.edit.is_some()
        {
            break publication.snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "editor open completion was not published"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    let id = snapshot.edit.as_ref().unwrap().document_id;
    let generation = Some((snapshot.document_generation, snapshot.focus_generation));
    runtime.enqueue(
        Command::SaveDraft {
            document_id: id,
            revision: 1,
            text: "first\n".into(),
        },
        generation,
    );
    runtime.enqueue(
        Command::SaveDraft {
            document_id: id,
            revision: 2,
            text: "second 文\n".into(),
        },
        generation,
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(publication) = runtime.try_snapshot()
            && publication
                .snapshot
                .edit
                .as_ref()
                .is_some_and(|e| e.edit_revision == 2 && !e.dirty)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "ordered editor save completion was not published"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("alpha.txt")).unwrap(),
        "second 文\n"
    );
    let mut reopened = runtime.attach(8);
    reopened.enqueue(
        Command::EditorDraft {
            document_id: id,
            revision: 3,
            text: "stale".into(),
        },
        generation,
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(Completion::Rejected { .. }) = reopened.try_completion() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "old editor callback was not rejected"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    reopened.stop(&[]);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !matches!(reopened.try_completion(), Some(Completion::Closed)) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn native_reveal_selects_files_and_directories_with_source_arguments() {
    for dir in [false, true] {
        assert_eq!(
            ira_desktop::platform::file_manager_candidates("/tmp/文, path", dir),
            vec![("open".into(), vec!["-R".into(), "/tmp/文, path".into()])]
        );
    }
    assert_eq!(
        ira_desktop::platform::terminal_candidates("/tmp/path")
            .last()
            .unwrap(),
        &(
            "open".into(),
            vec!["-a".into(), "Terminal".into(), "/tmp/path".into()]
        )
    );
}
