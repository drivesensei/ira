use super::*;

#[test]
fn terminal_events_do_not_accumulate_blocked_metadata_workers() {
    let mut app = running_app();
    let (entered_tx, entered_rx) = mpsc::channel();
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let worker_gate = gate.clone();
    let (finished_tx, finished_rx) = mpsc::channel();
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
    let mut count = 0;
    for id in 2..8 {
        let mut other = running_app().jobs.pop().unwrap();
        other.id = id;
        app.jobs.push(other);
        app.job_tx
            .send(JobEvent::Failed {
                id,
                error: "fixture".into(),
            })
            .unwrap();
        app.tick();
        if entered_rx.recv_timeout(Duration::from_millis(100)).is_ok() {
            count += 1;
        }
    }
    let (lock, changed) = &*gate;
    *lock.lock().unwrap() = true;
    changed.notify_all();
    for _ in 0..count {
        finished_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    assert!(count <= 1, "{count} simultaneous blocked metadata workers after unrelated terminal events; pending invalidation lost physical occupancy");
}

#[test]
fn root_drop_and_recreate_do_not_accumulate_blocked_workers() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
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
    for _ in 0..6 {
        let mut app = running_app();
        app.transfer_probe_test = Some(probe.clone());
        app.tick();
        if entered_rx.recv_timeout(Duration::from_millis(100)).is_ok() {
            count += 1;
        }
        drop(app);
    }
    let (lock, changed) = &*gate;
    *lock.lock().unwrap() = true;
    changed.notify_all();
    for _ in 0..count {
        finished_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    assert!(
        count <= 1,
        "{count} physical workers survive root drop/recreate"
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
fn paused_latest_request_retries_after_obsolete_physical_worker_exits() {
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
    app.tick();
    let first = entered_rx.recv_timeout(Duration::from_secs(1));
    app.jobs[0].status = JobStatus::Paused;
    app.transfer_dest.as_mut().unwrap().dest_dir = "latest-destination".into();
    app.transfer_dest.as_mut().unwrap().reveal_path = "latest-destination/item.txt".into();
    for id in 2..8 {
        app.job_tx
            .send(JobEvent::Failed {
                id,
                error: "unrelated".into(),
            })
            .unwrap();
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
    let mut app = running_app();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = Arc::clone(&count);
    app.transfer_probe_test = Some(Arc::new(move |_| {
        if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            panic!("fixture metadata callback panic");
        }
        false
    }));
    app.tick();
    wait_worker_finished(&app);
    app.tick();
    assert!(app.transfer_probe_pending.is_none());
    assert!(app.transfer_probe_finished.is_none());
    app.transfer_probe_last_attempt = None;
    app.tick();
    wait_worker_finished(&app);
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn spawn_failure_releases_admission_and_retries_without_fake_reply() {
    let mut app = running_app();
    app.transfer_probe_spawn_error = true;
    app.tick();
    assert!(app.transfer_probe_pending.is_none());
    wait_worker_finished(&app);
    app.transfer_probe_spawn_error = false;
    app.transfer_probe_last_attempt = None;
    let (called_tx, called_rx) = mpsc::channel();
    app.transfer_probe_test = Some(Arc::new(move |_| {
        called_tx.send(()).unwrap();
        false
    }));
    app.tick();
    called_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    wait_worker_finished(&app);
}
