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
    /// Replace existing destination files; existing directories are refused.
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
/// Host-supplied atomic entry rename: must refuse every existing destination,
/// preserve source and destination on failure, and never follow entry symlinks.
/// It must support files and directories without copying across volumes.
pub type NoReplaceProvider = dyn Fn(&Path, &Path) -> std::io::Result<()> + Send + Sync;

pub fn spawn_job(job: &Job, tx: mpsc::Sender<JobEvent>) {
    spawn_job_with_provider(job, tx, Arc::new(rename_no_replace));
}

/// Additive host hook for platforms needing a native no-replace primitive.
pub fn spawn_job_with_provider(
    job: &Job,
    tx: mpsc::Sender<JobEvent>,
    provider: Arc<NoReplaceProvider>,
) {
    let id = job.id;
    let kind = job.kind;
    let control = job.control.clone();
    let paths = job.paths.clone();
    let dest_dir = job.dest_dir.clone();
    let policy = job.overwrite;

    thread::spawn(move || {
        let result = run_batch_with_provider(
            id,
            kind,
            &paths,
            Path::new(&dest_dir),
            policy,
            &control,
            &tx,
            provider.as_ref(),
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
#[cfg(test)]
fn run_batch(
    id: u64,
    kind: JobKind,
    paths: &[String],
    dest_dir: &Path,
    policy: OverwritePolicy,
    control: &JobControl,
    tx: &mpsc::Sender<JobEvent>,
) -> Result<(), JobError> {
    run_batch_with_provider(
        id,
        kind,
        paths,
        dest_dir,
        policy,
        control,
        tx,
        &rename_no_replace,
    )
}
#[allow(clippy::too_many_arguments)]
fn run_batch_with_provider(
    id: u64,
    kind: JobKind,
    paths: &[String],
    dest_dir: &Path,
    policy: OverwritePolicy,
    control: &JobControl,
    tx: &mpsc::Sender<JobEvent>,
    provider: &NoReplaceProvider,
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
    let mut failure_details = Vec::new();
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
        let mut context = TransferContext {
            control,
            id,
            tx,
            bytes: &mut bytes,
            provider,
        };
        let result = transfer_entry(src, &dst, kind, policy, &mut context);
        match result {
            Ok(()) => {}
            Err(JobError::Cancelled) => {
                // Staged copies clean only their private working directory.
                return Err(JobError::Cancelled);
            }
            Err(JobError::Io(msg)) => {
                failed += 1;
                failure_details.push(format!("{p}: {msg}"));
                // A failed item never authorizes deleting the public path.
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
            "{} of {} items failed; {}",
            failed,
            paths.len(),
            failure_details.join("; ")
        )));
    }
    Ok(())
}

/// Atomically moves an entry without replacing a destination created by another actor.
/// The safe Unix backend is the already pinned rustix publication primitive.
pub fn rename_no_replace(src: &Path, dst: &Path) -> std::io::Result<()> {
    #[cfg(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "android",
        target_os = "ios"
    ))]
    {
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            src,
            rustix::fs::CWD,
            dst,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(Into::into)
    }
    #[cfg(windows)]
    {
        let _ = (src, dst);
        Err(std::io::Error::new(
            ErrorKind::Unsupported,
            "atomic public-entry capture requires a Windows no-replace provider",
        ))
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "android",
        target_os = "ios",
        windows
    )))]
    {
        let _ = (src, dst);
        Err(std::io::Error::new(
            ErrorKind::Unsupported,
            "atomic no-replace publication unavailable",
        ))
    }
}

/// Publishes a privately owned staged entry without replacing a public target.
/// The caller must own `src`; Windows file publication uses a hard-link reservation.
pub fn publish_private_no_replace(src: &Path, dst: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if fs::symlink_metadata(src)?.is_dir() {
            let _ = dst;
            return Err(std::io::Error::new(
                ErrorKind::Unsupported,
                "atomic directory publication requires a Windows no-replace provider",
            ));
        }
        fs::hard_link(src, dst)?;
        fs::remove_file(src)
    }
    #[cfg(not(windows))]
    {
        rename_no_replace(src, dst)
    }
}

static NEXT_STAGE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
fn create_stage(parent: &Path) -> Result<std::path::PathBuf, JobError> {
    loop {
        let sequence = NEXT_STAGE.fetch_add(1, Ordering::Relaxed);
        let stage = parent.join(format!(".ira-transfer-{}-{sequence}", std::process::id()));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&stage) {
            Ok(()) => return Ok(stage),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
}
struct TransferContext<'a> {
    control: &'a JobControl,
    id: u64,
    tx: &'a mpsc::Sender<JobEvent>,
    bytes: &'a mut u64,
    provider: &'a NoReplaceProvider,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StagePoint {
    PayloadReady,
    BackupCaptured,
    BeforePublication,
    Published,
    BeforeSourceRemoval,
}

