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
        app.bookmarks_path = Some(self.0.join("bookmarks"));
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

#[test]
fn wheel_uses_current_pane_cursor_and_cannot_bypass_modal_priority() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    apply(
        &mut app,
        envelope(
            1,
            Command::Wheel {
                pane: 0,
                listing_generation: 3,
                next: true,
            },
        ),
    )
    .unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(1));
    assert!(
        apply(
            &mut app,
            envelope(
                2,
                Command::Wheel {
                    pane: 0,
                    listing_generation: 2,
                    next: false
                }
            )
        )
        .is_err()
    );
    assert_eq!(app.panes[0].state.selected(), Some(1));
    apply(&mut app, envelope(3, key(KeyCode::Enter))).unwrap();
    assert!(
        apply(
            &mut app,
            envelope(
                4,
                Command::Wheel {
                    pane: 0,
                    listing_generation: 3,
                    next: false
                }
            )
        )
        .is_err()
    );
    assert_eq!(app.panes[0].state.selected(), Some(1));
}

#[test]
fn job_pointer_pause_resolves_stable_id_after_job_reorder() {
    use ira_core::services::transfer::{Job, JobControl, JobKind, JobStatus, OverwritePolicy};
    use ira_desktop::runtime::JobVerb;
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.copy_board = true;
    for id in [40, 30] {
        app.jobs.push(Job {
            id,
            kind: JobKind::Copy,
            overwrite: OverwritePolicy::SkipExisting,
            paths: vec![],
            dest_dir: fixture.0.to_string_lossy().into_owned(),
            label: format!("job{id}"),
            total_bytes: None,
            copied_bytes: 0,
            current: String::new(),
            status: JobStatus::Running,
            started_at: Instant::now(),
            control: JobControl::new(),
        });
    }
    apply(
        &mut app,
        envelope(
            1,
            Command::Job {
                id: 30,
                verb: JobVerb::Pause,
            },
        ),
    )
    .unwrap();
    assert!(app.board_focused);
    assert_eq!(app.copy_board_state.selected(), Some(1));
    assert!(matches!(app.jobs[1].status, JobStatus::Paused));
    assert!(matches!(app.jobs[0].status, JobStatus::Running));
    app.jobs.swap(0, 1);
    apply(
        &mut app,
        envelope(
            2,
            Command::Job {
                id: 30,
                verb: JobVerb::Pause,
            },
        ),
    )
    .unwrap();
    assert_eq!(app.copy_board_state.selected(), Some(0));
    assert!(matches!(app.jobs[0].status, JobStatus::Running));
    assert!(
        apply(
            &mut app,
            envelope(
                3,
                Command::Job {
                    id: 999,
                    verb: JobVerb::Pause
                }
            )
        )
        .is_err()
    );
}

#[test]
fn native_color_preserves_rgb_named_indexed_and_reset_resolution() {
    use ira_core::theme::Color;
    for (source, expected) in [
        (Color::Rgb(17, 34, 51), 0x112233ff),
        (Color::Red, 0x800000ff),
        (Color::Indexed(196), 0xff0000ff),
        (Color::Reset, 0x1e1e2eff),
    ] {
        assert_eq!(
            ira_desktop::views::native_color(source),
            gpui::rgba(expected)
        );
    }
}

#[test]
fn geometry_round_trip_is_separate_from_session_state_and_ordered_on_reopen() {
    use ira_desktop::platform::geometry::{Geometry, Writer};
    let fixture = Fixture::new();
    let path = fixture.0.join("desktop-window");
    let writer = Writer::new(Some(path.clone()));
    assert_eq!(
        writer.load().recv_timeout(Duration::from_secs(2)).unwrap(),
        None
    );
    let first = Geometry {
        x: 20.,
        y: 30.,
        width: 960.,
        height: 640.,
        mode: 0,
    };
    let last = Geometry {
        x: 40.,
        y: 50.,
        width: 1080.,
        height: 720.,
        mode: 1,
    };
    writer.save(first);
    writer.save(last);
    assert_eq!(
        writer.load().recv_timeout(Duration::from_secs(2)).unwrap(),
        Some(last)
    );
    writer
        .barrier()
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        Geometry::parse(&std::fs::read_to_string(path).unwrap()),
        Some(last)
    );
    assert!(!fixture.0.join("state").exists());
    assert!(writer.try_error().is_none());
}

