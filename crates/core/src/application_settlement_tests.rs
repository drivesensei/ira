use super::*;
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
        let until = Instant::now() + Duration::from_secs(5);
        while !self.entered.load(Ordering::Acquire) {
            assert!(Instant::now() < until);
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
fn owned_app() -> (App, PathBuf) {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "ira-t061-core-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    let mut app = App::default();
    app.state_path = Some(root.join("state"));
    app.bookmarks_path = Some(root.join("bookmarks"));
    (app, root) // Retain synthetic directory; never clean under physical workers.
}
fn await_settled(app: &mut App, seal: &ShutdownWorkSeal) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        match app.poll_shutdown_settlement(seal, 1) {
            WorkSettlement::Settled(_) => break,
            WorkSettlement::Error(e) => panic!("{e}"),
            WorkSettlement::Pending => assert!(Instant::now() < until),
        }
        thread::yield_now();
    }
}
#[test]
fn operation_after_epoch_check_and_queued_stale_request_require_authentic_watermark() {
    let (mut app, root) = owned_app();
    let gate = Arc::new(Gate::default());
    let hold = gate.clone();
    let _release = Release(gate.clone());
    app.operation_test = Some(Arc::new(move || hold.block()));
    app.goto_prompt = Some(root.join("accepted").to_str().unwrap().into());
    app.submit_operation(BlockingOperation::Goto);
    gate.wait();
    app.goto_prompt = Some(root.join("queued").to_str().unwrap().into());
    app.submit_operation(BlockingOperation::Goto);
    let seal = app.begin_shutdown_settlement();
    assert_eq!(app.pending_operations, 0);
    assert!(matches!(
        app.poll_shutdown_settlement(&seal, 1),
        WorkSettlement::Pending
    ));
    gate.release();
    await_settled(&mut app, &seal);
    assert!(root.join("accepted").is_dir());
    assert!(!root.join("queued").exists());
    assert!(
        app.panes[0].folder.is_none(),
        "stale operation business results must not apply"
    );
    app.goto_prompt = Some(root.join("after-seal").to_str().unwrap().into());
    app.submit_operation(BlockingOperation::Goto);
    assert_eq!(app.operation_issued, seal.operation_cutoff);
}
#[test]
fn original_startup_payload_applies_without_listing_tick_or_deferred_input_replay() {
    let (mut app, _root) = owned_app();
    app.initializing = true;
    app.startup_inputs.push_back(crate::input::Input::Action(
        crate::input::Command::ToggleHidden,
    ));
    let seal = app.begin_shutdown_settlement();
    assert!(matches!(
        app.poll_shutdown_settlement(&seal, 1),
        WorkSettlement::Pending
    ));
    app.startup_tx
        .send((
            SessionState {
                show_hidden: true,
                split: true,
                ..SessionState::default()
            },
            vec![],
        ))
        .unwrap();
    await_settled(&mut app, &seal);
    assert!(app.show_hidden);
    assert!(app.split);
    assert!(app.startup_inputs.is_empty());
    assert_eq!(app.panes[0].listing_generation, 1);
    assert!(app.take_host_requests().is_empty());
}
#[test]
fn different_app_seal_and_lost_receipt_never_settle() {
    let (mut app, _) = owned_app();
    let (mut other, _) = owned_app();
    let foreign = other.begin_shutdown_settlement();
    assert!(matches!(
        app.poll_shutdown_settlement(&foreign, 1),
        WorkSettlement::Error(_)
    ));
    let (tx, rx) = mpsc::channel();
    app.work_receipts.push_back(ActiveWork {
        receipt: rx,
        control: JobControl::new(),
    });
    drop(tx);
    let seal = app.begin_shutdown_settlement();
    assert!(matches!(
        app.poll_shutdown_settlement(&seal, 1),
        WorkSettlement::Error(_)
    ));
}

