use super::*;
use std::time::Duration;

const READY: Duration = Duration::from_secs(5);
const EARLY: Duration = Duration::from_millis(100);
const PROGRESS: Duration = Duration::from_secs(1);

struct Owner {
    control: Arc<JobControl>,
    release: Option<mpsc::Sender<()>>,
    threads: Vec<thread::JoinHandle<()>>,
    worker_receipt: Option<mpsc::Receiver<u64>>,
    worker_acknowledged: bool,
}
impl Owner {
    fn paused() -> (Self, mpsc::Receiver<()>) {
        let control = JobControl::new();
        control.set_paused(true);
        let (entered, observed) = mpsc::channel();
        let (release, released) = mpsc::channel();
        *control.before_wait.lock() = Some(GateWaitTest { entered, released });
        (
            Self {
                control,
                release: Some(release),
                threads: Vec::new(),
                worker_receipt: None,
                worker_acknowledged: false,
            },
            observed,
        )
    }
    fn release_hook(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
    fn cancel_thread(&mut self) -> (mpsc::Receiver<()>, mpsc::Receiver<()>) {
        let (enter, entered) = mpsc::channel();
        let (done, returned) = mpsc::channel();
        let control = self.control.clone();
        self.threads.push(thread::spawn(move || {
            let _ = enter.send(());
            control.request_cancel();
            let _ = done.send(());
        }));
        (entered, returned)
    }
    fn cleanup(&mut self) {
        self.release_hook();
        // Cleanup-only notification AFTER the tested progress result was captured.
        // Never count this unpause as a successful cancellation wake.
        self.control.set_paused(false);
        for worker in self.threads.drain(..) {
            let _ = worker.join();
        }
        if !self.worker_acknowledged {
            if let Some(receipt) = &self.worker_receipt {
                // Cleanup-only outcome, never counted as normal cancellation success.
                self.worker_acknowledged = receipt.recv_timeout(READY).is_ok();
            }
        }
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[test]
fn cancel_cannot_return_in_actual_gate_check_registration_gap_and_wakes_without_second_notify() {
    let (mut owner, observed) = Owner::paused();
    let (done, result) = mpsc::channel();
    let control = owner.control.clone();
    owner.threads.push(thread::spawn(move || {
        let _ = done.send(matches!(control.gate(), Err(JobError::Cancelled)));
    }));
    observed.recv_timeout(READY).unwrap(); // actual inner cancel=false, pause mutex HELD
    let (entered, returned) = owner.cancel_thread();
    entered.recv_timeout(READY).unwrap();
    let early = returned.recv_timeout(EARLY).is_ok();
    owner.release_hook(); // registration happens before the predicate mutex unlocks
    let cancel_returned = early || returned.recv_timeout(READY).is_ok();
    let actual_result = result.recv_timeout(PROGRESS).ok();
    let pause_preserved = *owner.control.pause.lock();
    owner.cleanup(); // records remain distinct from cancellation progress witness
    assert!(
        !early,
        "request_cancel returned while real gate still held predicate lock; lost-wake window"
    );
    assert!(
        cancel_returned,
        "cancel did not complete after actual wait registration"
    );
    assert_eq!(
        actual_result,
        Some(true),
        "real gate must yield Cancelled without unpause or second notify"
    );
    assert!(pause_preserved);
}

fn actual_terminal_event(events: &mpsc::Receiver<JobEvent>) -> Option<JobEvent> {
    let deadline = Instant::now() + PROGRESS;
    loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        let event = events.recv_timeout(remaining).ok()?;
        if matches!(
            event,
            JobEvent::Done { .. }
                | JobEvent::Cancelled { .. }
                | JobEvent::Failed { .. }
                | JobEvent::DeleteDone { .. }
        ) {
            return Some(event);
        }
        // Real transfer pre-scan emits Started before its first paused gate.
        // A progress event is neither a terminal event nor a settlement proof.
    }
}

fn actual_worker_race(delete: bool) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "ira-t078-root-cancel-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source");
    let dest = root.join("destination");
    std::fs::write(&source, b"owned cancelled bytes").unwrap();
    std::fs::create_dir(&dest).unwrap();
    let (mut owner, observed) = Owner::paused();
    let (events, outcomes) = mpsc::channel();
    let (settlement, receipt) = terminal_receipt(99);
    owner.worker_receipt = Some(receipt);
    if delete {
        spawn_delete_tracked(
            vec![source.to_str().unwrap().into()],
            events,
            owner.control.clone(),
            settlement,
            None,
        );
    } else {
        let job = Job {
            id: 99,
            kind: JobKind::Move,
            overwrite: OverwritePolicy::AutoRename,
            paths: vec![source.to_str().unwrap().into()],
            dest_dir: dest.to_str().unwrap().into(),
            label: "owned".into(),
            total_bytes: None,
            copied_bytes: 0,
            current: String::new(),
            status: JobStatus::Running,
            started_at: Instant::now(),
            control: owner.control.clone(),
        };
        spawn_job_tracked(&job, events, settlement, None);
    }
    observed.recv_timeout(READY).unwrap();
    let (entered, returned) = owner.cancel_thread();
    entered.recv_timeout(READY).unwrap();
    let early = returned.recv_timeout(EARLY).is_ok();
    owner.release_hook();
    let cancel_returned = early || returned.recv_timeout(READY).is_ok();
    let authentic = owner
        .worker_receipt
        .as_ref()
        .unwrap()
        .recv_timeout(PROGRESS)
        .ok();
    owner.worker_acknowledged = authentic.is_some();
    let terminal = actual_terminal_event(&outcomes);
    let pause_preserved = *owner.control.pause.lock();
    owner.cleanup();
    assert!(
        !early,
        "actual worker predicate gap allowed early cancel return"
    );
    assert!(cancel_returned);
    assert!(
        matches!(authentic, Some(99)),
        "actual root worker must acknowledge exact identity before cleanup-only release"
    );
    if delete {
        assert!(matches!(
            terminal,
            Some(JobEvent::DeleteDone {
                cancelled: true,
                ..
            })
        ));
    } else {
        assert!(matches!(terminal, Some(JobEvent::Cancelled { id: 99 })));
    }
    assert!(pause_preserved);
    assert_eq!(std::fs::read(&source).unwrap(), b"owned cancelled bytes");
    assert!(!dest.join("source").exists());
    // All fixture paths retained; receipts do not claim physical thread exit.
}
#[test]
fn actual_paused_transfer_cancel_wakes_and_sends_terminal_ack_without_unpause() {
    actual_worker_race(false);
}
#[test]
fn actual_paused_delete_cancel_wakes_and_sends_terminal_ack_without_unpause() {
    actual_worker_race(true);
}

