use super::*;
use std::fs;
fn fixture() -> PathBuf {
    let path = crate::services::persistence::test_path("worker").join(format!(
        "{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}
#[test]
fn original_checked_receiver_survives_timeout_and_delivers_late_write_ack() {
    let directory = fixture();
    let path = directory.join("state");
    let shared = Arc::new(Mutex::new(Ledger::default()));
    let held = shared.lock().unwrap();
    let (tx, rx) = mpsc::channel();
    let worker_shared = shared.clone();
    let worker = thread::spawn(move || worker_with_ledger(rx, worker_shared));
    tx.send(PersistenceRequest::State(
        Some(path.clone()),
        SessionState::default(),
    ))
    .unwrap();
    let (reply, original) = mpsc::channel();
    tx.send(PersistenceRequest::CheckedBarrier(reply)).unwrap();
    assert!(matches!(
        original.recv_timeout(Duration::from_millis(20)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert!(!path.exists());
    drop(held);
    assert_eq!(
        original
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap()
            .epoch,
        1
    );
    assert!(path.exists());
    drop(tx);
    worker.join().unwrap();
    fs::remove_dir_all(directory).unwrap();
}
#[test]
fn retry_admission_is_bounded_across_receipts_and_replays_only_sealed_payload() {
    let directory = fixture();
    let parent = directory.join("blocked");
    fs::write(&parent, b"block").unwrap();
    let path = parent.join("state");
    let shared = Arc::new(Mutex::new(Ledger::default()));
    let mut ledger = shared.lock().unwrap();
    let mut state = SessionState::default();
    state.split = true;
    ledger.write(Payload::State(Some(path.clone()), state));
    let first = ledger.outcome(&shared).unwrap_err();
    let second = ledger.outcome(&shared).unwrap_err();
    fs::remove_file(&parent).unwrap();
    fs::create_dir(&parent).unwrap();
    let original = first.retry.retry();
    assert!(matches!(
        original.recv_timeout(Duration::from_millis(20)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    let busy = second
        .retry
        .retry()
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap_err();
    assert_eq!(busy.errors[0].stage, "retry");
    drop(ledger);
    original
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert!(fs::read(&path).unwrap().starts_with(b"split=1\n"));
    fs::remove_dir_all(directory).unwrap();
}
