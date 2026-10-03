use super::*;
use crate::input::{handle_key_events, KeyCode, KeyEvent, KeyModifiers};
fn entry(name: &str, size: u64) -> FEntry {
    FEntry {
        path: format!("/fixture/{name}"),
        label: name.into(),
        is_dir: false,
        size,
        modified: None,
    }
}
fn fixture() -> App {
    let mut app = App::default();
    app.panes[0].files = vec![
        entry("alpha.txt", 20),
        entry("beta.txt", 10),
        entry("hidden.txt", 30),
    ];
    app.panes[0].selected = vec![false, false, true];
    app.panes[0].state.select(Some(0));
    app.panes[0].listing_settled = true;
    app
}
#[test] // Frozen oracle app.rs:3354: hidden selections remain operation sources.
fn operation_sources_include_hidden_selected_entries() {
    let mut app = fixture();
    app.panes[0].filter_query = Some("alpha".into());
    app.panes[0].filter_indices = vec![0];
    assert_eq!(app.collect_sources(), vec!["/fixture/hidden.txt"]);
}
#[test] // Frozen oracle app.rs:2298: size sort carries selection and cursor identity.
fn sort_carries_bits_and_cursor_identity() {
    let mut app = fixture();
    app.cycle_sort();
    assert_eq!(
        app.panes[0]
            .files
            .iter()
            .map(|e| e.label.as_str())
            .collect::<Vec<_>>(),
        vec!["hidden.txt", "alpha.txt", "beta.txt"]
    );
    assert_eq!(app.panes[0].selected, vec![true, false, false]);
    assert_eq!(app.panes[0].state.selected(), Some(1));
}
#[test] // Frozen oracle handler.rs:global Ctrl runs before help/error/search.
fn control_select_precedes_help_and_errors() {
    let mut app = fixture();
    app.keybindings_visible = true;
    app.set_status("error", true);
    handle_key_events(
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        &mut app,
    )
    .unwrap();
    assert_eq!(app.panes[0].selected, vec![true, true, true]);
    assert!(app.keybindings_visible);
    assert!(app.status.as_ref().unwrap().is_error);
}
#[test] // Frozen oracle handler.rs:help wins over error; notices allow normal keys.
fn help_dismissal_preserves_error_and_notice_does_not_consume_preview() {
    let mut app = fixture();
    app.keybindings_visible = true;
    app.set_status("error", true);
    handle_key_events(
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
        &mut app,
    )
    .unwrap();
    assert!(!app.keybindings_visible);
    assert!(app.status.is_some());
    app.set_status("notice", false);
    handle_key_events(
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
        &mut app,
    )
    .unwrap();
    assert_eq!(app.panes[0].preview_mode, PreviewMode::Column);
}
#[test] // Frozen oracle app.rs:807: paste strips exactly one CRLF and cursor is chars.
fn paste_into_prompt_handles_unicode_and_one_trailing_crlf() {
    let mut app = fixture();
    app.new_entry = Some(NewEntryPrompt {
        text: vec!['a'],
        cursor: 0,
    });
    app.handle_paste("Ж🙂\r\n");
    let prompt = app.new_entry.as_ref().unwrap();
    assert_eq!(prompt.text, vec!['Ж', '🙂', 'a']);
    assert_eq!(prompt.cursor, 2);
}
#[test] // Frozen oracle app.rs:2497: pane0 -> pane1 -> board -> pane0.
fn focus_cycle_clears_live_search() {
    let mut app = fixture();
    app.split = true;
    app.copy_board = true;
    app.search_query = Some("a".into());
    app.switch_pane();
    assert_eq!(app.active_pane, 1);
    assert!(app.search_query.is_none());
    app.switch_pane();
    assert!(app.board_focused);
    app.switch_pane();
    assert_eq!(app.active_pane, 0);
    assert!(!app.board_focused);
}

