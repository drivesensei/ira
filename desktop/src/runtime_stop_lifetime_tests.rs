use super::*;
#[test]
fn stop_wins_initial_attachment_tick_and_ready_drive_completion_boundaries() {
    let _fixture = crate::test_support::enter();
    let root = crate::test_support::current().unwrap().directory.clone();
    for unit in [
        ActorUnit::Attachment,
        ActorUnit::Tick,
        ActorUnit::Completion,
    ] {
        let owned = root.join(format!("unit-{unit:?}"));
        std::fs::create_dir(&owned).unwrap();
        let gate = Arc::new(Gate::default());
        let held = gate.clone();
        let once = AtomicBool::new(false);
        let (tx, rx) = mpsc::channel();
        let _hook = HookScope::install(Arc::new(move |observed, phase, app| {
            if observed != unit {
                return;
            }
            if phase == ActorPhase::BeforeAdmission && !once.swap(true, Ordering::AcqRel) {
                held.block();
            }
            if phase == ActorPhase::Canceled {
                let _ = tx.send((app.ack_sequence, app.jobs.len(), app.drives.clone()));
            }
        }));
        let _release = ReleaseOnDrop(gate.clone());
        let runtime = Runtime::with_drive_probe(7, move || owned_app(&owned, false), || Ok(vec![]));
        gate.wait();
        runtime.stop(&[]);
        gate.release();
        let (ack, jobs, drives) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(ack, 0);
        assert_eq!(jobs, 0);
        assert!(drives.is_none());
        finish(&runtime);
        assert!(!runtime.effect_guard(7).is_current());
    }
}
#[test]
fn stop_during_factory_delay_preserves_original_owner_and_rejects_queued_confirmation() {
    let _fixture = crate::test_support::enter();
    let root = crate::test_support::current().unwrap().directory.clone();
    let gate = Arc::new(Gate::default());
    let held = gate.clone();
    let calls = Arc::new(AtomicU64::new(0));
    let factory_calls = calls.clone();
    let (tx, rx) = mpsc::channel();
    let _hook = HookScope::install(Arc::new(move |unit, phase, app| {
        if unit == ActorUnit::Attachment && phase == ActorPhase::Canceled {
            let _ = tx.send((app.confirming.is_some(), app.jobs.len(), app.ack_sequence));
        }
    }));
    let _release = ReleaseOnDrop(gate.clone());
    let owned = root.clone();
    let mut runtime = Runtime::with_factory(7, move || {
        let app = owned_app(&owned, true); // Explicit paths BEFORE stop can observe it.
        factory_calls.fetch_add(1, Ordering::AcqRel);
        held.block();
        app // The same original factory owner, never a fallback App/default replacement.
    });
    gate.wait();
    runtime.enqueue(
        Command::Input(Input::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        ))),
        None,
    );
    runtime.flush();
    runtime.stop(&[]);
    let reopened = runtime.attach(8);
    reopened.stop(&[]);
    gate.release();
    let (confirm, jobs, ack) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(confirm);
    assert_eq!(jobs, 0);
    assert_eq!(ack, 0);
    finish(&runtime);
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert!(reopened.is_stopping());
    assert!(!reopened.effect_guard(8).is_current());
    assert!(!root.join("destination/source").exists());
    // Does not stand in for delayed asynchronous startup/checked-Result fixtures.
}
