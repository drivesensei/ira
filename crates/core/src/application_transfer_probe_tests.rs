use super::*;

// Serialize these synthetic blockers, while still allowing real peer probes to
// compete for the production lane. Admission is driven by actual host ticks.
static FIXTURE_LOCK: Mutex<()> = Mutex::new(());
fn fixture_lock() -> std::sync::MutexGuard<'static, ()> {
    FIXTURE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
fn drive_until_callback<T>(
    app: &mut App,
    rx: &mpsc::Receiver<T>,
    budget: Duration,
) -> Result<T, mpsc::TryRecvError> {
    let deadline = Instant::now() + budget;
    loop {
        app.tick();
        match rx.try_recv() {
            Ok(value) => return Ok(value),
            Err(mpsc::TryRecvError::Disconnected) => return Err(mpsc::TryRecvError::Disconnected),
            Err(mpsc::TryRecvError::Empty) if Instant::now() >= deadline => {
                return Err(mpsc::TryRecvError::Empty)
            }
            Err(mpsc::TryRecvError::Empty) => thread::yield_now(),
        }
    }
}
fn drive_until_spawn_attempt(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        app.tick();
        if app.transfer_probe_finished.is_some() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "owned spawn failure was never admitted"
        );
        thread::yield_now();
    }
}

fn running_app() -> App {
    let mut app = App::default();
    app.jobs.push(Job {
        id: 1,
        kind: JobKind::Copy,
        overwrite: OverwritePolicy::SkipExisting,
        paths: vec!["fixture-source/item.txt".into()],
        dest_dir: "fixture-destination".into(),
        label: "item.txt".into(),
        total_bytes: Some(1),
        copied_bytes: 0,
        current: String::new(),
        status: JobStatus::Running,
        started_at: Instant::now(),
        control: JobControl::new(),
    });
    app.transfer_dest = Some(TransferDestSync {
        dest_dir: "fixture-destination".into(),
        reveal_path: "fixture-destination/item.txt".into(),
        last_refresh: app.now() - Duration::from_secs(2),
    });
    app
}

