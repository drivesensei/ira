//! Private, exclusively created staging for atomic editor saves.
use std::fs::{self, Permissions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn private_directory(parent: &Path, mut next_name: impl FnMut() -> String) -> io::Result<PathBuf> {
    for _ in 0..64 {
        let directory = parent.join(next_name());
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "editor staging names exhausted",
    ))
}

/// The private namespace excludes unrelated actors under the OS permission model.
/// This does not promise protection against a malicious actor running as this user.
pub(super) fn save(path: &Path, content: &[u8], permissions: Permissions) -> io::Result<()> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "editor target has no parent")
    })?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let directory = private_directory(parent, || {
        format!(
            ".ira-save-{}-{stamp:x}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        )
    })?;
    let payload = directory.join("content");
    let mut owned_payload = false;
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&payload)?;
        owned_payload = true;
        file.write_all(content)?;
        file.flush()?;
        file.set_permissions(permissions)?;
        drop(file);
        fs::rename(&payload, path)
    })();
    // Only our payload and empty private directory; never recursively delete or
    // touch the legacy fixed .ira-tmp path. A foreign entry leaves cleanup incomplete.
    if owned_payload {
        let _ = fs::remove_file(&payload);
    }
    let _ = fs::remove_dir(&directory);
    result
}

#[cfg(test)]
#[path = "editor_staging_tests.rs"]
mod tests;