#[test]
fn geometry_rejects_corrupt_nonfinite_and_unbounded_values() {
    use ira_desktop::platform::geometry::Geometry;
    for text in [
        "",
        "ira-window-v1 0 0 NaN 720 0",
        "ira-window-v1 inf 0 1080 720 0",
        "ira-window-v1 0 0 100000 720 0",
        "ira-window-v1 0 0 1080 720 3",
        "ira-window-v0 0 0 1080 720 0",
    ] {
        assert!(Geometry::parse(text).is_none(), "{text}");
    }
}

#[test]
fn geometry_removed_monitor_restores_visible_fallback_and_window_mode() {
    use ira_desktop::platform::geometry::Geometry;
    let display = gpui::Bounds {
        origin: gpui::point(gpui::px(0.), gpui::px(0.)),
        size: gpui::size(gpui::px(1440.), gpui::px(900.)),
    };
    let fallback = gpui::Bounds {
        origin: gpui::point(gpui::px(180.), gpui::px(90.)),
        size: gpui::size(gpui::px(1080.), gpui::px(720.)),
    };
    let restored = Geometry {
        x: 3000.,
        y: 2000.,
        width: 1080.,
        height: 720.,
        mode: 2,
    }
    .restore(&[display], fallback);
    assert!(matches!(restored, gpui::WindowBounds::Fullscreen(_)));
    assert_eq!(restored.get_bounds(), fallback);
}

