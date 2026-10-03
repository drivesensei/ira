//! Checked same-directory publication. Success means complete bytes were synced
//! and renamed; directory/power-loss durability and Windows replacement are not promised.
use std::{
    fmt,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistenceError {
    pub path: Option<PathBuf>,
    pub stage: &'static str,
    pub message: String,
}
impl PersistenceError {
    pub fn not_configured() -> Self {
        Self {
            path: None,
            stage: "resolve",
            message: "Persistence path is not configured".into(),
        }
    }
    pub(crate) fn io(path: &Path, stage: &'static str, error: impl fmt::Display) -> Self {
        Self {
            path: Some(path.into()),
            stage,
            message: error.to_string(),
        }
    }
}
impl fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({:?}, {}): {}",
            "Persistence failed", self.path, self.stage, self.message
        )
    }
}
impl std::error::Error for PersistenceError {}
fn resolve(path: &Path) -> Result<PathBuf, PersistenceError> {
    let mut destination = path.to_path_buf();
    let mut links = 0;
    while destination.is_symlink() {
        if links == 40 {
            return Err(PersistenceError::io(
                path,
                "resolve symlink",
                "too many symbolic links",
            ));
        }
        let target = fs::read_link(&destination)
            .map_err(|e| PersistenceError::io(path, "resolve symlink", e))?;
        destination = if target.is_absolute() {
            target
        } else {
            destination
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(target)
        };
        links += 1;
    }
    Ok(destination)
}
#[cfg(unix)]
fn same_version(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    (
        before.dev(),
        before.ino(),
        before.len(),
        before.mtime(),
        before.mtime_nsec(),
        before.ctime(),
        before.ctime_nsec(),
    ) == (
        after.dev(),
        after.ino(),
        after.len(),
        after.mtime(),
        after.mtime_nsec(),
        after.ctime(),
        after.ctime_nsec(),
    )
}
#[cfg(not(unix))]
fn same_version(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    before.len() == after.len() && before.modified().ok() == after.modified().ok()
}
pub(crate) fn publish(path: &Path, bytes: &[u8]) -> Result<(), PersistenceError> {
    publish_with(path, bytes, || {})
}
fn publish_with(
    path: &Path,
    bytes: &[u8],
    before_commit: impl FnOnce(),
) -> Result<(), PersistenceError> {
    // Follow aliases as fs::write did, including a dangling relative target.
    let destination = resolve(path)?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| PersistenceError::io(path, "create directory", e))?;
    let original = match fs::metadata(&destination) {
        Ok(m) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if m.is_file() && m.nlink() > 1 {
                    return Err(PersistenceError::io(
                        path,
                        "identity",
                        "atomic publication of a hard-linked destination is unsupported",
                    ));
                }
            }
            // Opening without truncate proves effective mode/ACL write access.
            let handle = if m.is_file() {
                Some(
                    OpenOptions::new()
                        .write(true)
                        .open(&destination)
                        .map_err(|e| PersistenceError::io(path, "permissions", e))?,
                )
            } else {
                None
            };
            if let Some(handle) = &handle {
                let opened = handle
                    .metadata()
                    .map_err(|e| PersistenceError::io(path, "metadata", e))?;
                if !same_version(&m, &opened) {
                    return Err(PersistenceError::io(
                        path,
                        "identity",
                        "destination changed while opening",
                    ));
                }
            }
            Some((m, handle))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(PersistenceError::io(path, "metadata", e)),
    };
    let (temporary, mut file) = loop {
        let name = format!(
            ".ira-persist-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let temporary = parent.join(name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(PersistenceError::io(path, "create temporary", e)),
        }
    };
    let result = (|| {
        if let Some((metadata, _)) = &original {
            // Pinned Rust b940084d7 Apple std uses fcopyfile(COPYFILE_ALL)
            // for an existing destination: this preserves ACLs and xattrs.
            #[cfg(target_vendor = "apple")]
            if metadata.is_file() {
                fs::copy(&destination, &temporary)
                    .map_err(|e| PersistenceError::io(path, "copy metadata", e))?;
                file.set_len(0)
                    .map_err(|e| PersistenceError::io(path, "prepare temporary", e))?;
            }
            file.set_permissions(metadata.permissions())
                .map_err(|e| PersistenceError::io(path, "permissions", e))?;
        }
        file.write_all(bytes)
            .map_err(|e| PersistenceError::io(path, "write", e))?;
        file.sync_all()
            .map_err(|e| PersistenceError::io(path, "sync", e))?;
        before_commit();
        if resolve(path)? != destination {
            return Err(PersistenceError::io(
                path,
                "identity",
                "configuration alias changed before publication",
            ));
        }
        match (&original, fs::metadata(&destination)) {
            (Some((before, _)), Ok(after)) if same_version(before, &after) => {}
            (None, Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => {
                return Err(PersistenceError::io(
                    path,
                    "identity",
                    "destination changed before publication",
                ))
            }
        }
        drop(file);
        fs::rename(&temporary, &destination).map_err(|e| PersistenceError::io(path, "publish", e))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
pub(crate) fn test_path(name: &str) -> PathBuf {
    static DIRECTORY: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIRECTORY
        .get_or_init(|| {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "ira-unit-persistence-core-{}-{stamp}",
                std::process::id()
            ));
            fs::create_dir(&directory).expect("create task-owned unit persistence directory");
            directory
        })
        .join(name)
}

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod tests;
