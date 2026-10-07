//! Retained local Git-blob exports. Never registers a worktree or changes refs.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

pub(crate) fn output(
    command: &mut Command,
    deadline: Instant,
    limit: usize,
) -> Result<std::process::Output, String> {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_NO_LAZY_FETCH", "1");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: the child only establishes its own process group before exec.
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let (send, receive) = mpsc::channel();
    let stdout = child.stdout.take().ok_or("missing stdout")?;
    let stderr = child.stderr.take().ok_or("missing stderr")?;
    fn reader<R: Read + Send + 'static>(
        reader: R,
        limit: usize,
        id: bool,
        send: mpsc::Sender<(bool, Result<Vec<u8>, String>)>,
    ) {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = reader
                .take(limit as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())
                .and_then(|_| {
                    if bytes.len() > limit {
                        Err("subprocess output limit exceeded".into())
                    } else {
                        Ok(bytes)
                    }
                });
            let _ = send.send((id, result));
        });
    }
    reader(stdout, limit, true, send.clone());
    reader(stderr, limit, false, send);
    let mut out = None;
    let mut err = None;
    let result = (|| loop {
        if Instant::now() >= deadline {
            return Err("subprocess deadline exceeded".into());
        }
        while let Ok((id, value)) = receive.try_recv() {
            let bytes = value?;
            if id {
                out = Some(bytes)
            } else {
                err = Some(bytes)
            }
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if out.is_some() && err.is_some() {
                return Ok(std::process::Output {
                    status,
                    stdout: out.take().unwrap(),
                    stderr: err.take().unwrap(),
                });
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    })();
    if result.is_err() {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        let _ = child.kill();
        let reap = Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < reap => std::thread::sleep(Duration::from_millis(10)),
                _ => {
                    return Err(
                        "subprocess failed and owned child reaping remains unresolved".into(),
                    )
                }
            }
        }
    }
    result
}
fn git(repo: &Path, args: &[&str], deadline: Instant, limit: usize) -> Result<Vec<u8>, String> {
    let out = output(
        Command::new("git")
            .arg("--no-optional-locks")
            .arg("-C")
            .arg(repo)
            .args(args),
        deadline,
        limit,
    )?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into());
    }
    Ok(out.stdout)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub path: PathBuf,
    pub blob: String,
    pub mode: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Manifest {
    pub commit: String,
    pub tree: String,
    pub entries: Vec<Entry>,
}
pub(crate) fn owned_target_child(path: &Path) -> Result<PathBuf, String> {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .ok_or("explicit CARGO_TARGET_DIR required")?;
    if !target.is_absolute() {
        return Err("absolute canonical target required".into());
    }
    let mut prefix = PathBuf::new();
    for c in target.components() {
        prefix.push(c.as_os_str());
        let m = fs::symlink_metadata(&prefix).map_err(|e| e.to_string())?;
        if m.file_type().is_symlink() || !m.is_dir() {
            return Err("target ancestor must be a real directory".into());
        }
    }
    let target = fs::canonicalize(target).map_err(|e| e.to_string())?;
    if !path.is_absolute() || !path.starts_with(&target) || path == target {
        return Err("export/cache must be an absolute child of CARGO_TARGET_DIR".into());
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Prefix(p) => current.push(p.as_os_str()),
            Component::RootDir => current.push(std::path::MAIN_SEPARATOR.to_string()),
            Component::Normal(p) => current.push(p),
            _ => return Err("parent/current traversal rejected".into()),
        };
        match fs::symlink_metadata(&current) {
            Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
                return Err("non-directory or symlink ancestor rejected".into())
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(path.to_owned())
}
pub(crate) fn export(
    repo: &Path,
    commit: &str,
    parent: &Path,
) -> Result<(PathBuf, Manifest), String> {
    owned_target_child(parent)?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let tree = String::from_utf8(git(
        repo,
        &["rev-parse", "--verify", &format!("{commit}^{{tree}}")],
        deadline,
        256,
    )?)
    .map_err(|e| e.to_string())?
    .trim()
    .to_owned();
    let listing = git(
        repo,
        &["ls-tree", "-r", "-z", "-l", commit],
        deadline,
        1024 * 1024,
    )?;
    let mut entries = Vec::new();
    let mut total = 0u64;
    for record in listing.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        if entries.len() >= 4096 {
            return Err("export entry limit exceeded".into());
        }
        let sep = record
            .iter()
            .position(|b| *b == b'\t')
            .ok_or("invalid tree record")?;
        let fields = std::str::from_utf8(&record[..sep])
            .map_err(|e| e.to_string())?
            .split_whitespace()
            .collect::<Vec<_>>();
        if fields.len() != 4 || fields[1] != "blob" || !matches!(fields[0], "100644" | "100755") {
            return Err("only regular Git blobs may be exported".into());
        }
        let path = PathBuf::from(
            std::str::from_utf8(&record[sep + 1..])
                .map_err(|_| "non-UTF8 export path unsupported")?,
        );
        if path.as_os_str().is_empty()
            || path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            || path.components().any(|c| c.as_os_str() == ".git")
        {
            return Err("unsafe export path".into());
        }
        if matches!(
            path.to_str(),
            Some(
                ".ira-parity-export.toml" | ".ira-parity-build.stdout" | ".ira-parity-build.stderr"
            )
        ) {
            return Err("reserved export bookkeeping path".into());
        }
        let bytes = fields[3].parse::<u64>().map_err(|e| e.to_string())?;
        total = total.checked_add(bytes).ok_or("export size overflow")?;
        if bytes > 8 * 1024 * 1024 || total > 64 * 1024 * 1024 {
            return Err("export size limit exceeded".into());
        }
        entries.push(Entry {
            path,
            blob: fields[2].into(),
            mode: fields[0].into(),
            bytes,
            sha256: String::new(),
        });
    }
    let root = tempfile::Builder::new()
        .prefix("ira-parity-export-")
        .tempdir_in(parent)
        .map_err(|e| e.to_string())?
        .keep();
    // All inventory validation precedes payload creation. Partial exports are retained on error.
    for entry in &mut entries {
        let bytes = git(
            repo,
            &["cat-file", "blob", &entry.blob],
            deadline,
            8 * 1024 * 1024,
        )?;
        if bytes.len() as u64 != entry.bytes {
            return Err("Git blob length mismatch".into());
        }
        let p = root.join(&entry.path);
        fs::create_dir_all(p.parent().ok_or("missing parent")?).map_err(|e| e.to_string())?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&p)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(if entry.mode == "100755" {
                0o755
            } else {
                0o644
            }))
            .map_err(|e| e.to_string())?;
        }
        entry.sha256 = hex::encode(Sha256::digest(&bytes));
    }
    let manifest = Manifest {
        commit: commit.into(),
        tree,
        entries,
    };
    fs::write(
        root.join(".ira-parity-export.toml"),
        toml::to_string(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok((root, manifest))
}
pub(crate) fn verified(root: &Path, manifest: &Manifest) -> bool {
    manifest
        .entries
        .iter()
        .all(|entry| verified_entry(root, entry).unwrap_or(false))
}
#[cfg(unix)]
fn verified_entry(root: &Path, entry: &Entry) -> Result<bool, String> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::{
                ffi::OsStrExt,
                fs::{OpenOptionsExt, PermissionsExt},
            },
        },
    };
    owned_target_child(root)?;
    let mut directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)
        .map_err(|e| e.to_string())?;
    let mut parts = entry.path.components().peekable();
    while let Some(part) = parts.next() {
        let name = match part {
            Component::Normal(n) => CString::new(n.as_bytes()).map_err(|e| e.to_string())?,
            _ => return Err("unsafe verification path".into()),
        };
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | if parts.peek().is_some() {
                libc::O_DIRECTORY
            } else {
                0
            };
        // SAFETY: directory is an owned FD and name is one checked NUL-terminated component.
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let file = unsafe { fs::File::from_raw_fd(fd) };
        if parts.peek().is_some() {
            directory = file;
            continue;
        }
        let before = file.metadata().map_err(|e| e.to_string())?;
        let mode = if entry.mode == "100755" { 0o755 } else { 0o644 };
        if !before.is_file()
            || before.len() != entry.bytes
            || before.permissions().mode() & 0o777 != mode
        {
            return Ok(false);
        }
        let mut bytes = Vec::new();
        let mut reader = file;
        Read::by_ref(&mut reader)
            .take(entry.bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        let after = reader.metadata().map_err(|e| e.to_string())?;
        use std::os::unix::fs::MetadataExt;
        return Ok(before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.size() == after.size()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec()
            && before.mode() == after.mode()
            && hex::encode(Sha256::digest(bytes)) == entry.sha256);
    }
    Ok(false)
}
#[cfg(windows)]
fn verified_entry(root: &Path, entry: &Entry) -> Result<bool, String> {
    use std::os::windows::fs::OpenOptionsExt;
    owned_target_child(root)?;
    let mut pinned = Vec::new();
    let mut path = root.to_owned();
    for part in entry.path.components() {
        match part {
            Component::Normal(n) => path.push(n),
            _ => return Err("unsafe verification path".into()),
        };
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if metadata.file_type().is_symlink() {
            return Ok(false);
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .custom_flags(0x00200000 | 0x02000000)
            .open(&path)
            .map_err(|e| e.to_string())?;
        pinned.push(file);
    }
    let mut file = pinned.pop().ok_or("empty entry path")?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() != entry.bytes {
        return Ok(false);
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(entry.bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    // Windows has no Unix executable-bit equivalent; modes remain bound in the manifest.
    Ok(hex::encode(Sha256::digest(bytes)) == entry.sha256)
}
