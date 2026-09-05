//! Asynchronous copy/move jobs between panes.
//!
//! Each transfer runs on its own thread and reports progress over an `mpsc`
//! channel; the caller (the TUI) drains events and stays responsive. Jobs can
//! be paused and cancelled through a shared [`JobControl`].

use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Instant;

use parking_lot::{Condvar, Mutex};

/// Whether a job copies or moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Copy,
    Move,
}

/// Lifecycle state of a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Paused,
    Cancelled,
    Done,
    Failed(String),
}

/// Events the worker emits on the job channel.
#[derive(Debug, Clone)]
pub enum JobEvent {
    Started {
        id: u64,
        total_bytes: Option<u64>,
    },
    Progress {
        id: u64,
        copied_bytes: u64,
        current: String,
    },
    Done {
        id: u64,
    },
    /// Delete worker progress: `done`/`total` are path counts, `current` is
    /// the path just removed.
    DeleteProgress {
        done: usize,
        total: usize,
        current: String,
    },
    /// Delete worker finished. `failed` carries (path, error) pairs.
    DeleteDone {
        cancelled: bool,
        failed: Vec<(String, String)>,
    },
    Cancelled {
        id: u64,
    },
    Failed {
        id: u64,
        error: String,
    },
}

/// Shared cancellation + pause state between the UI and the worker thread.
#[derive(Debug)]
pub struct JobControl {
    cancel: AtomicBool,
    pause: Mutex<bool>,
    resume: Condvar,
}

impl JobControl {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            cancel: AtomicBool::new(false),
            pause: Mutex::new(false),
            resume: Condvar::new(),
        })
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.resume.notify_all();
    }

    pub fn set_paused(&self, paused: bool) {
        let mut p = self.pause.lock();
        if *p == paused {
            return;
        }
        *p = paused;
        if !paused {
            self.resume.notify_all();
        }
    }

    /// Blocks while paused; returns `Err` if cancelled.
    fn gate(&self) -> Result<(), JobError> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(JobError::Cancelled);
        }
        let mut paused = self.pause.lock();
        while *paused {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(JobError::Cancelled);
            }
            self.resume.wait(&mut paused);
        }
        Ok(())
    }
}

#[derive(Debug)]
enum JobError {
    Cancelled,
    Io(String),
}

impl From<std::io::Error> for JobError {
    fn from(e: std::io::Error) -> Self {
        JobError::Io(e.to_string())
    }
}

/// A transfer job as tracked by the UI.
pub struct Job {
    pub id: u64,
    pub kind: JobKind,
    /// All paths to copy/move, processed sequentially by the worker.
    pub paths: Vec<String>,
    pub dest_dir: String,
    pub label: String,
    pub total_bytes: Option<u64>,
    pub copied_bytes: u64,
    pub current: String,
    pub status: JobStatus,
    pub started_at: Instant,
    pub control: Arc<JobControl>,
}

const CHUNK: usize = 256 * 1024;
const REPORT_EVERY: u64 = 4 * 1024 * 1024; // progress event every ~4 MiB
const MAX_ENTRIES: u64 = 200_000; // pre-scan cap; above this show indeterminate

/// Spawns ONE worker thread for the whole batch; it processes `job.paths`
/// sequentially and reports progress on `tx`. Returns immediately.
pub fn spawn_job(job: &Job, tx: mpsc::Sender<JobEvent>) {
    let id = job.id;
    let kind = job.kind;
    let control = job.control.clone();
    let paths = job.paths.clone();
    let dest_dir = job.dest_dir.clone();

    thread::spawn(move || {
        let result = run_batch(id, kind, &paths, Path::new(&dest_dir), &control, &tx);
        let event = match result {
            Ok(()) => JobEvent::Done { id },
            Err(JobError::Cancelled) => JobEvent::Cancelled { id },
            Err(JobError::Io(msg)) => JobEvent::Failed { id, error: msg },
        };
        let _ = tx.send(event);
    });
}

/// Runs the batch: pre-scans total bytes, then copies/moves each path in
/// order with one shared byte counter. Per-item I/O failures are counted
/// and skipped (the rest still transfers); the job ends Failed with a
/// summary if any item failed.
fn run_batch(
    id: u64,
    kind: JobKind,
    paths: &[String],
    dest_dir: &Path,
    control: &JobControl,
    tx: &mpsc::Sender<JobEvent>,
) -> Result<(), JobError> {
    // Pre-scan totals (capped): any oversized/unreadable tree -> indeterminate.
    let mut total = 0u64;
    let mut capped = false;
    for p in paths {
        match total_bytes(Path::new(p)) {
            Some(b) => total += b,
            None => capped = true,
        }
    }
    let _ = tx.send(JobEvent::Started {
        id,
        total_bytes: (!capped).then_some(total),
    });

    let mut bytes = 0u64;
    let mut failed = 0usize;
    for p in paths {
        control.gate()?;
        let src = Path::new(p);
        let dst = dest_dir.join(src.file_name().unwrap_or_default());
        let result = match kind {
            JobKind::Copy => copy_entry(src, &dst, control, id, tx, &mut bytes),
            JobKind::Move => match fs::rename(src, &dst) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == ErrorKind::CrossesDevices => {
                    copy_entry(src, &dst, control, id, tx, &mut bytes)?;
                    remove_tree(src)
                }
                Err(e) => Err(JobError::Io(e.to_string())),
            },
        };
        match result {
            Ok(()) => {}
            Err(JobError::Cancelled) => return Err(JobError::Cancelled),
            Err(JobError::Io(_msg)) => {
                failed += 1;
                let _ = tx.send(JobEvent::Progress {
                    id,
                    copied_bytes: bytes,
                    current: format!("FAILED: {}", p),
                });
                continue;
            }
        }
        let _ = tx.send(JobEvent::Progress {
            id,
            copied_bytes: bytes,
            current: p.clone(),
        });
    }

    if failed > 0 {
        return Err(JobError::Io(format!(
            "{} of {} items failed",
            failed,
            paths.len()
        )));
    }
    Ok(())
}