struct TempFixture(PathBuf);
impl TempFixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ira-model-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn app(&self) -> App {
        let mut app = App {
            state_path: Some(self.0.join("state")),
            bookmarks_path: Some(self.0.join("bookmarks")),
            ..App::default()
        };
        app.panes[0].folder = Some(Folder::new(
            "fixture".into(),
            self.0.to_string_lossy().into_owned(),
            '#',
        ));
        app
    }
}
impl Drop for TempFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn until(app: &mut App, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.tick();
        if ready(app) {
            return;
        }
        assert!(Instant::now() < deadline, "worker did not finish");
        std::thread::yield_now();
    }
}
#[test] // Oracle app.rs:1105: final generation clears selected bits; stale chunks are discarded.
fn listing_completion_clears_bits_and_drops_stale_generation() {
    let mut app = fixture();
    app.panes[0].listing_generation = 9;
    app.file_list_tx
        .send((0, vec![entry("wrong", 0)], true, 8))
        .unwrap();
    app.pick_up_pane_listings();
    assert_eq!(app.panes[0].files[0].label, "alpha.txt");
    app.file_list_tx
        .send((0, vec![entry("fresh", 0)], true, 9))
        .unwrap();
    app.pick_up_pane_listings();
    assert_eq!(app.panes[0].selected, vec![false]);
    assert_eq!(app.panes[0].state.selected(), Some(0));
}
#[test] // Oracle app.rs:989: bounded final pass uses names, clears selection, recomputes filter.
fn bounded_worker_settle_uses_name_order_and_preserves_filter() {
    let tmp = TempFixture::new();
    std::fs::write(tmp.0.join("beta.txt"), "b").unwrap();
    std::fs::write(tmp.0.join("alpha.txt"), "a").unwrap();
    let mut app = tmp.app();
    app.panes[0].filter_query = Some("alpha".into());
    app.panes[0].selected = vec![true];
    app.list_files_from_selected_folder();
    until(&mut app, |a| a.panes[0].listing_settled);
    assert_eq!(
        app.panes[0]
            .files
            .iter()
            .map(|e| e.label.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha.txt", "beta.txt"]
    );
    assert_eq!(app.panes[0].selected, vec![false, false]);
    assert_eq!(app.panes[0].filter_indices, vec![0]);
}
#[test]
fn bounded_stale_result_does_not_replace_current_pane() {
    let mut app = fixture();
    app.panes[0].listing_generation = 3;
    app.bounded_tx
        .send((0, 2, Ok((vec![entry("stale", 0)], true))))
        .unwrap();
    app.pick_up_bounded_listings();
    assert_eq!(app.panes[0].files[0].label, "alpha.txt");
}
#[test] // Oracle app.rs:3014: collision closes rename and preserves both files.
fn rename_collision_preserves_bytes_and_exact_error() {
    let tmp = TempFixture::new();
    std::fs::write(tmp.0.join("a.txt"), "a").unwrap();
    std::fs::write(tmp.0.join("b.txt"), "b").unwrap();
    let mut app = tmp.app();
    app.panes[0].files = vec![FEntry {
        path: tmp.0.join("a.txt").to_string_lossy().into_owned(),
        ..entry("a.txt", 1)
    }];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));
    app.start_rename();
    app.renaming.as_mut().unwrap().text = "b.txt".chars().collect();
    app.commit_rename();
    assert!(app.renaming.is_none());
    until(&mut app, |a| a.status.is_some());
    assert_eq!(
        app.status.as_ref().unwrap().text,
        "Cannot rename: 'a.txt' already exists."
    );
    assert_eq!(std::fs::read(tmp.0.join("a.txt")).unwrap(), b"a");
    assert_eq!(std::fs::read(tmp.0.join("b.txt")).unwrap(), b"b");
}
#[test] // Oracle app.rs:2845: extension rule, nested parents, created item revealed after settle.
fn create_nested_unicode_file_reveals_created_entry() {
    let tmp = TempFixture::new();
    let mut app = tmp.app();
    app.start_new_entry();
    app.handle_paste("nested/Ж🙂.txt");
    app.confirm_new_entry();
    until(&mut app, |a| {
        a.new_entry.is_none() && a.panes[0].listing_settled
    });
    assert!(tmp.0.join("nested/Ж🙂.txt").is_file());
    assert_eq!(
        Path::new(&app.panes[0].folder.as_ref().unwrap().path),
        tmp.0.join("nested")
    );
    assert_eq!(app.selected_visible_entry().unwrap().label, "Ж🙂.txt");
}
#[test] // Oracle app.rs:2687: goto creates missing path by extension and closes prompt on failure/success.
fn goto_creates_folder_and_empty_prompt_closes() {
    let tmp = TempFixture::new();
    let mut app = tmp.app();
    app.start_goto();
    app.goto_push("new/deep");
    app.confirm_goto();
    assert!(app.goto_prompt.is_none());
    until(&mut app, |a| a.panes[0].listing_settled);
    assert!(tmp.0.join("new/deep").is_dir());
    assert_eq!(
        Path::new(&app.panes[0].folder.as_ref().unwrap().path),
        tmp.0.join("new/deep")
    );
}
#[test] // Oracle app.rs:3888+4008: exact legacy codecs; serial writes preserve newest value.
fn serialized_persistence_keeps_last_snapshot_and_legacy_bytes() {
    let tmp = TempFixture::new();
    let mut app = tmp.app();
    app.show_hidden = false;
    app.persist_state();
    app.show_hidden = true;
    app.split = true;
    app.panes[1].preview_mode = PreviewMode::Grid;
    app.persist_state();
    app.persistence_barrier()
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let loaded = load_state_from(app.state_path.as_ref().unwrap());
    assert!(loaded.show_hidden);
    assert!(loaded.split);
    assert_eq!(loaded.preview, [3, 2]);
    let bytes = std::fs::read_to_string(app.state_path.as_ref().unwrap()).unwrap();
    assert!(bytes.contains("hidden=1\n"));
    assert!(bytes.contains("preview1=2\n"));
}
#[test]
fn snapshots_are_owned_and_include_hidden_sources() {
    let mut app = fixture();
    app.panes[0].filter_query = Some("alpha".into());
    app.panes[0].filter_indices = vec![0];
    let snapshot = app.snapshot();
    assert_eq!(snapshot.panes[0].rows.len(), 1);
    assert_eq!(
        snapshot.panes[0].selected_paths,
        vec![PathBuf::from("/fixture/hidden.txt")]
    );
    app.panes[0].files[0].label = "changed".into();
    assert_eq!(snapshot.panes[0].rows[0].entry.label, "alpha.txt");
}
#[test]
fn stale_mouse_target_rejected_after_generation_change() {
    let mut app = fixture();
    let target = EntryTarget {
        pane: 0,
        path: PathBuf::from("/fixture/alpha.txt"),
        listing_generation: 0,
    };
    assert!(app.resolve_target(&target).is_some());
    app.panes[0].listing_generation = 1;
    assert!(app.resolve_target(&target).is_none());
}
#[test] // Oracle handler.rs: rename has priority above help, error and other text prompts.
fn rename_priority_and_control_a_exception_preserved() {
    let mut app = fixture();
    app.start_rename();
    app.keybindings_visible = true;
    handle_key_events(
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        &mut app,
    )
    .unwrap();
    assert_eq!(app.panes[0].selected, vec![false, false, true]);
    let before = app.renaming.as_ref().unwrap().text.clone();
    handle_key_events(
        KeyEvent::new(KeyCode::Char('Ж'), KeyModifiers::NONE),
        &mut app,
    )
    .unwrap();
    assert_eq!(app.renaming.as_ref().unwrap().text.len(), before.len() + 1);
    assert!(app.keybindings_visible);
}
#[test] // Oracle handler.rs: any non-control key hides deletion UI; worker keeps running.
fn hiding_delete_dialog_keeps_worker_control_and_paths() {
    let mut app = fixture();
    let control = JobControl::new();
    app.deletion = Some(DeletionState {
        total: 3,
        done: 1,
        current: Some("/fixture/alpha.txt".into()),
        started: Instant::now(),
        control: control.clone(),
    });
    handle_key_events(
        KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        &mut app,
    )
    .unwrap();
    assert!(app.running);
    assert!(app.deletion_box_hidden);
    assert!(Arc::ptr_eq(
        &app.deletion.as_ref().unwrap().control,
        &control
    ));
}
#[test] // Oracle handler.rs: normal Alt Up moves to top then invokes previous-row clamping.
fn normal_alt_up_calls_previous_and_search_alt_up_only_goes_top() {
    let mut app = fixture();
    app.panes[0].state.select(Some(1));
    handle_key_events(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &mut app).unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(0));
    app.start_search();
    handle_key_events(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT), &mut app).unwrap();
    assert_eq!(app.panes[0].state.selected(), Some(0));
}
#[test]
fn job_completion_applies_status_and_failure_without_losing_job() {
    let mut app = fixture();
    app.jobs.push(Job {
        id: 7,
        kind: JobKind::Copy,
        overwrite: OverwritePolicy::AutoRename,
        paths: vec!["/fixture/a".into()],
        dest_dir: "/fixture/dest".into(),
        label: "a".into(),
        total_bytes: None,
        copied_bytes: 0,
        current: String::new(),
        status: JobStatus::Running,
        started_at: Instant::now(),
        control: JobControl::new(),
    });
    app.job_tx
        .send(JobEvent::Failed {
            id: 7,
            error: "test failure".into(),
        })
        .unwrap();
    app.drain_jobs();
    assert_eq!(app.jobs[0].status, JobStatus::Failed("test failure".into()));
    assert_eq!(app.jobs.len(), 1);
}

