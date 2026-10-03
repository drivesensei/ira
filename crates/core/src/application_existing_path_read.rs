use super::*;
pub(super) fn load(
    path: PathBuf,
    kind: ExistingPathKind,
    hidden: bool,
    sort: usize,
) -> Result<Loaded, String> {
    if path.to_str().is_none() {
        return Err("Native path is unsupported: non-UTF8 identity".into());
    }
    if !path.is_absolute() {
        return Err("Selected path must be absolute".into());
    }
    let metadata =
        std::fs::metadata(&path).map_err(|e| format!("Selected path unavailable: {e}"))?;
    let (folder, target) = match kind {
        ExistingPathKind::Folder if metadata.is_dir() => (path, None),
        ExistingPathKind::File if metadata.is_file() => {
            let parent = path
                .parent()
                .ok_or("Selected file has no parent")?
                .to_path_buf();
            (parent, Some(path))
        }
        _ => return Err("Selected path does not have the requested file/folder kind".into()),
    };
    if folder.to_str().is_none() {
        return Err("Native parent path is unsupported: non-UTF8 identity".into());
    }
    let entries =
        std::fs::read_dir(&folder).map_err(|e| format!("Directory listing failed: {e}"))?;
    let mut files = read_entries(entries, hidden)?;
    sort_entries(&mut files, sort);
    if target
        .as_ref()
        .is_some_and(|target| !files.iter().any(|f| Path::new(&f.path) == target))
    {
        return Err(
            "Selected file is unavailable in the retained hidden-file policy/listing".into(),
        );
    }
    Ok(Loaded {
        folder,
        target,
        files,
    })
}
// Each real readdir/metadata failure is propagated, including after a prefix.
pub(super) fn read_entries(
    entries: impl IntoIterator<Item = std::io::Result<std::fs::DirEntry>>,
    hidden: bool,
) -> Result<Vec<FEntry>, String> {
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("Partial directory listing failed: {e}"))?;
        let name = entry.file_name();
        if !hidden && raw_dot_prefix(&name) {
            continue;
        }
        let path = entry.path();
        let identity = path
            .to_str()
            .ok_or("Directory listing unsupported: non-UTF8 identity")?;
        let label = name
            .to_str()
            .ok_or("Directory listing unsupported: non-UTF8 label")?
            .to_owned();
        let metadata =
            std::fs::metadata(&path).map_err(|e| format!("Directory entry unavailable: {e}"))?;
        files.push(FEntry {
            path: identity.into(),
            label,
            is_dir: metadata.is_dir(),
            size: if metadata.is_dir() { 0 } else { metadata.len() },
            modified: metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64),
        });
    }
    Ok(files)
}

fn raw_dot_prefix(name: &std::ffi::OsStr) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        name.as_bytes().first() == Some(&b'.')
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        name.encode_wide().next() == Some(u16::from(b'.'))
    }
    #[cfg(not(any(unix, windows)))]
    {
        name.as_encoded_bytes().first() == Some(&b'.')
    }
}

pub(super) fn sort_entries(files: &mut [FEntry], sort: usize) {
    files.sort_by(|x, y| match sort {
        1 => match (x.is_dir, y.is_dir) {
            (true, true) => x.label.cmp(&y.label),
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            (false, false) => y.size.cmp(&x.size).then(x.label.cmp(&y.label)),
        },
        2 => y.modified.cmp(&x.modified).then(x.label.cmp(&y.label)),
        3 => y.is_dir.cmp(&x.is_dir).then(x.label.cmp(&y.label)),
        _ => x.label.cmp(&y.label),
    });
}
