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
pub(crate) fn publish(path: &Path, bytes: &[u8]) -> Result<(), PersistenceError> {
    // Follow an existing symlink as the legacy fs::write did, preserving the link.
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
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| PersistenceError::io(path, "create directory", e))?;
    // Refuse read-only targets rather than replacing them through directory rights.
    let permissions = match fs::metadata(&destination) {
        Ok(m) => {
            if m.permissions().readonly() {
                return Err(PersistenceError::io(
                    path,
                    "permissions",
                    "destination is read-only",
                ));
            }
            Some(m.permissions())
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
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)
                .map_err(|e| PersistenceError::io(path, "permissions", e))?;
        }
        file.write_all(bytes)
            .map_err(|e| PersistenceError::io(path, "write", e))?;
        file.sync_all()
            .map_err(|e| PersistenceError::io(path, "sync", e))?;
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
                "ira-unit-persistence-root-{}-{stamp}",
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