fn navigation_result(app: &App, folder: &str, ticket: u64, error: Option<&str>) -> OperationResult {
    let before = app.operation_state();
    let source = std::array::from_fn(|i| {
        (
            before.panes[i].folder.as_ref().map(|f| f.path.clone()),
            before.panes[i].listing_generation,
        )
    });
    let mut state = before.clone();
    state.panes[0].folder = Some(Folder::new(folder.into(), folder.into(), '#'));
    OperationResult {
        epoch: app.operation_epoch.load(Ordering::Acquire),
        navigation_ticket: Some(ticket),
        before,
        source,
        state,
        status: error.map(|text| Status {
            text: text.into(),
            is_error: true,
            raised: Instant::now(),
        }),
        listings: vec![],
        host_requests: vec![],
    }
}
#[test]
fn latest_absolute_navigation_wins_a_b_reversed_completions() {
    let mut app = fixture();
    let a = navigation_result(&app, "/A", 1, Some("stale failure"));
    let b = navigation_result(&app, "/B", 2, None);
    app.navigation_generation[0] = 2;
    app.navigation_inflight[0] = Some(2);
    app.pending_operations = 2;
    app.apply_operation_result(b);
    app.apply_operation_result(a);
    assert_eq!(app.panes[0].folder.as_ref().unwrap().path, "/B");
    assert!(app.status.is_none());
    assert_eq!(app.pending_operations, 0);
}
#[test]
fn latest_absolute_navigation_wins_a_b_a_reversed_completions() {
    let mut app = fixture();
    let first_a = navigation_result(&app, "/A", 1, Some("old A failed"));
    let b = navigation_result(&app, "/B", 2, Some("old B failed"));
    let last_a = navigation_result(&app, "/A", 3, None);
    app.navigation_generation[0] = 3;
    app.navigation_inflight[0] = Some(3);
    app.pending_operations = 3;
    app.apply_operation_result(last_a);
    app.apply_operation_result(b);
    app.apply_operation_result(first_a);
    assert_eq!(app.panes[0].folder.as_ref().unwrap().path, "/A");
    assert!(app.status.is_none());
    assert_eq!(app.pending_operations, 0);
}
#[test]
fn relative_navigation_waits_for_preceding_destination_rows() {
    let tmp = TempFixture::new();
    std::fs::create_dir_all(tmp.0.join("first/second")).unwrap();
    let mut app = tmp.app();
    app.start_goto();
    app.goto_push("first");
    app.confirm_goto();
    app.enter_folder();
    assert_eq!(app.relative_navigation.len(), 1);
    until(&mut app, |a| {
        a.pending_operations() == 0 && a.panes[0].listing_settled
    });
    assert_eq!(
        Path::new(&app.panes[0].folder.as_ref().unwrap().path),
        tmp.0.join("first/second")
    );
}
#[test]
fn quit_bypasses_stale_focus_envelope_and_pending_worker() {
    let mut app = fixture();
    app.navigation_inflight[0] = Some(1);
    app.pending_operations = 1;
    let accepted = app
        .dispatch_envelope(crate::input::CommandEnvelope {
            sequence: 77,
            window_generation: 0,
            document_generation: 100,
            focus_generation: 100,
            command: crate::input::Input::Action(crate::input::Command::Quit),
        })
        .unwrap();
    assert!(accepted);
    assert!(!app.running);
    assert_eq!(app.ack_sequence, 77);
}
#[test]
fn event_dispatch_filters_release_as_frozen_oracle_event_loop() {
    let mut app = fixture();
    let key = KeyEvent {
        code: KeyCode::Char('q'),
        modifiers: KeyModifiers::NONE,
        phase: crate::input::KeyPhase::Release,
    };
    app.dispatch(crate::input::Input::Key(key)).unwrap();
    assert!(app.running);
}
#[test]
fn stale_generation_envelope_acknowledged_without_mutation() {
    let mut app = fixture();
    let accepted = app
        .dispatch_envelope(crate::input::CommandEnvelope {
            sequence: 8,
            window_generation: 0,
            document_generation: 1,
            focus_generation: 0,
            command: crate::input::Input::Action(crate::input::Command::ToggleSelectAll),
        })
        .unwrap();
    assert!(!accepted);
    assert_eq!(app.ack_sequence, 8);
    assert_eq!(app.panes[0].selected, vec![false, false, true]);
}