fn copy_staged(
    src: &Path,
    dst: &Path,
    replace: bool,
    context: &mut TransferContext<'_>,
    mut before_publish: impl FnMut(&Path),
) -> Result<(), JobError> {
    transfer_staged(
        src,
        dst,
        JobKind::Copy,
        replace,
        context,
        false,
        |point, payload| {
            if point == StagePoint::PayloadReady {
                before_publish(payload);
            }
            Ok(())
        },
    )
}
fn transfer_staged(
    src: &Path,
    dst: &Path,
    kind: JobKind,
    replace: bool,
    context: &mut TransferContext<'_>,
    force_copy_move: bool,
    mut hook: impl FnMut(StagePoint, &Path) -> Result<(), JobError>,
) -> Result<(), JobError> {
    let stage = create_stage(
        dst.parent()
            .ok_or_else(|| JobError::Io("destination has no parent".into()))?,
    )?;
    let payload = stage.join("payload");
    let backup = stage.join("backup");
    let mut source_moved = false;
    let mut published = false;
    let mut backup_captured = false;
    let result = (|| {
        if kind == JobKind::Move && !force_copy_move {
            match (context.provider)(src, &payload) {
                Ok(()) => source_moved = true,
                Err(error) if error.kind() == ErrorKind::CrossesDevices => {}
                Err(error) => return Err(error.into()),
            }
        }
        if !source_moved {
            copy_entry(
                src,
                &payload,
                context.control,
                context.id,
                context.tx,
                context.bytes,
            )?;
        }
        hook(StagePoint::PayloadReady, &payload)?;
        context.control.gate()?;
        if replace {
            // Capture first; no path-check followed by deletion of a public entry.
            (context.provider)(dst, &backup)?;
            backup_captured = true;
            // The object actually captured can differ from the earlier lookup.
            // Existing directories are refused, then restored without clobbering.
            if fs::symlink_metadata(&backup)?.is_dir() {
                return Err(std::io::Error::new(
                    ErrorKind::AlreadyExists,
                    "captured destination is a directory",
                )
                .into());
            }
            hook(StagePoint::BackupCaptured, &payload)?;
        }
        hook(StagePoint::BeforePublication, &payload)?;
        context.control.gate()?;
        (context.provider)(&payload, dst)?;
        published = true;
        hook(StagePoint::Published, &payload)?;
        if kind == JobKind::Move && !source_moved {
            hook(StagePoint::BeforeSourceRemoval, &payload)?;
            remove_tree(src)?;
        }
        Ok(())
    })();
    if let Err(primary) = result {
        let mut recovery = Vec::new();
        // A published path can have been replaced by another actor. Capture it
        // atomically and retain it; never blindly delete it during rollback.
        let mut retain = published;
        if published {
            if let Err(error) = (context.provider)(dst, &payload) {
                recovery.push(format!("published entry recovery failed: {error}"));
            }
        }
        if backup_captured {
            if let Err(error) = (context.provider)(&backup, dst) {
                retain = true;
                recovery.push(format!("destination restore failed: {error}"));
            }
        }
        if source_moved && !published {
            if let Err(error) = (context.provider)(&payload, src) {
                retain = true;
                recovery.push(format!("source restore failed: {error}"));
            }
        }
        // Even a provider which reports an error after creating a backup must
        // not cause that backup to be destroyed by generic staging cleanup.
        if fs::symlink_metadata(&backup).is_ok() {
            retain = true;
        }
        if retain {
            return Err(JobError::Io(format!(
                "{primary:?}; {}; recoverable transfer data retained at {}",
                recovery.join("; "),
                stage.display()
            )));
        }
        return match fs::remove_dir_all(&stage) {
            Ok(()) => Err(primary),
            Err(cleanup) => Err(JobError::Io(format!(
                "{primary:?}; staging cleanup failed at {}: {cleanup}",
                stage.display()
            ))),
        };
    }
    // Backup destruction is allowed only after publication and move-source
    // deletion have both committed successfully. It is inside the private root.
    fs::remove_dir_all(&stage).map_err(|cleanup| {
        JobError::Io(format!(
            "staging cleanup failed at {}: {cleanup}",
            stage.display()
        ))
    })
}
fn transfer_entry(
    src: &Path,
    dst: &Path,
    kind: JobKind,
    policy: OverwritePolicy,
    context: &mut TransferContext<'_>,
) -> Result<(), JobError> {
    let destination = match fs::symlink_metadata(dst) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if policy == OverwritePolicy::Overwrite
        && destination.as_ref().is_some_and(fs::Metadata::is_dir)
    {
        return Err(std::io::Error::new(
            ErrorKind::AlreadyExists,
            "destination directory already exists",
        )
        .into());
    }
    let replace = policy == OverwritePolicy::Overwrite && destination.is_some();
    if kind == JobKind::Copy {
        copy_staged(src, dst, replace, context, |_| {})
    } else {
        transfer_staged(src, dst, kind, replace, context, false, |_, _| Ok(()))
    }
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