fn copy_entry(
    src: &Path,
    dst: &Path,
    control: &JobControl,
    id: u64,
    tx: &mpsc::Sender<JobEvent>,
    bytes: &mut u64,
) -> Result<(), JobError> {
    control.gate()?;
    let meta = fs::metadata(src)?;
    if meta.is_dir() {
        fs::create_dir_all(dst)?;
        let _ = tx.send(JobEvent::Progress {
            id,
            copied_bytes: *bytes,
            current: src.to_string_lossy().into_owned(),
        });
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_entry(
                &entry.path(),
                &dst.join(entry.file_name()),
                control,
                id,
                tx,
                bytes,
            )?;
        }
        Ok(())
    } else {
        copy_file(src, dst, control, id, tx, bytes)
    }
}

fn copy_file(
    src: &Path,
    dst: &Path,
    control: &JobControl,
    id: u64,
    tx: &mpsc::Sender<JobEvent>,
    bytes: &mut u64,
) -> Result<(), JobError> {
    let mut input = File::open(src)?;
    let mut output = File::create(dst)?;
    let mut buf = vec![0u8; CHUNK];
    let mut since_report = 0u64;
    loop {
        control.gate()?;
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        output.write_all(&buf[..n])?;
        *bytes += n as u64;
        since_report += n as u64;
        if since_report >= REPORT_EVERY {
            since_report = 0;
            let _ = tx.send(JobEvent::Progress {
                id,
                copied_bytes: *bytes,
                current: src.to_string_lossy().into_owned(),
            });
        }
    }
    let _ = tx.send(JobEvent::Progress {
        id,
        copied_bytes: *bytes,
        current: src.to_string_lossy().into_owned(),
    });
    Ok(())
}

/// Sums file sizes in a tree, capped at [`MAX_ENTRIES`] entries.
fn total_bytes(path: &Path) -> Option<u64> {
    let mut total = 0u64;
    let mut count = 0u64;
    sum_sizes(path, &mut total, &mut count);
    if count <= MAX_ENTRIES {
        Some(total)
    } else {
        None
    }
}

fn sum_sizes(path: &Path, total: &mut u64, count: &mut u64) {
    if *count > MAX_ENTRIES {
        return;
    }
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            sum_sizes(&entry.path(), total, count);
            if *count > MAX_ENTRIES {
                return;
            }
        }
    } else {
        *total += meta.len();
        *count += 1;
    }
}

fn remove_tree(path: &Path) -> Result<(), JobError> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Spawns a worker that deletes `paths` one by one (dirs recursively),
/// reporting progress on `tx`. Returns the control handle; `gate()` honours
/// cancel/pause between paths (a single huge `remove_dir_all` is atomic, so
/// granularity is per path).
pub fn spawn_delete_job(paths: Vec<String>, tx: mpsc::Sender<JobEvent>) -> Arc<JobControl> {
    let control = JobControl::new();
    let c = control.clone();
    thread::spawn(move || {
        let total = paths.len();
        let mut failed: Vec<(String, String)> = Vec::new();
        for (i, path) in paths.iter().enumerate() {
            if c.gate().is_err() {
                let _ = tx.send(JobEvent::DeleteDone {
                    cancelled: true,
                    failed,
                });
                return;
            }
            let result = std::fs::symlink_metadata(path).ok().map(|meta| {
                if meta.is_dir() {
                    std::fs::remove_dir_all(path)
                } else {
                    std::fs::remove_file(path)
                }
            });
            if let Some(Err(e)) = result {
                failed.push((path.clone(), e.to_string()));
            }
            let _ = tx.send(JobEvent::DeleteProgress {
                done: i + 1,
                total,
                current: path.clone(),
            });
        }
        let _ = tx.send(JobEvent::DeleteDone {
            cancelled: false,
            failed,
        });
    });
    control
}

#[cfg(test)]
mod batch_tests {
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
            std::fs::write(&p, vec![b'x'; 100]);
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
        let (last, all) = wait_done(&rx);

        assert!(
            matches!(&last, Some(JobEvent::Failed { error, .. }) if error.contains("1 of 3 items failed")),
            "failure summary expected: {last:?}"
        );
        // Good items still transferred despite the middle failure.
        assert!(base.join("dst").join("good1").exists());
        assert!(base.join("dst").join("good2").exists());
        // A FAILED progress event was reported for the bad path.
        assert!(all.iter().any(
            |ev| matches!(ev, JobEvent::Progress { current, .. } if current.starts_with("FAILED:"))
        ));
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
            std::fs::write(&p, "data");
            paths.push(p.to_string_lossy().into_owned());
        }

        let (tx, rx) = mpsc::channel();
        let mut job = make_job(paths, base.join("dst").to_str().unwrap());
        job.kind = JobKind::Move;
        spawn_job(&job, tx);
        let (last, _) = wait_done(&rx);

        assert!(matches!(last, Some(JobEvent::Done { .. })));
        assert_eq!(std::fs::read_dir(base.join("dst")).unwrap().count(), 5);
        assert_eq!(std::fs::read_dir(base.join("src")).unwrap().count(), 0);
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
}
