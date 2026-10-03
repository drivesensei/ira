//! Asynchronous copy/move jobs between panes.
//!
//! Each transfer runs on its own thread and reports progress over an `mpsc`
//! channel; the caller (the TUI) drains events and stays responsive. Jobs can
//! be paused and cancelled through a shared [`JobControl`].

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{symlink as create_symlink, OpenOptionsExt, PermissionsExt};
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

/// What happens when a destination path already exists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OverwritePolicy {
    /// Never overwrite: colliding destinations are renamed to
    /// `name (2).ext`, `name (3).ext`, ... (first free number).
    #[default]
    AutoRename,
    /// Never overwrite: colliding destinations are skipped and reported.
    SkipExisting,
    /// Replace existing destination files outright (folders merge).
    Overwrite,
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
    pub overwrite: OverwritePolicy,
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
    let policy = job.overwrite;

    thread::spawn(move || {
        let result = run_batch(
            id,
            kind,
            &paths,
            Path::new(&dest_dir),
            policy,
            &control,
            &tx,
        );
        let event = match result {
            Ok(()) => JobEvent::Done { id },
            Err(JobError::Cancelled) => JobEvent::Cancelled { id },
            Err(JobError::Io(msg)) => JobEvent::Failed { id, error: msg },
        };
        let _ = tx.send(event);
    });
}

/// Resolves the destination path under the overwrite policy. Returns the
/// final destination, or `None` when the policy says to skip the item.
fn resolve_destination(
    src: &Path,
    dest_dir: &Path,
    policy: OverwritePolicy,
) -> Option<std::path::PathBuf> {
    let name = src.file_name().unwrap_or_default();
    let dst = dest_dir.join(name);
    if fs::symlink_metadata(&dst).is_err() {
        return Some(dst);
    }
    match policy {
        OverwritePolicy::Overwrite => Some(dst),
        OverwritePolicy::SkipExisting => None,
        OverwritePolicy::AutoRename => {
            // name (2).ext, name (3).ext, ... — first free number.
            let stem = Path::new(name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let ext = Path::new(name)
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()));
            for n in 2.. {
                let candidate = match &ext {
                    Some(e) => dest_dir.join(format!("{stem} ({n}){e}")),
                    None => dest_dir.join(format!("{stem} ({n})")),
                };
                if fs::symlink_metadata(&candidate).is_err() {
                    return Some(candidate);
                }
            }
            None
        }
    }
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
    policy: OverwritePolicy,
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
        let Some(dst) = resolve_destination(src, dest_dir, policy) else {
            failed += 1;
            let _ = tx.send(JobEvent::Progress {
                id,
                copied_bytes: bytes,
                current: format!("SKIPPED (already exists): {}", p),
            });
            continue;
        };
        // Overwrite policy: replace the existing destination file before
        // copying (create_new would otherwise refuse). Folders merge.
        if policy == OverwritePolicy::Overwrite
            && fs::symlink_metadata(&dst).is_ok_and(|m| !m.is_dir())
        {
            fs::remove_file(&dst)?;
        }
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
            Err(JobError::Cancelled) => {
                // Cancelled mid-item: remove the partial destination we
                // created so neither a truncated copy nor a half-moved tree
                // is left behind. The source is untouched.
                let _ = remove_tree(&dst);
                return Err(JobError::Cancelled);
            }
            Err(JobError::Io(msg)) => {
                failed += 1;
                // Item failed: remove our partial destination (never the
                // source) so no truncated file is mistaken for a copy.
                let _ = remove_tree(&dst);
                let _ = tx.send(JobEvent::Progress {
                    id,
                    copied_bytes: bytes,
                    current: format!("FAILED ({}): {}", msg, p),
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

/// Windows fallback for symlink sources: creating symlinks needs
/// privileges, so the link is followed and its destination copied as a
/// regular file or directory. Broken links and link cycles fail with an
/// error instead of recursing (canonicalize resolves the chain or errors).
#[cfg(not(unix))]
fn copy_file_follow(
    src: &Path,
    dst: &Path,
    control: &JobControl,
    id: u64,
    tx: &mpsc::Sender<JobEvent>,
    bytes: &mut u64,
) -> Result<(), JobError> {
    let target = fs::read_link(src)?;
    let link_dir = src.parent().unwrap_or(Path::new("."));
    let resolved = fs::canonicalize(link_dir.join(&target))?;
    copy_entry(&resolved, dst, control, id, tx, bytes)
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
    // symlink_metadata does NOT follow symlinks: links are preserved as
    // links (never dereferenced, so cyclic symlinks cannot recurse).
    let meta = fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        #[cfg(unix)]
        {
            let target = fs::read_link(src)?;
            create_symlink(&target, dst)?;
            let _ = tx.send(JobEvent::Progress {
                id,
                copied_bytes: *bytes,
                current: src.to_string_lossy().into_owned(),
            });
            return Ok(());
        }
        #[cfg(not(unix))]
        {
            // Windows: creating symlinks needs privileges; fall back to
            // following the link (previous behavior).
            return copy_file_follow(src, dst, control, id, tx, bytes);
        }
    }
    if meta.is_dir() {
        fs::create_dir(dst)?;
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
        // Directory permissions are applied AFTER the children (a read-only
        // source dir would otherwise block writing into the copy).
        fs::set_permissions(dst, meta.permissions())?;
        Ok(())
    } else {
        copy_file(src, dst, control, id, tx, bytes, &meta)
    }
}

fn copy_file(
    src: &Path,
    dst: &Path,
    control: &JobControl,
    id: u64,
    tx: &mpsc::Sender<JobEvent>,
    bytes: &mut u64,
    meta: &fs::Metadata,
) -> Result<(), JobError> {
    let mut input = File::open(src)?;
    // create_new is atomic: an existing destination can never be truncated
    // (the batch-level exists-check already routed those away; this closes
    // the race and any symlink-follow surprise).
    let mut output = open_dest_file(dst, meta)?;
    let result = (|| {
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
        // Preserve the source modification time (std-only, no new deps).
        if let Ok(modified) = meta.modified() {
            let times = std::fs::FileTimes::new().set_modified(modified);
            let _ = output.set_times(times);
        }
        Ok(())
    })();
    if result.is_err() {
        // Cancelled or failed mid-file: remove the partial destination.
        let _ = fs::remove_file(dst);
    }
    result
}

/// Opens the destination file atomically (create_new), preserving the
/// source's permission bits on Unix.
#[cfg(unix)]
fn open_dest_file(dst: &Path, meta: &fs::Metadata) -> std::io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(meta.permissions().mode())
        .open(dst)
}

#[cfg(not(unix))]
fn open_dest_file(dst: &Path, _meta: &fs::Metadata) -> std::io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(dst)
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
#[path = "transfer_tests.rs"]
mod batch_tests;

#[cfg(test)]
#[path = "transfer_safety_tests.rs"]
mod safety_tests;