#[test]
fn queued_native_copy_is_rejected_observably_after_window_replacement() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    app.copy_folder_path();
    let mut old = Runtime::with_factory(7, move || app);
    old.enqueue(Command::Input(Input::Tick), None);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if old
            .try_snapshot()
            .is_some_and(|publication| publication.snapshot.ack_sequence >= 1)
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    old.detach();
    let reopened = old.attach(8);
    let guard = reopened.effect_guard(7);
    assert!(!guard.is_current());
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match reopened.try_completion() {
            Some(Completion::Rejected {
                window_generation: 7,
                reason,
                ..
            }) => {
                assert!(reason.contains("replaced window"));
                break;
            }
            Some(Completion::Host { .. }) => panic!("Old window native effect escaped the guard"),
            _ => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    reopened.stop(&[]);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !matches!(reopened.try_completion(), Some(Completion::Closed)) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn recurring_drive_worker_does_not_block_input_and_rejects_old_window_results() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let folder_path = fixture.0.to_string_lossy().into_owned();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (gate_tx, gate_rx) = mpsc::channel();
    let mut first = true;
    let mut runtime = Runtime::with_drive_probe(
        7,
        move || app,
        move || {
            if first {
                first = false;
                entered_tx.send(()).unwrap();
                gate_rx.recv().unwrap();
                Ok(vec![Folder::new(
                    "old-mounted".into(),
                    folder_path.clone(),
                    '1',
                )])
            } else {
                Ok(vec![Folder::new(
                    "new-mounted".into(),
                    folder_path.clone(),
                    '1',
                )])
            }
        },
    );
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    runtime.enqueue(key(KeyCode::Down), None);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if runtime.try_snapshot().is_some_and(|publication| {
            publication.snapshot.ack_sequence >= 1
                && publication.snapshot.panes[0].cursor == Some(1)
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "input blocked behind drive enumeration"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let reopened = runtime.attach(8);
    gate_tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(publication) = reopened.try_snapshot() {
            assert!(
                publication
                    .snapshot
                    .drives
                    .as_ref()
                    .is_none_or(|drives| drives.iter().all(|drive| drive.label != "old-mounted")),
                "Old enumeration mutated the reopened window"
            );
            if publication.snapshot.window_generation == 8
                && publication.snapshot.drives.as_ref().is_some_and(|drives| {
                    drives
                        .first()
                        .is_some_and(|drive| drive.label == "new-mounted")
                })
            {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "recurring fresh drive scan was not published"
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

#[test]
fn detached_view_cannot_consume_new_window_mailboxes() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let old = Runtime::with_factory(7, move || app);
    let mut reopened = old.attach(8);
    assert!(old.try_snapshot().is_none());
    assert!(old.try_completion().is_none());
    reopened.enqueue(Command::Input(Input::Tick), None);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        assert!(old.try_snapshot().is_none());
        assert!(old.try_completion().is_none());
        if reopened.try_snapshot().is_some_and(|publication| {
            publication.snapshot.window_generation == 8 && publication.snapshot.ack_sequence >= 1
        }) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    reopened.stop(&[]);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !matches!(reopened.try_completion(), Some(Completion::Closed)) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn first_reopened_command_refreshes_epoch_after_blocked_receive() {
    for _ in 0..10 {
        let fixture = Fixture::new();
        let app = fixture.app();
        let old = Runtime::with_factory(7, move || app);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if old.try_snapshot().is_some() {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        std::thread::sleep(Duration::from_millis(5));
        let mut reopened = old.attach(8);
        let sequence = reopened.enqueue(Command::SetFocus(99), None);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(Completion::Rejected {
                sequence: rejected,
                window_generation: 8,
                reason,
            }) = reopened.try_completion()
            {
                assert_ne!(
                    rejected, sequence,
                    "First current-window command was rejected: {reason}"
                );
            }
            if reopened.try_snapshot().is_some_and(|publication| {
                publication.snapshot.window_generation == 8
                    && publication.snapshot.focus_generation == 99
                    && publication.snapshot.ack_sequence >= sequence
            }) {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        reopened.stop(&[]);
        let deadline = Instant::now() + Duration::from_secs(3);
        while !matches!(reopened.try_completion(), Some(Completion::Closed)) {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn ax_entry(app: &App, action: ira_desktop::platform::accessibility::model::Action) -> Command {
    use ira_desktop::platform::accessibility::{
        ResolvedAction,
        model::{NodeId, Stamp, Target},
    };
    Command::Accessibility(ResolvedAction {
        node: NodeId {
            window: 7,
            serial: 1,
        },
        stamp: Stamp {
            window: 7,
            document: app.document_generation,
            focus: app.focus_generation,
            revision: app.revision,
            text_revision: 0,
        },
        target: Target::Entry {
            pane: 0,
            path: PathBuf::from(&app.panes[0].files[0].path),
            listing_generation: app.panes[0].listing_generation,
        },
        action,
        prepared_key: None,
    })
}
#[test]
fn accessibility_selected_is_idempotent_exclusive_and_generation_checked() {
    use ira_desktop::platform::accessibility::model::Action;
    let fixture = Fixture::new();
    let mut app = fixture.app();
    let old = app.snapshot();
    let first = ax_entry(&app, Action::SetSelected(true));
    apply(&mut app, envelope(1, first)).unwrap();
    assert_eq!(app.panes[0].selected, vec![true, false]);
    assert_eq!(app.panes[0].state.selected(), Some(0));
    let new = app.snapshot();
    assert!(!old.panes[0].rows[0].selected);
    assert!(new.panes[0].rows[0].selected);
    assert!(!Arc::ptr_eq(&old.panes[0].rows, &new.panes[0].rows));
    let second = ax_entry(&app, Action::SetSelected(true));
    apply(&mut app, envelope(2, second)).unwrap();
    assert_eq!(app.panes[0].selected, vec![true, false]);
    app.panes[0].selected[1] = true;
    let only = ax_entry(&app, Action::SelectOnly);
    apply(&mut app, envelope(3, only)).unwrap();
    assert_eq!(app.panes[0].selected, vec![true, false]);
    let stale = ax_entry(&app, Action::SetSelected(false));
    app.revision += 1;
    assert!(apply(&mut app, envelope(4, stale)).is_err());
    assert!(app.panes[0].selected[0]);
}

#[test]
fn background_semantic_publications_advance_revision_without_input() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let path = fixture.0.to_string_lossy().into_owned();
    let mut generation = 0;
    let runtime = Runtime::with_drive_probe(
        7,
        move || app,
        move || {
            generation += 1;
            Ok(vec![Folder::new(
                format!("drive-{generation}"),
                path.clone(),
                '1',
            )])
        },
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut first = None;
    loop {
        if let Some(publication) = runtime.try_snapshot()
            && let Some(drives) = &publication.snapshot.drives
            && let Some(drive) = drives.first()
        {
            if let Some((label, revision)) = &first {
                if label != &drive.label {
                    runtime.stop(&[]);
                    assert!(
                        publication.snapshot.revision > *revision,
                        "background drive semantics changed without a fresh publication revision"
                    );
                    assert_eq!(
                        publication.snapshot.ack_sequence, 0,
                        "no input command was needed"
                    );
                    break;
                }
            } else {
                first = Some((drive.label.clone(), publication.snapshot.revision));
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn queued_accessibility_entry_is_rejected_after_same_path_relisting() {
    use ira_desktop::platform::accessibility::{
        AccessibilityIntent, ActionSink,
        model::{AccessibilityModel, Action, LayoutSnapshot, Role, Target},
    };
    let fixture = Fixture::new();
    let app = fixture.app();
    let mut runtime = Runtime::with_factory(7, move || app);
    let deadline = Instant::now() + Duration::from_secs(4);
    let first = loop {
        if let Some(publication) = runtime.try_snapshot() {
            break publication.snapshot;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    };
    let mut model = AccessibilityModel::default();
    let tree = Arc::new(model.project(&first, None, &LayoutSnapshot::default()));
    let node = tree
        .nodes
        .values()
        .find(|n| {
            n.role == Role::Row
                && matches!(&n.target, Target::Entry { path, .. } if path.ends_with("alpha.txt"))
        })
        .unwrap();
    let id = node.id;
    let (sink, receiver) = ActionSink::channel(tree.clone(), 4);
    sink.try_dispatch(AccessibilityIntent {
        node: id,
        stamp: tree.stamp,
        action: Action::SetSelected(true),
    })
    .unwrap();
    let queued = receiver.try_next().unwrap().unwrap();
    let token = first.panes[0].listing_generation;
    runtime.enqueue(key(KeyCode::Char('.')), None);
    let next = loop {
        if let Some(publication) = runtime.try_snapshot()
            && publication.snapshot.panes[0].listing_generation > token
            && publication.snapshot.panes[0].listing_settled
        {
            break publication.snapshot;
        }
        assert!(Instant::now() < deadline, "relisting failed to settle");
        std::thread::sleep(Duration::from_millis(1));
    };
    let fresh_tree = Arc::new(model.project(&next, None, &LayoutSnapshot::default()));
    let fresh = &fresh_tree.nodes[&id];
    assert!(
        matches!(&fresh.target, Target::Entry { path, listing_generation, .. } if path.ends_with("alpha.txt") && *listing_generation > token)
    );
    runtime.enqueue(Command::Accessibility(queued), None);
    loop {
        if let Some(Completion::Rejected { reason, .. }) = runtime.try_completion() {
            assert!(reason.contains("older semantic frame"));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    loop {
        if let Some(publication) = runtime.try_snapshot()
            && publication.snapshot.ack_sequence == 1
        {
            assert!(
                publication.snapshot.panes[0]
                    .rows
                    .iter()
                    .all(|row| !row.selected),
                "rejected old intent mutated selection"
            );
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    sink.publish(fresh_tree.clone()).unwrap();
    sink.try_dispatch(AccessibilityIntent {
        node: id,
        stamp: fresh_tree.stamp,
        action: Action::SetSelected(true),
    })
    .unwrap();
    runtime.enqueue(
        Command::Accessibility(receiver.try_next().unwrap().unwrap()),
        None,
    );
    loop {
        if let Some(publication) = runtime.try_snapshot()
            && publication.snapshot.ack_sequence >= 3
        {
            assert!(
                publication.snapshot.panes[0]
                    .rows
                    .iter()
                    .find(|r| r.entry.label == "alpha.txt")
                    .unwrap()
                    .selected
            );
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    runtime.stop(&[]);
}

#[test]
fn no_input_relisting_rejects_queued_full_stamp_on_same_entry_node() {
    use ira_desktop::platform::accessibility::{
        AccessibilityIntent, ActionSink,
        model::{AccessibilityModel, Action, LayoutSnapshot, Role, Target},
    };
    let fixture = Fixture::new();
    let mut app = fixture.app();
    let old = app.snapshot();
    let mut model = AccessibilityModel::default();
    let old_tree = Arc::new(model.project(&old, None, &LayoutSnapshot::default()));
    let node = old_tree
        .nodes
        .values()
        .find(|n| {
            n.role == Role::Row
                && matches!(&n.target, Target::Entry { path, .. } if path.ends_with("alpha.txt"))
        })
        .unwrap();
    let id = node.id;
    let (sink, actions) = ActionSink::channel(old_tree.clone(), 4);
    sink.try_dispatch(AccessibilityIntent {
        node: id,
        stamp: old_tree.stamp,
        action: Action::SetSelected(true),
    })
    .unwrap();
    let queued = actions.try_next().unwrap().unwrap();
    app.list_files_from_selected_folder();
    let mut runtime = Runtime::with_factory(7, move || app);
    let deadline = Instant::now() + Duration::from_secs(4);
    let current = loop {
        if let Some(p) = runtime.try_snapshot()
            && p.snapshot.panes[0].listing_settled
        {
            assert_eq!(
                p.snapshot.ack_sequence, 0,
                "background listing must settle without input"
            );
            assert!(p.snapshot.revision > old.revision);
            break p.snapshot;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    let tree = Arc::new(model.project(&current, None, &LayoutSnapshot::default()));
    assert!(
        matches!(&tree.nodes[&id].target, Target::Entry { path, listing_generation, .. } if path.ends_with("alpha.txt") && *listing_generation > old.panes[0].listing_generation)
    );
    runtime.enqueue(Command::Accessibility(queued), None);
    loop {
        if let Some(Completion::Rejected { reason, .. }) = runtime.try_completion() {
            assert!(reason.contains("older semantic frame"));
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    loop {
        if let Some(p) = runtime.try_snapshot() {
            assert!(p.snapshot.panes[0].rows.iter().all(|r| !r.selected));
            assert_eq!(p.snapshot.ack_sequence, 0);
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    sink.publish(tree.clone()).unwrap();
    sink.try_dispatch(AccessibilityIntent {
        node: id,
        stamp: tree.stamp,
        action: Action::SetSelected(true),
    })
    .unwrap();
    runtime.enqueue(
        Command::Accessibility(actions.try_next().unwrap().unwrap()),
        None,
    );
    loop {
        if let Some(p) = runtime.try_snapshot()
            && p.snapshot.ack_sequence >= 2
        {
            assert!(
                p.snapshot.panes[0]
                    .rows
                    .iter()
                    .find(|r| r.entry.label == "alpha.txt")
                    .unwrap()
                    .selected
            );
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    runtime.stop(&[]);
}

#[test]
fn unicode_search_draft_and_escape_preserve_running_window() {
    let fixture = Fixture::new();
    let mut app = fixture.app();
    for (sequence, command) in [
        key(KeyCode::Down),
        key(KeyCode::Char(' ')),
        key(KeyCode::Char('/')),
        Command::Draft {
            text: "文😀e\u{301}".into(),
            cursor: 4,
        },
    ]
    .into_iter()
    .enumerate()
    {
        apply(&mut app, envelope(sequence as u64 + 1, command)).unwrap();
        app.tick();
        assert!(app.running, "search path cannot request shutdown");
        assert_eq!(app.window_generation, 7);
    }
    assert_eq!(app.search_query.as_deref(), Some("文😀e\u{301}"));
    apply(&mut app, envelope(5, key(KeyCode::Esc))).unwrap();
    app.tick();
    assert!(app.running);
    assert_eq!(app.window_generation, 7);
    assert!(app.search_query.is_none());
}

#[test]
fn tab_preserves_neutral_editor_ticket_and_pending_escape_tab_help_cancel() {
    use ira_core::{
        input::Command as CoreCommand,
        model::{HostRequest, PreviewMode},
    };
    let fixture = Fixture::new();
    for cancellation in [
        None,
        Some(key(KeyCode::Esc)),
        Some(key(KeyCode::Tab)),
        Some(Command::Input(Input::Action(CoreCommand::ShowHelp))),
    ] {
        let mut app = fixture.app();
        app.panes[0].preview_mode = PreviewMode::Column;
        apply(&mut app, envelope(1, key(KeyCode::Tab))).unwrap();
        let request = app
            .take_host_requests()
            .into_iter()
            .find_map(|request| match request {
                HostRequest::OpenEditor(request) => Some(request),
                _ => None,
            })
            .expect("Tab must request text editor");
        assert_eq!(
            request.focus_generation, app.focus_generation,
            "wrapper must preserve focus ticket stamped by neutral dispatch"
        );
        let document =
            ira_core::editor::open_document(request.document_id, &request.target.path).unwrap();
        if let Some(cancellation) = cancellation {
            apply(&mut app, envelope(2, cancellation)).unwrap();
            assert!(
                !app.apply_open_editor(request, Ok(document)),
                "cancelled editor cannot reopen late"
            );
            assert!(app.edit.is_none());
        } else {
            assert!(app.apply_open_editor(request, Ok(document)));
            assert_eq!(app.edit.as_ref().unwrap().content, "fixture");
        }
        assert!(app.running);
    }
}