#[test]
fn cancel_before_gate_is_irreversible_idempotent_and_preserves_pause_true() {
    let control = JobControl::new();
    control.set_paused(true);
    control.request_cancel();
    control.request_cancel();
    assert!(*control.pause.lock());
    assert!(control.cancel.load(Ordering::Relaxed));
    assert!(matches!(control.gate(), Err(JobError::Cancelled)));
    control.set_paused(false);
    assert!(matches!(control.gate(), Err(JobError::Cancelled)));
}
#[test]
fn already_registered_paused_gate_is_cancelled_without_changing_pause_flag() {
    let (mut owner, observed) = Owner::paused();
    let (done, result) = mpsc::channel();
    let control = owner.control.clone();
    owner.threads.push(thread::spawn(move || {
        let _ = done.send(matches!(control.gate(), Err(JobError::Cancelled)));
    }));
    observed.recv_timeout(READY).unwrap();
    owner.release_hook();
    // This acquisition is possible only after actual wait registered/unlocked.
    drop(owner.control.pause.lock());
    owner.control.request_cancel();
    let actual = result.recv_timeout(PROGRESS).ok();
    let preserved = *owner.control.pause.lock();
    owner.cleanup();
    assert_eq!(actual, Some(true));
    assert!(preserved);
}
#[test]
fn set_paused_false_still_releases_actual_registered_gate_without_cancellation() {
    let (mut owner, observed) = Owner::paused();
    let (done, result) = mpsc::channel();
    let control = owner.control.clone();
    owner.threads.push(thread::spawn(move || {
        let _ = done.send(control.gate().is_ok());
    }));
    observed.recv_timeout(READY).unwrap();
    owner.release_hook();
    drop(owner.control.pause.lock());
    owner.control.set_paused(false);
    let actual = result.recv_timeout(PROGRESS).ok();
    owner.cleanup();
    assert_eq!(actual, Some(true));
    assert!(!owner.control.cancel.load(Ordering::Relaxed));
}