#[test]
fn startup_result_restores_legacy_state_before_queued_input() {
    let tmp = TempFixture::new();
    let mut app = tmp.app();
    app.initializing = true;
    app.dispatch(crate::input::Input::Action(
        crate::input::Command::ToggleHidden,
    ))
    .unwrap();
    assert_eq!(app.startup_inputs.len(), 1);
    let state = SessionState {
        show_hidden: true,
        split: true,
        preview: [1, 2],
        left: app.panes[0].folder.clone(),
        ..SessionState::default()
    };
    app.startup_tx
        .send((
            state,
            vec![("Saved".into(), tmp.0.to_string_lossy().into_owned())],
        ))
        .unwrap();
    app.tick();
    assert!(!app.is_initializing());
    assert!(!app.show_hidden);
    assert!(app.split);
    assert_eq!(app.panes[0].preview_mode, PreviewMode::Column);
    assert_eq!(app.bookmarks.as_ref().unwrap().len(), 1);
    app.persistence_barrier()
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
}
#[test]
fn startup_quit_is_immediate_without_waiting_for_codec_read() {
    let mut app = fixture();
    app.initializing = true;
    app.dispatch(crate::input::Input::Action(crate::input::Command::Quit))
        .unwrap();
    assert!(!app.running);
    assert!(app.startup_inputs.is_empty());
}
#[test]
fn stale_transfer_destination_probe_does_not_refresh_new_transfer() {
    let mut app = fixture();
    app.transfer_generation = 2;
    let before = app.panes[0].listing_generation;
    app.transfer_probe_tx
        .send((
            1,
            TransferDestSync {
                dest_dir: "/fixture".into(),
                reveal_path: "/fixture/new".into(),
                last_refresh: Instant::now(),
            },
            true,
        ))
        .unwrap();
    app.refresh_transfer_destinations();
    assert_eq!(app.panes[0].listing_generation, before);
    assert!(app.panes[0].pending_select.is_none());
}
#[test]
fn transfer_refresh_respects_user_navigated_suppression() {
    let mut app = fixture();
    app.panes[0].folder = Some(Folder::new("fixture".into(), "/fixture".into(), '#'));
    app.panes[0].user_navigated = true;
    let before = app.panes[0].listing_generation;
    app.apply_transfer_destination_refresh(TransferDestSync {
        dest_dir: "/fixture".into(),
        reveal_path: "/fixture/new".into(),
        last_refresh: Instant::now(),
    });
    assert_eq!(app.panes[0].listing_generation, before);
    assert!(app.panes[0].pending_select.is_none());
}
#[test]
fn close_invalidates_old_operation_and_listing_completions() {
    let mut app = fixture();
    let result = navigation_result(&app, "/late", 1, None);
    let generation = app.panes[0].listing_generation;
    app.navigation_generation[0] = 1;
    app.pending_operations = 1;
    app.cancel_pending_work();
    app.apply_operation_result(result);
    app.file_list_tx
        .send((0, vec![entry("old", 0)], true, generation))
        .unwrap();
    app.pick_up_pane_listings();
    assert!(app.panes[0].folder.is_none());
    assert_eq!(app.panes[0].files[0].label, "alpha.txt");
    assert_eq!(app.pending_operations(), 0);
}
fn editor_fixture(tmp: &TempFixture) -> App {
    let path = tmp.0.join("edit.txt");
    std::fs::write(&path, "one\r\ntwo\r\n").unwrap();
    let mut app = tmp.app();
    app.panes[0].files = vec![FEntry {
        path: path.to_string_lossy().into_owned(),
        ..entry("edit.txt", 10)
    }];
    app.panes[0].selected = vec![false];
    app.panes[0].state.select(Some(0));
    app.panes[0].preview_mode = PreviewMode::Column;
    app.panes[0].listing_settled = true;
    app
}
fn open_editor(app: &mut App) {
    app.switch_pane();
    let request = app
        .take_host_requests()
        .into_iter()
        .find_map(|r| match r {
            HostRequest::OpenEditor(request) => Some(request),
            _ => None,
        })
        .unwrap();
    let document = crate::editor::open_document(request.document_id, &request.target.path).unwrap();
    assert!(app.apply_open_editor(request, Ok(document)));
}
#[test]
fn editor_focus_keeps_plain_s_and_control_a_out_of_pane_commands() {
    let tmp = TempFixture::new();
    let mut app = editor_fixture(&tmp);
    open_editor(&mut app);
    handle_key_events(
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE),
        &mut app,
    )
    .unwrap();
    handle_key_events(
        KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        &mut app,
    )
    .unwrap();
    let requests = app.take_host_requests();
    assert!(matches!(
        requests[0],
        HostRequest::EditorKey {
            key: KeyEvent {
                code: KeyCode::Char('s'),
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        requests[1],
        HostRequest::EditorKey {
            key: KeyEvent {
                code: KeyCode::Char('a'),
                modifiers: KeyModifiers::CONTROL,
                ..
            },
            ..
        }
    ));
    assert_eq!(app.panes[0].selected, vec![false]);
}
#[test]
fn close_during_open_rejects_late_document_and_tab_discards() {
    let tmp = TempFixture::new();
    let mut app = editor_fixture(&tmp);
    app.switch_pane();
    let request = app.pending_editor_request().unwrap();
    let document = crate::editor::open_document(request.document_id, &request.target.path).unwrap();
    app.switch_pane();
    assert!(!app.edit_focus);
    assert!(!app.apply_open_editor(request, Ok(document)));
    assert!(app.edit.is_none());
    open_editor(&mut app);
    let id = app.edit.as_ref().unwrap().document_id;
    app.update_editor_draft(id, 1, "unsaved".into());
    app.switch_pane();
    assert!(app.edit.is_none());
    assert_eq!(
        std::fs::read(tmp.0.join("edit.txt")).unwrap(),
        b"one\r\ntwo\r\n"
    );
}
#[test]
fn save_snapshot_is_immutable_and_completion_only_clears_matching_revision() {
    let tmp = TempFixture::new();
    let mut app = editor_fixture(&tmp);
    open_editor(&mut app);
    let id = app.edit.as_ref().unwrap().document_id;
    app.update_editor_draft(id, 1, "first\n".into());
    let snapshot = app.editor_save_snapshot().unwrap();
    app.update_editor_draft(id, 2, "newer\n".into());
    assert_eq!(snapshot.content, "first\n");
    let completion = crate::editor::save_document(&snapshot).unwrap();
    assert!(app.apply_save_result(id, 1, Ok(completion)));
    assert!(app.edit.as_ref().unwrap().dirty);
    assert_eq!(app.edit.as_ref().unwrap().content, "newer\n");
    assert_eq!(std::fs::read(tmp.0.join("edit.txt")).unwrap(), b"first\r\n");
    let latest = app.editor_save_snapshot().unwrap();
    let completion = crate::editor::save_document(&latest).unwrap();
    assert!(app.apply_save_result(id, 2, Ok(completion)));
    assert!(!app.edit.as_ref().unwrap().dirty);
    assert_eq!(std::fs::read(tmp.0.join("edit.txt")).unwrap(), b"newer\r\n");
}
#[test]
fn save_completion_cannot_modify_reopened_document() {
    let tmp = TempFixture::new();
    let mut app = editor_fixture(&tmp);
    open_editor(&mut app);
    let id = app.edit.as_ref().unwrap().document_id;
    app.update_editor_draft(id, 1, "saved".into());
    let completion = crate::editor::save_document(&app.editor_save_snapshot().unwrap()).unwrap();
    app.close_edit();
    app.edit_focus = false;
    open_editor(&mut app);
    let new_id = app.edit.as_ref().unwrap().document_id;
    assert_ne!(id, new_id);
    assert!(!app.apply_save_result(id, 1, Ok(completion)));
    assert_eq!(app.edit.as_ref().unwrap().document_id, new_id);
}
#[test]
fn deterministic_clock_expires_notice_and_ramps_held_navigation() {
    let mut app = fixture();
    let start = Instant::now();
    app.dispatch_at(
        crate::input::Input::Action(crate::input::Command::CyclePreview),
        start,
    )
    .unwrap();
    assert!(app.status.is_some());
    app.dispatch_at(crate::input::Input::Tick, start + STATUS_TTL)
        .unwrap();
    assert!(app.status.is_none());
    app.panes[0].files = (0..20).map(|i| entry(&format!("{i}.txt"), 0)).collect();
    app.panes[0].selected = vec![false; 20];
    app.panes[0].state.select(Some(0));
    for i in 0..7 {
        app.dispatch_at(
            crate::input::Input::Action(crate::input::Command::MoveNext),
            start + Duration::from_millis(i * 20),
        )
        .unwrap();
    }
    assert_eq!(app.panes[0].state.selected(), Some(8));
}
// Independent QA probe G-0030 from evidence/T-016/qa-model-probes.rs.
#[test]
fn qa_accepted_older_sequence_never_regresses_acknowledgement() {
    let tmp = TempFixture::new();
    let mut app = tmp.app();
    let envelope = |app: &App, sequence| crate::input::CommandEnvelope {
        sequence,
        window_generation: app.window_generation,
        document_generation: app.document_generation,
        focus_generation: app.focus_generation,
        command: crate::input::Input::Action(crate::input::Command::CycleTheme),
    };
    app.dispatch_envelope(envelope(&app, 20)).unwrap();
    app.dispatch_envelope(envelope(&app, 10)).unwrap();
    assert_eq!(
        app.ack_sequence, 20,
        "acknowledgement must remain a monotonic high-water mark"
    );
    app.persistence_barrier()
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
}

#[test]
fn duplicate_mutation_envelope_is_ignored_and_window_sequences_restart() {
    let tmp = TempFixture::new();
    let mut app = tmp.app();
    app.start_new_entry();
    app.handle_paste("new.txt");
    let envelope = crate::input::CommandEnvelope {
        sequence: 5,
        window_generation: 0,
        document_generation: 0,
        focus_generation: 0,
        command: crate::input::Input::Action(crate::input::Command::ConfirmNewEntry),
    };
    assert!(app.dispatch_envelope(envelope.clone()).unwrap());
    assert!(!app.dispatch_envelope(envelope).unwrap());
    assert_eq!(app.pending_operations(), 1);
    until(&mut app, |a| {
        a.pending_operations() == 0 && a.panes[0].listing_settled
    });
    app.attach_window(2);
    assert_eq!(app.ack_sequence, 0);
    let stale = crate::input::CommandEnvelope {
        sequence: 999,
        window_generation: 0,
        document_generation: app.document_generation,
        focus_generation: app.focus_generation,
        command: crate::input::Input::Action(crate::input::Command::Quit),
    };
    assert!(!app.dispatch_envelope(stale).unwrap());
    assert_eq!(app.ack_sequence, 0);
    assert!(app.running);
    let current = crate::input::CommandEnvelope {
        sequence: 0,
        window_generation: 2,
        document_generation: app.document_generation,
        focus_generation: app.focus_generation,
        command: crate::input::Input::Action(crate::input::Command::Quit),
    };
    assert!(app.dispatch_envelope(current).unwrap());
    assert!(!app.running);
}
// Independent T020 probes G-0032/G-0033, strengthened to explicit rejection.
#[test]
fn conflicting_equal_draft_revision_is_rejected_before_old_save_completion() {
    let tmp = TempFixture::new();
    let mut app = editor_fixture(&tmp);
    open_editor(&mut app);
    let id = app.edit.as_ref().unwrap().document_id;
    assert!(app.update_editor_draft(id, 1, "first".into()));
    let saved = crate::editor::save_document(&app.editor_save_snapshot().unwrap()).unwrap();
    assert!(!app.update_editor_draft(id, 1, "second".into()));
    assert_eq!(app.edit.as_ref().unwrap().content, "first");
    assert!(app.apply_save_result(id, 1, Ok(saved)));
    assert!(!app.edit.as_ref().unwrap().dirty);
    assert_eq!(std::fs::read(tmp.0.join("edit.txt")).unwrap(), b"first");
}
#[test]
fn late_editor_open_cannot_steal_newer_help_focus() {
    let tmp = TempFixture::new();
    let mut app = editor_fixture(&tmp);
    app.open_edit();
    let request = app.pending_editor_request().unwrap();
    let document = crate::editor::open_document(request.document_id, &request.target.path).unwrap();
    app.dispatch(crate::input::Input::Action(crate::input::Command::ShowHelp))
        .unwrap();
    assert!(app.keybindings_visible);
    assert_ne!(request.focus_generation, app.focus_generation);
    assert!(!app.apply_open_editor(request, Ok(document)));
    assert!(!app.edit_focus);
    assert_eq!(app.input_context(), crate::input::InputContext::Help);
}
#[test]
fn identical_equal_draft_revision_is_idempotent_without_dirty_or_notification_change() {
    let tmp = TempFixture::new();
    let mut app = editor_fixture(&tmp);
    open_editor(&mut app);
    let id = app.edit.as_ref().unwrap().document_id;
    assert!(app.update_editor_draft(id, 1, "same".into()));
    let completion = crate::editor::save_document(&app.editor_save_snapshot().unwrap()).unwrap();
    assert!(app.apply_save_result(id, 1, Ok(completion)));
    let revision = app.revision;
    assert!(app.update_editor_draft(id, 1, "same".into()));
    assert_eq!(app.revision, revision);
    assert!(!app.edit.as_ref().unwrap().dirty);
    assert!(!app.update_editor_draft(id, 1, "conflicting".into()));
    assert_eq!(app.revision, revision);
    assert_eq!(app.edit.as_ref().unwrap().content, "same");
}
#[test]
fn editor_open_ticket_accepts_its_tab_focus_transition_and_rejects_later_generations() {
    let tmp = TempFixture::new();
    let mut app = editor_fixture(&tmp);
    app.dispatch(crate::input::Input::Key(KeyEvent::new(
        KeyCode::Tab,
        KeyModifiers::NONE,
    )))
    .unwrap();
    let request = app.pending_editor_request().unwrap();
    assert_eq!(request.focus_generation, app.focus_generation);
    let document = crate::editor::open_document(request.document_id, &request.target.path).unwrap();
    assert!(app.apply_open_editor(request, Ok(document)));
    app.close_edit();
    app.edit_focus = false;
    for stale_generation in 0..3 {
        app.open_edit();
        let request = app.pending_editor_request().unwrap();
        let document =
            crate::editor::open_document(request.document_id, &request.target.path).unwrap();
        match stale_generation {
            0 => app.focus_generation += 1,
            1 => app.document_generation += 1,
            _ => app.window_generation += 1,
        };
        assert!(!app.apply_open_editor(request, Ok(document)));
        assert!(app.pending_editor_request().is_none());
        assert!(!app.edit_focus);
    }
}
