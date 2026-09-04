//! Asynchronous copy/move jobs between panes.
//!
//! Each transfer runs on its own thread and reports progress over an `mpsc`
//! channel; the caller (the TUI) drains events and stays responsive. Jobs can
//! be paused and cancelled through a shared [`JobControl`].

use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
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
#[derive(Debug)]
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
    Cancelled {
        id: u64,
    },
    Failed {
        id: u64,
        error: String,
    },
}

/// Shared cancellation + pause state between the UI and the worker thread.
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
    pub source: String,
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

/// Spawns a worker thread for `job`; the thread runs to completion and reports
/// its outcome on `tx`. The cloned values are owned by the thread, so this
/// returns immediately.
pub fn spawn_job(job: &Job, tx: mpsc::Sender<JobEvent>) {
    let id = job.id;
    let kind = job.kind;
    let control = job.control.clone();
    let src = PathBuf::from(&job.source);
    let dst = Path::new(&job.dest_dir).join(&job.label).to_path_buf();

    thread::spawn(move || {
        let result = run(id, kind, &src, &dst, &control, &tx);
        let event = match result {
            Ok(()) => JobEvent::Done { id },
            Err(JobError::Cancelled) => JobEvent::Cancelled { id },
            Err(JobError::Io(msg)) => JobEvent::Failed { id, error: msg },
        };
        let _ = tx.send(event);
    });
}

fn run(
    id: u64,
    kind: JobKind,
    src: &Path,
    dst: &Path,
    control: &JobControl,
    tx: &mpsc::Sender<JobEvent>,
) -> Result<(), JobError> {
    let total = if kind == JobKind::Copy {
        total_bytes(src)
    } else {
        None
    };
    let _ = tx.send(JobEvent::Started {
        id,
        total_bytes: total,
    });

    let mut bytes = 0u64;
    match kind {
        JobKind::Copy => copy_entry(src, dst, control, id, tx, &mut bytes),
        JobKind::Move => match fs::rename(src, dst) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::CrossesDevices => {
                copy_entry(src, dst, control, id, tx, &mut bytes)?;
                remove_tree(src)
            }
            Err(e) => Err(JobError::Io(e.to_string())),
        },
    }
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
