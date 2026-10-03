use super::*;
use std::time::Duration;

fn wait_done(rx: &mpsc::Receiver<JobEvent>) -> (Option<JobEvent>, Vec<JobEvent>) {
    let mut last = None;
    let mut all = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        while let Ok(ev) = rx.try_recv() {
            let is_final = matches!(
                ev,
                JobEvent::Done { .. } | JobEvent::Failed { .. } | JobEvent::Cancelled { .. }
            );
            all.push(ev);
            if is_final {
                last = Some(all.last().unwrap().clone());
            }
        }
        if last.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    (last, all)
}

fn make_job(paths: Vec<String>, dest: &str) -> Job {
    Job {
        id: 1,
        kind: JobKind::Copy,
        overwrite: OverwritePolicy::AutoRename,
        paths,
        dest_dir: dest.to_string(),
        label: "batch".to_string(),
        total_bytes: None,
        copied_bytes: 0,
        current: String::new(),
        status: JobStatus::Running,
        started_at: std::time::Instant::now(),
        control: JobControl::new(),
    }
}

#[test]
fn batch_copies_all_files_with_one_worker() {
    let base = std::env::temp_dir().join(format!("ira_batch_{}", std::process::id()));
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::create_dir_all(base.join("dst")).unwrap();
    let mut paths = Vec::new();
    for i in 0..30 {
        let p = base.join("src").join(format!("f{i}.txt"));
        std::fs::write(&p, vec![b'x'; 100]).unwrap();
        paths.push(p.to_string_lossy().into_owned());
    }

    let (tx, rx) = mpsc::channel();
    let job = make_job(paths, base.join("dst").to_str().unwrap());
    spawn_job(&job, tx);
    let (last, _) = wait_done(&rx);

    assert!(matches!(last, Some(JobEvent::Done { .. })));
    assert_eq!(std::fs::read_dir(base.join("dst")).unwrap().count(), 30);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn batch_continues_after_io_failure_and_reports_summary() {
    let base = std::env::temp_dir().join(format!("ira_batchfail_{}", std::process::id()));
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::create_dir_all(base.join("dst")).unwrap();
    std::fs::write(base.join("src").join("good1"), "a").unwrap();
    std::fs::write(base.join("src").join("good2"), "b").unwrap();

    let paths = vec![
        base.join("src")
            .join("good1")
            .to_string_lossy()
            .into_owned(),
        "/nonexistent/missing-file".to_string(),
        base.join("src")
            .join("good2")
            .to_string_lossy()
            .into_owned(),
    ];

    let (tx, rx) = mpsc::channel();
    let job = make_job(paths, base.join("dst").to_str().unwrap());
    spawn_job(&job, tx);
    let mut failed_progress = false;
    let mut last = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        match rx.try_recv() {
            Ok(ev) => match ev {
                JobEvent::Progress { current, .. } if current.starts_with("FAILED") => {
                    failed_progress = true;
                }
                JobEvent::Done { .. } | JobEvent::Failed { .. } | JobEvent::Cancelled { .. } => {
                    last = Some(ev);
                    break;
                }
                _ => {}
            },
            Err(_) => {
                if last.is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    assert!(
        matches!(&last, Some(JobEvent::Failed { error, .. }) if error.contains("1 of 3 items failed")),
        "failure summary expected: {last:?}"
    );
    // Good items still transferred despite the middle failure.
    assert!(base.join("dst").join("good1").exists());
    assert!(base.join("dst").join("good2").exists());
    assert!(failed_progress);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn batch_move_removes_sources() {
    let base = std::env::temp_dir().join(format!("ira_batchmove_{}", std::process::id()));
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::create_dir_all(base.join("dst")).unwrap();
    let mut paths = Vec::new();
    for i in 0..5 {
        let p = base.join("src").join(format!("m{i}"));
        std::fs::write(&p, "data").unwrap();
        paths.push(p.to_string_lossy().into_owned());
    }

    let (tx, rx) = mpsc::channel();
    let mut job = make_job(paths, base.join("dst").to_str().unwrap());
    job.kind = JobKind::Move;
    job.overwrite = OverwritePolicy::SkipExisting;
    spawn_job(&job, tx);
    let (last, _) = wait_done(&rx);

    assert!(matches!(last, Some(JobEvent::Done { .. })));
    assert_eq!(std::fs::read_dir(base.join("dst")).unwrap().count(), 5);
    assert_eq!(std::fs::read_dir(base.join("src")).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn move_never_replaces_existing_destination() {
    let base = std::env::temp_dir().join(format!("ira_hard_move_{}", std::process::id()));
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::create_dir_all(base.join("dst")).unwrap();
    std::fs::write(base.join("src").join("a"), "NEW").unwrap();
    std::fs::write(base.join("dst").join("a"), "PRECIOUS").unwrap();

    let paths = vec![base.join("src").join("a").to_string_lossy().into_owned()];
    let (tx, rx) = mpsc::channel();
    let mut job = make_job(paths, base.join("dst").to_str().unwrap());
    job.kind = JobKind::Move;
    job.overwrite = OverwritePolicy::SkipExisting;
    spawn_job(&job, tx);
    let (last, _) = wait_done(&rx);

    assert!(
        matches!(&last, Some(JobEvent::Failed { error, .. }) if error.contains("1 of 1")),
        "move onto an existing file must fail: {last:?}"
    );
    assert_eq!(
        std::fs::read(base.join("dst").join("a")).unwrap(),
        b"PRECIOUS",
        "existing destination must never be replaced"
    );
    assert_eq!(
        std::fs::read(base.join("src").join("a")).unwrap(),
        b"NEW",
        "the source must survive a rejected move"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn copy_never_truncates_existing_destination() {
    let base = std::env::temp_dir().join(format!("ira_hard_copy_{}", std::process::id()));
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::create_dir_all(base.join("dst")).unwrap();
    std::fs::write(base.join("src").join("b"), "NEWDATA").unwrap();
    std::fs::write(base.join("dst").join("b"), "IMPORTANT-OLD").unwrap();

    let paths = vec![base.join("src").join("b").to_string_lossy().into_owned()];
    let (tx, rx) = mpsc::channel();
    let mut job = make_job(paths, base.join("dst").to_str().unwrap());
    job.overwrite = OverwritePolicy::SkipExisting;
    spawn_job(&job, tx);
    let (last, _) = wait_done(&rx);

    assert!(
        matches!(&last, Some(JobEvent::Failed { error, .. }) if error.contains("1 of 1")),
        "copy onto an existing file must fail: {last:?}"
    );
    assert_eq!(
        std::fs::read(base.join("dst").join("b")).unwrap(),
        b"IMPORTANT-OLD",
        "existing destination content must be intact"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn copy_preserves_permissions_and_mtime() {
    use std::os::unix::fs::PermissionsExt;

    let base = std::env::temp_dir().join(format!("ira_hard_perm_{}", std::process::id()));
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::create_dir_all(base.join("dst")).unwrap();
    let src = base.join("src").join("script.sh");
    std::fs::write(&src, "#!/bin/sh\necho hi\n").unwrap();
    std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mtime = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    {
        let f = std::fs::File::options().write(true).open(&src).unwrap();
        f.set_times(std::fs::FileTimes::new().set_modified(mtime))
            .unwrap();
    }

    let paths = vec![src.to_string_lossy().into_owned()];
    let (tx, rx) = mpsc::channel();
    let job = make_job(paths, base.join("dst").to_str().unwrap());
    spawn_job(&job, tx);
    let (last, _) = wait_done(&rx);
    assert!(matches!(last, Some(JobEvent::Done { .. })));

    let dst = base.join("dst").join("script.sh");
    let meta = std::fs::metadata(&dst).unwrap();
    assert_eq!(
        meta.permissions().mode() & 0o777,
        0o755,
        "exec bits preserved"
    );
    assert_eq!(
        meta.modified()
            .unwrap()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        1_700_000_000,
        "mtime preserved"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn symlinks_are_preserved_not_followed() {
    let base = std::env::temp_dir().join(format!("ira_hard_link_{}", std::process::id()));
    std::fs::create_dir_all(base.join("src").join("real")).unwrap();
    std::fs::create_dir_all(base.join("dst")).unwrap();
    std::fs::write(base.join("src").join("real").join("data"), "D").unwrap();
    std::os::unix::fs::symlink("real", base.join("src").join("dirlink")).unwrap();
    std::os::unix::fs::symlink(
        base.join("src").join("real").join("data"),
        base.join("src").join("filelink"),
    )
    .unwrap();

    let paths = vec![
        base.join("src")
            .join("dirlink")
            .to_string_lossy()
            .into_owned(),
        base.join("src")
            .join("filelink")
            .to_string_lossy()
            .into_owned(),
    ];
    let (tx, rx) = mpsc::channel();
    let job = make_job(paths, base.join("dst").to_str().unwrap());
    spawn_job(&job, tx);
    let (last, _) = wait_done(&rx);
    assert!(matches!(last, Some(JobEvent::Done { .. })), "{last:?}");

    // Links preserved as links, pointing at the same relative target.
    let dl = base.join("dst").join("dirlink");
    let fl = base.join("dst").join("filelink");
    assert!(std::fs::symlink_metadata(&dl)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(std::fs::symlink_metadata(&fl)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        std::fs::read_link(&dl).unwrap(),
        std::path::Path::new("real")
    );
    // No dereferenced copies were materialized.
    assert!(!std::fs::symlink_metadata(&dl).unwrap().is_dir());
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn cyclic_symlink_terminates_without_hanging() {
    let base = std::env::temp_dir().join(format!("ira_hard_cyc_{}", std::process::id()));
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::create_dir_all(base.join("dst")).unwrap();
    std::os::unix::fs::symlink("..", base.join("src").join("loop")).unwrap();

    let paths = vec![base.join("src").join("loop").to_string_lossy().into_owned()];
    let (tx, rx) = mpsc::channel();
    let job = make_job(paths, base.join("dst").to_str().unwrap());
    spawn_job(&job, tx);
    let (last, _) = wait_done(&rx);
    assert!(
        matches!(last, Some(JobEvent::Done { .. })),
        "must terminate: {last:?}"
    );
    assert!(std::fs::symlink_metadata(base.join("dst").join("loop"))
        .unwrap()
        .file_type()
        .is_symlink());
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn cancel_mid_copy_removes_partial_destination() {
    let base = std::env::temp_dir().join(format!("ira_hard_cancel_{}", std::process::id()));
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::create_dir_all(base.join("dst")).unwrap();
    let src = base.join("src").join("big.bin");
    std::fs::write(&src, vec![0u8; 64 * 1024 * 1024]).unwrap();

    let paths = vec![src.to_string_lossy().into_owned()];
    let (tx, rx) = mpsc::channel();
    let job = make_job(paths, base.join("dst").to_str().unwrap());
    let control = job.control.clone();
    spawn_job(&job, tx);

    // Pause as soon as the partial dst appears: the worker parks at its
    // next 256KB gate with the partial file still on disk.
    let mut partial_seen = false;
    for _ in 0..1000 {
        if std::fs::symlink_metadata(base.join("dst").join("big.bin")).is_ok() {
            control.set_paused(true);
            partial_seen = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(partial_seen, "partial destination must exist mid-copy");
    std::thread::sleep(Duration::from_millis(50)); // let the worker park
    control.request_cancel();

    let (last, _) = wait_done(&rx);
    assert!(matches!(last, Some(JobEvent::Cancelled { .. })), "{last:?}");
    assert!(
        std::fs::symlink_metadata(base.join("dst").join("big.bin")).is_err(),
        "partial destination must be removed on cancel"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn batch_cancelled_before_start_copies_nothing() {
    let base = std::env::temp_dir().join(format!("ira_batchcancel_{}", std::process::id()));
    std::fs::create_dir_all(base.join("src")).unwrap();
    std::fs::create_dir_all(base.join("dst")).unwrap();
    std::fs::write(base.join("src").join("a"), "a").unwrap();

    let paths = vec![base.join("src").join("a").to_string_lossy().into_owned()];
    let (tx, rx) = mpsc::channel();
    let job = make_job(paths, base.join("dst").to_str().unwrap());
    // Cancel before spawn: the first gate() sees the cancel flag.
    job.control.request_cancel();
    spawn_job(&job, tx);
    let (last, _) = wait_done(&rx);

    assert!(matches!(last, Some(JobEvent::Cancelled { .. })));
    assert!(!base.join("dst").join("a").exists());
    let _ = std::fs::remove_dir_all(&base);
}
