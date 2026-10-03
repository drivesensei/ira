use super::*;
use ira_core::model::{Confirm, ConfirmAction};
use ira_core::services::transfer::{OverwritePolicy, rename_no_replace};
use std::sync::Condvar;
#[derive(Default)]
struct Gate {
    entered: AtomicBool,
    released: Mutex<bool>,
    ready: Condvar,
}
impl Gate {
    fn block(&self) {
        self.entered.store(true, Ordering::Release);
        let mut open = self.released.lock().unwrap();
        while !*open {
            open = self.ready.wait(open).unwrap();
        }
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.ready.notify_all();
    }
    fn wait(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.entered.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
    }
}
struct Release(Arc<Gate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
fn finish(runtime: &Runtime) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !runtime.shutdown_complete() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(matches!(
        runtime.shutdown_state(),
        Some(ShutdownState::Success { .. })
    ));
}
#[test]
fn runtime_waits_for_actual_accepted_transfer_provider_return_before_final_persistence() {
    let _fixture = crate::test_support::enter();
    let root = crate::test_support::current().unwrap().directory.clone();
    let gate = Arc::new(Gate::default());
    let hold = gate.clone();
    let _release = Release(gate.clone());
    std::fs::write(root.join("source"), b"owned source").unwrap();
    std::fs::create_dir(root.join("destination")).unwrap();
    let owned = root.clone();
    let mut runtime = Runtime::with_factory(7, move || {
        let mut app = App::default();
        app.state_path = Some(owned.join("state"));
        app.bookmarks_path = Some(owned.join("bookmarks"));
        app.window_generation = 7;
        app.set_transfer_provider(Arc::new(move |src, dst| {
            hold.block();
            rename_no_replace(src, dst)
        }));
        app.confirming = Some(Confirm {
            action: ConfirmAction::Move,
            policy: OverwritePolicy::AutoRename,
            label: "owned".into(),
            paths: vec![owned.join("source").to_str().unwrap().into()],
            dest_dir: Some(owned.join("destination").to_str().unwrap().into()),
        });
        app
    });
    runtime.enqueue(
        Command::Input(Input::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        None,
    );
    runtime.flush();
    gate.wait();
    runtime.stop(&[]);
    let until = Instant::now() + Duration::from_millis(2200);
    while Instant::now() < until {
        assert!(!runtime.shutdown_complete());
        assert!(!matches!(
            runtime.shutdown_state(),
            Some(ShutdownState::Success { .. })
        ));
        assert!(!root.join("state").exists());
        thread::yield_now();
    }
    gate.release();
    finish(&runtime);
    // This gates an actual provider continuation, not a genuinely blocked OS syscall.
    assert!(root.join("state").exists());
    assert_eq!(std::fs::read(root.join("source")).unwrap(), b"owned source");
    assert!(!root.join("destination/source").exists());
}

struct EditorHookScope(Option<EditorHook>);
impl Drop for EditorHookScope {
    fn drop(&mut self) {
        EDITOR_HOOK.with(|slot| slot.replace(self.0.take()));
    }
}
#[test]
fn runtime_real_editor_save_waits_then_skips_queued_save_after_stop() {
    let _fixture = crate::test_support::enter();
    let root = crate::test_support::current().unwrap().directory.clone();
    let path = root.join("edit.txt");
    std::fs::write(&path, b"old\r\n").unwrap();
    let gate = Arc::new(Gate::default());
    let hold = gate.clone();
    let _release = Release(gate.clone());
    let _hook = EditorHookScope(
        EDITOR_HOOK.with(|slot| slot.replace(Some(Arc::new(move || hold.block())))),
    );
    let owned = root.clone();
    let input = path.clone();
    let mut runtime = Runtime::with_factory(7, move || {
        let mut app = App::default();
        app.state_path = Some(owned.join("state"));
        app.bookmarks_path = Some(owned.join("bookmarks"));
        app.window_generation = 7;
        app.panes[0].files = vec![ira_core::services::list_files::FEntry {
            path: input.to_str().unwrap().into(),
            label: "edit.txt".into(),
            is_dir: false,
            size: 5,
            modified: None,
        }];
        app.panes[0].selected = vec![false];
        app.panes[0].state.select(Some(0));
        app.panes[0].preview_mode = ira_core::model::PreviewMode::Column;
        app.panes[0].listing_settled = true;
        app
    });
    runtime.enqueue(
        Command::Input(Input::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))),
        None,
    );
    runtime.flush();
    let deadline = Instant::now() + Duration::from_secs(5);
    let id = loop {
        if let Some(publication) = runtime.try_snapshot() {
            if let Some(edit) = &publication.snapshot.edit {
                break edit.document_id;
            }
        }
        assert!(Instant::now() < deadline);
        thread::yield_now();
    };
    runtime.enqueue(
        Command::SaveDraft {
            document_id: id,
            revision: 1,
            text: "first\n".into(),
        },
        None,
    );
    runtime.flush();
    gate.wait();
    let sequence = runtime.enqueue(
        Command::SaveDraft {
            document_id: id,
            revision: 2,
            text: "second\n".into(),
        },
        None,
    );
    runtime.flush();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(publication) = runtime.try_snapshot() {
            if publication.snapshot.ack_sequence == sequence {
                break;
            }
        }
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    runtime.stop(&[]);
    let until = Instant::now() + Duration::from_millis(2200);
    while Instant::now() < until {
        assert!(!runtime.shutdown_complete());
        assert!(!root.join("state").exists());
        thread::yield_now();
    }
    gate.release();
    finish(&runtime);
    assert_eq!(std::fs::read(&path).unwrap(), b"first\r\n");
    // Actual actor dispatch, OpenEditor and EditorSession::save; gate models the
    // already-entered worker continuation immediately before save, not blocked OS I/O.
}
#[test]
fn actual_original_startup_is_applied_before_immediate_stop_publishes_state() {
    let _fixture = crate::test_support::enter();
    let root = crate::test_support::current().unwrap().directory.clone();
    let state = root.join("state");
    let bookmarks = root.join("bookmarks");
    ira_core::services::state::save_state_to(
        &state,
        &ira_core::services::state::SessionState {
            show_hidden: true,
            split: true,
            active_pane: 1,
            preview: [1, 2],
            theme: Some("light".into()),
            ..Default::default()
        },
    );
    std::fs::write(&bookmarks, b"").unwrap();
    let path = state.clone();
    let mut runtime =
        Runtime::with_factory(7, move || App::new_with_paths(Some(path), Some(bookmarks)));
    runtime.stop(&[]);
    finish(&runtime);
    let saved = ira_core::services::state::load_state_from(&state);
    assert!(saved.show_hidden);
    assert!(saved.split);
    assert_eq!(saved.active_pane, 1);
    assert_eq!(saved.preview, [1, 2]);
}
#[test]
fn editor_sequence_overflow_and_missing_worker_cutoff_never_authorize_success() {
    let (tx, _rx) = mpsc::channel();
    let lane = EditorDispatch {
        sender: std::cell::RefCell::new(Some(tx)),
        issued: std::cell::Cell::new(u64::MAX),
        error: std::cell::RefCell::new(None),
        settlement: Arc::new(EditorSettlement::default()),
    };
    lane.submit(7, HostRequest::RefreshDrives);
    assert!(lane.poll(lane.seal()).is_err());
    let (tx, rx) = mpsc::channel();
    drop(rx);
    let lane = EditorDispatch {
        sender: std::cell::RefCell::new(Some(tx)),
        issued: std::cell::Cell::new(0),
        error: std::cell::RefCell::new(None),
        settlement: Arc::new(EditorSettlement::default()),
    };
    lane.submit(7, HostRequest::RefreshDrives);
    assert!(lane.poll(lane.seal()).is_err());
}
