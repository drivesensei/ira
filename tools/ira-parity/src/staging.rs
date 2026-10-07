//! No-clobber staging anchored to directory ownership rather than symlink paths.
#[cfg(windows)]
use std::path::PathBuf;
#[cfg(unix)]
use std::{
    ffi::CString,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    },
};
use std::{
    fs::File,
    io::Write,
    path::{Component, Path},
};

pub(super) struct Directory {
    #[cfg(unix)]
    file: File,
    #[cfg(windows)]
    path: PathBuf,
    // Denying delete sharing pins every Windows ancestor against rename/reparse substitution.
    #[cfg(windows)]
    guards: Vec<File>,
}
impl Directory {
    pub(super) fn stage(path: &Path) -> Result<Self, String> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(path)
        };
        // macOS exposes these OS-owned aliases in the standard temporary directory.
        // Resolve only these exact system aliases; all caller-controlled components stay no-follow.
        #[cfg(target_os = "macos")]
        let absolute = {
            let mut p = absolute;
            for (alias, target) in [("/var", "/private/var"), ("/tmp", "/private/tmp")] {
                if let Ok(tail) = p.strip_prefix(alias) {
                    if std::fs::canonicalize(alias).ok().as_deref() == Some(Path::new(target)) {
                        p = Path::new(target).join(tail);
                        break;
                    }
                }
            }
            p
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open("/")
                .map_err(|e| e.to_string())?;
            let mut dir = Self { file };
            for c in absolute.components() {
                match c {
                    Component::RootDir | Component::CurDir => {}
                    Component::Normal(name) => {
                        dir = dir.directory(name, true)?;
                    }
                    _ => return Err("staging path must not contain parent traversal".into()),
                }
            }
            Ok(dir)
        }
        #[cfg(windows)]
        {
            let mut path = PathBuf::new();
            let mut guards = vec![];
            for c in absolute.components() {
                match c {
                    Component::Prefix(p) => path.push(p.as_os_str()),
                    Component::RootDir => path.push(c.as_os_str()),
                    Component::CurDir => {}
                    Component::ParentDir => {
                        return Err("staging path must not contain parent traversal".into())
                    }
                    Component::Normal(name) => {
                        path.push(name);
                        match std::fs::create_dir(&path) {
                            Ok(()) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                            Err(e) => return Err(e.to_string()),
                        };
                        guards.push(Self::pin_windows(&path)?);
                    }
                }
            }
            if guards.is_empty() {
                guards.push(Self::pin_windows(&path)?);
            }
            Ok(Self { path, guards })
        }
    }
    #[cfg(unix)]
    fn directory(&self, name: &std::ffi::OsStr, allow_existing: bool) -> Result<Self, String> {
        let name = CString::new(name.as_bytes()).map_err(|e| e.to_string())?;
        // SAFETY: owned directory FD and NUL-terminated single component. No path traversal.
        let result = unsafe { libc::mkdirat(self.file.as_raw_fd(), name.as_ptr(), 0o700) };
        if result != 0 {
            let e = std::io::Error::last_os_error();
            if !allow_existing || e.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(e.to_string());
            }
        }
        // SAFETY: opening relative to the pinned parent and rejecting symlinks.
        let fd = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(Self {
            file: unsafe { File::from_raw_fd(fd) },
        })
    }
    #[cfg(windows)]
    fn pin_windows(path: &Path) -> Result<File, String> {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        let f = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .custom_flags(0x02000000 | 0x00200000)
            .open(path)
            .map_err(|e| e.to_string())?;
        let meta = f.metadata().map_err(|e| e.to_string())?;
        if !meta.is_dir() || meta.file_attributes() & 0x400 != 0 {
            return Err("staging directory is a reparse point or not a directory".into());
        }
        Ok(f)
    }
    pub(super) fn child(&self, name: &str) -> Result<Self, String> {
        #[cfg(unix)]
        {
            self.directory(std::ffi::OsStr::new(name), false)
        }
        #[cfg(windows)]
        {
            let path = self.path.join(name);
            std::fs::create_dir(&path).map_err(|e| e.to_string())?;
            let mut guards = self
                .guards
                .iter()
                .map(|f| f.try_clone())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            guards.push(Self::pin_windows(&path)?);
            Ok(Self { path, guards })
        }
    }
    pub(super) fn create_file(&self, name: &str) -> Result<File, String> {
        #[cfg(unix)]
        {
            let name = CString::new(name).map_err(|e| e.to_string())?;
            // SAFETY: pinned directory FD, generated single-component file name; O_EXCL rejects existing links/files.
            let fd = unsafe {
                libc::openat(
                    self.file.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(unsafe { File::from_raw_fd(fd) })
        }
        #[cfg(windows)]
        {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.path.join(name))
                .map_err(|e| e.to_string())
        }
    }
    pub(super) fn ensure_absent(&self, names: &[&str]) -> Result<(), String> {
        for name in names {
            #[cfg(unix)]
            {
                let name = CString::new(*name).map_err(|e| e.to_string())?;
                let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
                // SAFETY: valid directory FD/name and writable stat storage; no link following.
                if unsafe {
                    libc::fstatat(
                        self.file.as_raw_fd(),
                        name.as_ptr(),
                        metadata.as_mut_ptr(),
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                } == 0
                {
                    return Err("staging artifact already exists".into());
                }
                let e = std::io::Error::last_os_error();
                if e.kind() != std::io::ErrorKind::NotFound {
                    return Err(e.to_string());
                }
            }
            #[cfg(windows)]
            {
                match std::fs::symlink_metadata(self.path.join(name)) {
                    Ok(_) => return Err("staging artifact already exists".into()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
        }
        Ok(())
    }
    pub(super) fn write_new(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let mut file = self.create_file(name)?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())
    }
    pub(super) fn publish_manifest(&self, bytes: &[u8]) -> Result<(), String> {
        self.write_new("manifest.pending", bytes)?;
        #[cfg(unix)]
        {
            let old = CString::new("manifest.pending").map_err(|e| e.to_string())?;
            let new = CString::new("manifest.toml").map_err(|e| e.to_string())?;
            // SAFETY: both names are generated and resolve under the same owned directory FD. linkat never clobbers.
            if unsafe {
                libc::linkat(
                    self.file.as_raw_fd(),
                    old.as_ptr(),
                    self.file.as_raw_fd(),
                    new.as_ptr(),
                    0,
                )
            } != 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            // Remove only the exclusively created pending entry after successful publication.
            if unsafe { libc::unlinkat(self.file.as_raw_fd(), old.as_ptr(), 0) } != 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            self.file.sync_all().map_err(|e| e.to_string())?;
        }
        #[cfg(windows)]
        {
            std::fs::hard_link(
                self.path.join("manifest.pending"),
                self.path.join("manifest.toml"),
            )
            .map_err(|e| e.to_string())?;
            std::fs::remove_file(self.path.join("manifest.pending")).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
