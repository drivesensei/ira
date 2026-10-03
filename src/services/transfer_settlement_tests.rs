use super::*;
use std::time::Duration;

fn job(paths: Vec<String>, dest: String) -> Job {
    Job {
        id: 8,
        kind: JobKind::Copy,
        overwrite: OverwritePolicy::AutoRename,
        paths,
        dest_dir: dest,
        label: "owned".into(),
        total_bytes: None,
        copied_bytes: 0,
        current: String::new(),
        status: JobStatus::Running,
        started_at: Instant::now(),
        control: JobControl::new(),
    }
}

#[test]
fn cancelled_paused_transfer_receipt_follows_terminal_event_even_if_event_receiver_lost() {
    let root = owned_fixture("pause");
    fs::write(root.join("source"), b"owned source").unwrap();
    let j = job(
        vec![root.join("source").to_string_lossy().into_owned()],
        root.join("dest").to_string_lossy().into_owned(),
    );
    j.control.set_paused(true);
    let (tx, rx) = mpsc::channel();
    // Started is sent by the real batch before it blocks in gate().
    let (terminal, receipt) = terminal_receipt(41);
    spawn_job_tracked(&j, tx, terminal, None);
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        JobEvent::Started { .. }
    ));
    assert!(matches!(
        receipt.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    drop(rx);
    j.control.request_cancel();
    assert_eq!(receipt.recv_timeout(Duration::from_secs(5)).unwrap(), 41);
    assert_eq!(fs::read(root.join("source")).unwrap(), b"owned source");
    assert_eq!(fs::read_dir(root.join("dest")).unwrap().count(), 0);
    fs::remove_dir_all(root).unwrap();
}

fn owned_fixture(label: &str) -> std::path::PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "ira-t066-service-{label}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("dest")).unwrap();
    root
}

#[test]
fn transfer_failure_and_worker_panic_never_fabricate_successful_receipts() {
    let root = owned_fixture("failure");
    fs::write(root.join("source"), b"owned bytes").unwrap();
    for panic_worker in [false, true] {
        let j = job(
            vec![root.join("source").to_string_lossy().into_owned()],
            root.join("dest").to_string_lossy().into_owned(),
        );
        let (tx, rx) = mpsc::channel();
        let (terminal, receipt) = terminal_receipt(74);
        let hooks = WorkerTestHooks {
            provider: Some(Arc::new(move |_, _| {
                if panic_worker {
                    panic!("owned prepublication provider panic");
                }
                Err(std::io::Error::other("owned provider failure"))
            })),
            ..Default::default()
        };
        spawn_job_tracked(&j, tx, terminal, Some(hooks));
        if panic_worker {
            assert!(matches!(
                receipt.recv_timeout(Duration::from_secs(5)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ));
        } else {
            assert_eq!(receipt.recv_timeout(Duration::from_secs(5)).unwrap(), 74);
            let mut terminal = None;
            while let Ok(event) = rx.recv_timeout(Duration::from_secs(5)) {
                if matches!(event, JobEvent::Failed { .. }) {
                    terminal = Some(event);
                    break;
                }
            }
            assert!(matches!(terminal, Some(JobEvent::Failed { .. })));
        }
        assert_eq!(fs::read(root.join("source")).unwrap(), b"owned bytes");
    }
    // The panic intentionally bypasses the recovery suffix. Preserve the
    // owned fixture for QA inspection rather than treating lost ACK as proof.
}

#[test]
fn delete_cancellation_and_normal_empty_branch_acknowledge_the_exact_work_identity() {
    let root = owned_fixture("delete");
    for cancelled in [false, true] {
        let c = JobControl::new();
        if cancelled {
            c.request_cancel();
        }
        let (tx, rx) = mpsc::channel();
        let (terminal, receipt) = terminal_receipt(93);
        let paths = if cancelled {
            vec![root
                .join("cancelled-owned-path")
                .to_string_lossy()
                .into_owned()]
        } else {
            Vec::new()
        };
        spawn_delete_tracked(paths, tx, c, terminal, None);
        assert_eq!(receipt.recv_timeout(Duration::from_secs(5)).unwrap(), 93);
        assert!(matches!(rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            JobEvent::DeleteDone { cancelled: value, .. } if value == cancelled));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_owned_delete_target_keeps_original_terminal_semantics_and_receipt() {
    let root = owned_fixture("missing-delete");
    let (tx, rx) = mpsc::channel();
    let (terminal, receipt) = terminal_receipt(102);
    spawn_delete_tracked(
        vec![root.join("missing").to_string_lossy().into_owned()],
        tx,
        JobControl::new(),
        terminal,
        None,
    );
    assert_eq!(receipt.recv_timeout(Duration::from_secs(5)).unwrap(), 102);
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        JobEvent::DeleteProgress { done: 1, .. }
    ));
    assert!(
        matches!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), JobEvent::DeleteDone { cancelled: false, failed } if failed.is_empty())
    );
    fs::remove_dir_all(root).unwrap();
}