#[test]
fn core_close_and_recreate_do_not_accumulate_blocked_workers() {
    let _fixture = fixture_lock();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let release_guard = ReleaseGate(Arc::clone(&gate));
    let worker_gate = gate.clone();
    let probe: TransferProbeTest = Arc::new(move |_| {
        entered_tx.send(()).unwrap();
        let (lock, changed) = &*worker_gate;
        let mut released = lock.lock().unwrap();
        while !*released {
            released = changed.wait(released).unwrap();
        }
        finished_tx.send(()).unwrap();
        false
    });
    let mut count = 0;
    for index in 0..6 {
        let mut app = running_app();
        app.transfer_probe_test = Some(probe.clone());
        let entered = if index == 0 {
            drive_until_callback(&mut app, &entered_rx, Duration::from_millis(100)).is_ok()
        } else {
            app.tick();
            entered_rx.recv_timeout(Duration::from_millis(100)).is_ok()
        };
        count += usize::from(entered);
        app.cancel_pending_work();
        drop(app);
    }
    release_guard.release();
    for _ in 0..count {
        finished_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    assert_eq!(
        count, 1,
        "{count} physical workers survive core close/recreate"
    );
}

struct ReleaseGate(Arc<(Mutex<bool>, std::sync::Condvar)>);
impl ReleaseGate {
    fn release(&self) {
        let (lock, changed) = &*self.0;
        *lock.lock().unwrap() = true;
        changed.notify_all();
    }
}
impl Drop for ReleaseGate {
    fn drop(&mut self) {
        self.release();
    }
}

fn wait_worker_finished(app: &App) {
    let deadline = Instant::now() + Duration::from_secs(2);
    // tick may already have consumed the completion signal and its reply.
    while app
        .transfer_probe_finished
        .as_ref()
        .is_some_and(|done| !done.load(std::sync::atomic::Ordering::Acquire))
        || app.transfer_probe_finished.is_none() && app.transfer_probe_pending.is_some()
    {
        assert!(Instant::now() < deadline, "owned worker did not exit");
        thread::yield_now();
    }
}

#[test]
fn core_closed_paused_latest_request_retries_after_physical_worker_exits() {
    let _fixture = fixture_lock();
    let mut app = running_app();
    let (entered_tx, entered_rx) = mpsc::channel();
    let gate = ReleaseGate(Arc::new((Mutex::new(false), std::sync::Condvar::new())));
    let worker_gate = Arc::clone(&gate.0);
    app.transfer_probe_test = Some(Arc::new(move |path| {
        entered_tx.send(path.to_owned()).unwrap();
        let (lock, changed) = &*worker_gate;
        let released = lock.lock().unwrap();
        let _ = changed
            .wait_timeout_while(released, Duration::from_secs(2), |released| !*released)
            .unwrap();
        false
    }));
    let first = drive_until_callback(&mut app, &entered_rx, Duration::from_secs(1));
    app.jobs[0].status = JobStatus::Paused;
    app.transfer_dest.as_mut().unwrap().dest_dir = "latest-destination".into();
    app.transfer_dest.as_mut().unwrap().reveal_path = "latest-destination/item.txt".into();
    let latest = app.transfer_dest.clone();
    for _ in 0..6 {
        app.cancel_pending_work();
        app.transfer_dest = latest.clone();
        app.transfer_probe_last_attempt = None;
        app.tick();
    }
    let denied_without_consuming_attempt =
        app.transfer_probe_pending.is_none() && app.transfer_probe_last_attempt.is_none();
    let extra = entered_rx.try_recv();
    gate.release();
    wait_worker_finished(&app);
    let deadline = Instant::now() + Duration::from_secs(1);
    let latest = loop {
        app.tick();
        if let Ok(path) = entered_rx.try_recv() {
            break Some(path);
        }
        if Instant::now() >= deadline {
            break None;
        }
        thread::yield_now();
    };
    wait_worker_finished(&app);
    assert_eq!(first.unwrap(), "fixture-destination");
    assert!(
        extra.is_err(),
        "another callback entered before physical exit"
    );
    assert!(denied_without_consuming_attempt);
    assert_eq!(latest.as_deref(), Some("latest-destination"));
    assert_eq!(
        app.transfer_dest.as_ref().unwrap().dest_dir,
        "latest-destination"
    );
}

#[test]
fn worker_panic_without_reply_releases_slot_and_unwedges_pending() {
    let _fixture = fixture_lock();
    let mut app = running_app();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = Arc::clone(&count);
    let (started_tx, started_rx) = mpsc::channel();
    app.transfer_probe_test = Some(Arc::new(move |_| {
        started_tx.send(()).unwrap();
        if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            panic!("fixture metadata callback panic");
        }
        false
    }));
    drive_until_callback(&mut app, &started_rx, Duration::from_secs(1)).unwrap();
    wait_worker_finished(&app);
    app.tick();
    assert!(app.transfer_probe_pending.is_none());
    assert!(app.transfer_probe_finished.is_none());
    app.transfer_probe_last_attempt = None;
    drive_until_callback(&mut app, &started_rx, Duration::from_secs(1)).unwrap();
    wait_worker_finished(&app);
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn spawn_failure_releases_admission_and_retries_without_fake_reply() {
    let _fixture = fixture_lock();
    let mut app = running_app();
    app.transfer_probe_spawn_error = true;
    drive_until_spawn_attempt(&mut app);
    assert!(app.transfer_probe_pending.is_none());
    wait_worker_finished(&app);
    app.transfer_probe_spawn_error = false;
    app.transfer_probe_last_attempt = None;
    let (called_tx, called_rx) = mpsc::channel();
    app.transfer_probe_test = Some(Arc::new(move |_| {
        called_tx.send(()).unwrap();
        false
    }));
    drive_until_callback(&mut app, &called_rx, Duration::from_secs(1)).unwrap();
    wait_worker_finished(&app);
}

#[test]
fn core_real_transfer_replacement_keeps_one_physical_probe() {
    let _fixture = fixture_lock();
    let base = std::env::temp_dir().join(format!("ira-t041-core-replace-{}", std::process::id()));
    std::fs::create_dir(&base).unwrap();
    let mut app = running_app();
    app.next_job_id = 100;
    let (entered_tx, entered_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let release_guard = ReleaseGate(Arc::clone(&gate));
    let worker_gate = Arc::clone(&gate);
    app.transfer_probe_test = Some(Arc::new(move |_| {
        entered_tx.send(()).unwrap();
        let (lock, changed) = &*worker_gate;
        let mut released = lock.lock().unwrap();
        while !*released {
            released = changed.wait(released).unwrap();
        }
        finished_tx.send(()).unwrap();
        false
    }));
    let mut count = usize::from(
        drive_until_callback(&mut app, &entered_rx, Duration::from_millis(100)).is_ok(),
    );
    for i in 0..6 {
        let dest = base.join(format!("dest-{i}"));
        std::fs::create_dir(&dest).unwrap();
        // A deliberately missing owned source fails without writing user data.
        app.spawn_transfer_jobs(
            JobKind::Copy,
            vec![base
                .join(format!("missing-{i}"))
                .to_string_lossy()
                .into_owned()],
            dest.to_string_lossy().into_owned(),
            OverwritePolicy::SkipExisting,
        );
        app.clock_override = Some(app.now() + Duration::from_secs(2));
        app.tick();
        if entered_rx.recv_timeout(Duration::from_millis(100)).is_ok() {
            count += 1;
        }
    }
    // Release all metadata calls before any assertions/fixture cleanup.
    release_guard.release();
    for _ in 0..count {
        finished_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while app
        .jobs
        .iter()
        .skip(1)
        .any(|job| matches!(job.status, JobStatus::Running | JobStatus::Paused))
        && Instant::now() < deadline
    {
        app.tick();
        thread::yield_now();
    }
    let jobs_finished = app
        .jobs
        .iter()
        .skip(1)
        .all(|job| matches!(job.status, JobStatus::Failed(_)));
    app.cancel_pending_work();
    drop(app);
    std::fs::remove_dir_all(&base).unwrap();
    assert!(
        jobs_finished,
        "all owned missing-source workers must finish before cleanup"
    );
    assert_eq!(
        count, 1,
        "{count} simultaneously blocked probes from actual core transfer replacement"
    );
}