#[test]
fn actual_delete_batch_survives_hidden_replaced_dialog_and_disconnected_business_results() {
    let (mut app, root) = owned_app();
    let a = root.join("a");
    let b = root.join("b");
    std::fs::write(&a, b"a").unwrap();
    std::fs::write(&b, b"b").unwrap();
    let (_unused_tx, disconnected_rx) = mpsc::channel();
    drop(std::mem::replace(&mut app.job_rx, disconnected_rx));
    for path in [&a, &b] {
        app.confirming = Some(Confirm {
            action: ConfirmAction::Delete,
            policy: OverwritePolicy::AutoRename,
            label: "owned".into(),
            paths: vec![path.to_str().unwrap().into()],
            dest_dir: None,
        });
        app.confirm_delete();
        app.deletion = None;
    }
    let seal = app.begin_shutdown_settlement();
    await_settled(&mut app, &seal);
    assert_eq!(app.work_receipts.len(), 0);
    // Cancellation may precede either deletion; settled means attempts have ended,
    // not that cancelled business operations succeeded.
}
#[test]
fn operation_sequence_overflow_is_error_without_issuing_a_mutation() {
    let (mut app, root) = owned_app();
    app.operation_issued = u64::MAX;
    app.goto_prompt = Some(root.join("overflow").to_str().unwrap().into());
    app.submit_operation(BlockingOperation::Goto);
    let seal = app.begin_shutdown_settlement();
    assert!(matches!(
        app.poll_shutdown_settlement(&seal, 1),
        WorkSettlement::Error(_)
    ));
    assert!(!root.join("overflow").exists());
}
#[test]
fn fair_bounded_active_receipt_poll_does_not_wait_on_first_blocked_worker() {
    let (mut app, _) = owned_app();
    let (held, pending) = mpsc::channel();
    let (done, ready) = mpsc::channel();
    done.send(WorkOutcome::Done).unwrap();
    app.work_receipts.push_back(ActiveWork {
        receipt: pending,
        control: JobControl::new(),
    });
    app.work_receipts.push_back(ActiveWork {
        receipt: ready,
        control: JobControl::new(),
    });
    let seal = app.begin_shutdown_settlement();
    assert!(matches!(
        app.poll_shutdown_settlement(&seal, 1),
        WorkSettlement::Pending
    ));
    assert_eq!(app.work_receipts.len(), 2);
    assert!(matches!(
        app.poll_shutdown_settlement(&seal, 1),
        WorkSettlement::Pending
    ));
    assert_eq!(app.work_receipts.len(), 1);
    held.send(WorkOutcome::Cancelled).unwrap();
    await_settled(&mut app, &seal);
    // Accounting-only supplemental fixture; actual workers are primary above.
}

#[test]
fn actual_transfer_provider_panic_loses_ack_and_never_settles() {
    let (mut app, root) = owned_app();
    let source = root.join("source");
    let dest = root.join("dest");
    std::fs::write(&source, b"owned").unwrap();
    std::fs::create_dir(&dest).unwrap();
    app.set_transfer_provider(Arc::new(|_, _| panic!("owned worker panic fixture")));
    app.spawn_transfer_jobs(
        JobKind::Move,
        vec![source.to_str().unwrap().into()],
        dest.to_str().unwrap().into(),
        OverwritePolicy::AutoRename,
    );
    // Wait for sender loss through the real worker, avoiding a cancellation race
    // that could skip the provider before it is entered.
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.work_error.is_none() {
        app.poll_work_receipts(1);
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    let seal = app.begin_shutdown_settlement();
    assert!(matches!(
        app.poll_shutdown_settlement(&seal, 1),
        WorkSettlement::Error(_)
    ));
    assert!(source.exists());
}
#[test]
fn actual_failed_transfer_is_terminal_without_replaying_business_work() {
    let (mut app, root) = owned_app();
    let source = root.join("source");
    let dest = root.join("dest");
    std::fs::write(&source, b"owned").unwrap();
    std::fs::create_dir(&dest).unwrap();
    app.set_transfer_provider(Arc::new(|_, _| {
        Err(std::io::Error::other("owned provider failure"))
    }));
    app.spawn_transfer_jobs(
        JobKind::Move,
        vec![source.to_str().unwrap().into()],
        dest.to_str().unwrap().into(),
        OverwritePolicy::AutoRename,
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.work_receipts.is_empty() {
        app.poll_work_receipts(1);
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(app.work_diagnostic.is_some());
    let seal = app.begin_shutdown_settlement();
    await_settled(&mut app, &seal);
    assert!(source.exists());
    assert!(!dest.join("source").exists());
}
