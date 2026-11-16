use super::*;
use ira_core::{
    model::{Confirm, ConfirmAction},
    services::transfer::{
        Job, JobEvent, JobKind, JobStatus, OverwritePolicy, spawn_job_with_provider,
    },
};
use std::sync::Condvar;
struct HookScope(Option<ActorHook>);
impl HookScope {
    fn install(hook: ActorHook) -> Self {
        Self(ACTOR_HOOK.with(|slot| slot.replace(Some(hook))))
    }
}
impl Drop for HookScope {
    fn drop(&mut self) {
        ACTOR_HOOK.with(|slot| slot.replace(self.0.take()));
    }
}
#[derive(Default)]
struct Gate {
    entered: AtomicBool,
    released: Mutex<bool>,
    ready: Condvar,
}
impl Gate {
    fn block(&self) {
        self.entered.store(true, Ordering::Release);
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.ready.wait(released).unwrap();
        }
    }
    fn wait(&self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.entered.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.ready.notify_all();
    }
}
struct ReleaseOnDrop(Arc<Gate>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}
struct CancelOnDrop(Arc<JobControl>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.request_cancel();
    }
}
fn owned_app(root: &Path, confirm: bool) -> App {
    let mut app = App::default();
    app.state_path = Some(root.join("state"));
    app.bookmarks_path = Some(root.join("bookmarks"));
    app.window_generation = 7;
    app.panes[0].listing_settled = true;
    if confirm {
        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::write(&source, b"owned source").unwrap();
        std::fs::create_dir(&destination).unwrap();
        app.confirming = Some(Confirm {
            action: ConfirmAction::Copy,
            policy: OverwritePolicy::AutoRename,
            label: "owned source".into(),
            paths: vec![source.to_str().unwrap().into()],
            dest_dir: Some(destination.to_str().unwrap().into()),
        });
    }
    app
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
    // This observes existing checked persistence only, not transfer/editor settlement.
}
#[test]
fn stopped_before_real_confirm_admission_rejects_queued_enter_without_new_job_or_ack() {
    let _fixture = crate::test_support::enter();
    let root = crate::test_support::current().unwrap().directory.clone();
    let gate = Arc::new(Gate::default());
    let held = gate.clone();
    let (tx, rx) = mpsc::channel();
    let _hook = HookScope::install(Arc::new(move |unit, phase, app| {
        if unit == ActorUnit::Command && phase == ActorPhase::BeforeAdmission {
            held.block();
        }
        if unit == ActorUnit::Command && phase == ActorPhase::Canceled {
            let _ = tx.send((app.jobs.len(), app.ack_sequence, app.confirming.is_some()));
        }
    }));
    let _release = ReleaseOnDrop(gate.clone());
    let owned = root.clone();
    let mut runtime = Runtime::with_factory(7, move || owned_app(&owned, true));
    runtime.enqueue(
        Command::Input(Input::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        None,
    );
    runtime.flush();
    gate.wait(); // Actual dequeue has returned; actor has not admitted the command.
    runtime.stop(&[]);
    assert!(runtime.is_stopping());
    gate.release();
    let (jobs, ack, confirm) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(jobs, 0);
    assert_eq!(ack, 0);
    assert!(confirm);
    finish(&runtime);
    assert!(!root.join("destination/source").exists());
    let mut rejected = false;
    while let Some(completion) = runtime.try_completion() {
        if matches!(completion, Completion::Rejected { sequence: 1, .. }) {
            rejected = true;
        }
    }
    assert!(rejected);
}
#[test]
fn admitted_confirm_current_unpublished_control_is_cancelled_and_wakes_same_control_witness() {
    let _fixture = crate::test_support::enter();
    let root = crate::test_support::current().unwrap().directory.clone();
    std::fs::write(root.join("witness-source"), b"witness").unwrap();
    std::fs::create_dir(root.join("witness-destination")).unwrap();
    let gate = Arc::new(Gate::default());
    let held = gate.clone();
    let canceled_hold = Arc::new(Gate::default());
    let actor_canceled_hold = canceled_hold.clone();
    let (control_tx, control_rx) = mpsc::channel();
    let (events, events_rx) = mpsc::channel();
    let (canceled_tx, canceled_rx) = mpsc::channel();
    let paths = root.clone();
    let _hook = HookScope::install(Arc::new(move |unit, phase, app| {
        if unit != ActorUnit::Command {
            return;
        }
        if phase == ActorPhase::Admitted {
            held.block();
        }
        if phase == ActorPhase::Returned && !app.jobs.is_empty() {
            let control = app.jobs[0].control.clone();
            control.set_paused(true);
            let _ = control_tx.send(control.clone());
            // Witness shares the EXACT just-created actor control; no fake cancel flag.
            let witness = Job {
                id: 999,
                kind: JobKind::Copy,
                overwrite: OverwritePolicy::AutoRename,
                paths: vec![paths.join("witness-source").to_str().unwrap().into()],
                dest_dir: paths.join("witness-destination").to_str().unwrap().into(),
                label: "witness".into(),
                total_bytes: None,
                copied_bytes: 0,
                current: String::new(),
                status: JobStatus::Running,
                started_at: Instant::now(),
                control,
            };
            spawn_job_with_provider(
                &witness,
                events.clone(),
                Arc::new(|_, _| {
                    Err(std::io::Error::other(
                        "witness must cancel before publication",
                    ))
                }),
            );
        }
        if phase == ActorPhase::Canceled {
            let _ = canceled_tx.send(app.jobs.len());
            actor_canceled_hold.block(); // No later loop-exit cancellation can rescue this assertion.
        }
    }));
    let _release = ReleaseOnDrop(gate.clone());
    let _release_canceled = ReleaseOnDrop(canceled_hold.clone());
    let owned = root.clone();
    let mut runtime = Runtime::with_factory(7, move || owned_app(&owned, true));
    runtime.enqueue(
        Command::Input(Input::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        None,
    );
    runtime.flush();
    gate.wait(); // CAS has admitted exactly this unit before stop wins.
    runtime.stop(&[]); // Published controls deliberately empty.
    gate.release();
    let control = control_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let _cancel = CancelOnDrop(control);
    assert_eq!(canceled_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
    loop {
        match events_rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            JobEvent::Started { id: 999, .. } => {}
            JobEvent::Cancelled { id: 999 } => break,
            event => panic!("same current-control witness did not cancel: {event:?}"),
        }
    }
    assert!(!root.join("witness-destination/witness-source").exists());
    canceled_hold.release();
    finish(&runtime);
    // Original Confirm was preaccepted and may have physical effects. This test
    // proves its exact current control flag/wakeup, NOT its settlement/receipt.
}
#[test]
fn stop_before_real_browse_command_emits_no_chooser_prompt() {
    let _fixture = crate::test_support::enter();
    let root = crate::test_support::current().unwrap().directory.clone();
    let gate = Arc::new(Gate::default());
    let held = gate.clone();
    let _hook = HookScope::install(Arc::new(move |unit, phase, _| {
        if unit == ActorUnit::Command && phase == ActorPhase::BeforeAdmission {
            held.block();
        }
    }));
    let _release = ReleaseOnDrop(gate.clone());
    let mut runtime = Runtime::with_factory(7, move || owned_app(&root, false));
    runtime.enqueue(
        Command::Browse(crate::platform::chooser::ChooserKind::File),
        None,
    );
    runtime.flush();
    gate.wait();
    runtime.stop(&[]);
    gate.release();
    finish(&runtime);
    assert!(runtime.try_chooser_event().is_none());
}
#[test]
fn normal_core_quit_commits_same_stop_latch_before_another_unit() {
    let _fixture = crate::test_support::enter();
    let root = crate::test_support::current().unwrap().directory.clone();
    let (tx, rx) = mpsc::channel();
    let _hook = HookScope::install(Arc::new(move |unit, phase, app| {
        if unit == ActorUnit::Command && phase == ActorPhase::Canceled {
            let _ = tx.send(app.running);
        }
    }));
    let mut runtime = Runtime::with_factory(7, move || owned_app(&root, false));
    runtime.enqueue(
        Command::Input(Input::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        ))),
        None,
    );
    runtime.flush();
    assert!(!rx.recv_timeout(Duration::from_secs(5)).unwrap());
    assert!(runtime.is_stopping());
    finish(&runtime);
    assert!(!runtime.effect_guard(7).is_current());
}

#[test]
fn strong_admission_never_resets_monotone_stop_even_with_empty_controls() {
    let _fixture = crate::test_support::enter();
    let root = crate::test_support::current().unwrap().directory.clone();
    let mut app = owned_app(&root, false);
    let stop = Arc::new(AtomicBool::new(false));
    let gate = ActorAdmission {
        stop: stop.clone(),
        hook: None,
    };
    assert!(gate.admit(&mut app, ActorUnit::Command));
    stop.store(true, Ordering::Release);
    assert!(!gate.finish(&mut app, ActorUnit::Command));
    for unit in [
        ActorUnit::Attachment,
        ActorUnit::Completion,
        ActorUnit::Command,
        ActorUnit::Tick,
        ActorUnit::ChooserDrain,
        ActorUnit::Host,
        ActorUnit::Publication,
    ] {
        assert!(!gate.admit(&mut app, unit));
        assert!(stop.load(Ordering::Acquire));
    }
}

#[path = "runtime_stop_lifetime_tests.rs"]
mod lifetime_tests;
