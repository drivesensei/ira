use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::services::list_files::FEntry;

/// The walk reports a progress event every this many files, so a slow
/// (cold spinning disk) walk shows a live item count instead of a frozen
/// "calculating…" line.
const PROGRESS_STEP: u64 = 512;

/// A folder's recursive size, measured by a background worker. `on_disk` is
/// the allocated size (cluster-rounded; matches what file managers call
/// "size on disk"); Unix only, `0` elsewhere.
#[derive(Debug, Clone, Copy)]
pub struct DirSize {
    pub bytes: u64,
    pub items: u64,
    pub on_disk: u64,
}

/// Cached measurement state for a folder, kept in `App` so sizes survive
/// dialog dismissal and are re-shown instantly on re-query.
#[derive(Debug, Clone, Copy)]
pub struct SizeInfo {
    pub bytes: u64,
    pub items: u64,
    pub on_disk: u64,
    /// `true` once the walk finished; `false` = partial lower bound.
    pub complete: bool,
    /// Last update (progress tick or completion) — shown as "last updated".
    pub updated: SystemTime,
}

/// Allocated size of a file/dir in bytes (`st_blocks`); 0 off-Unix.
#[cfg(unix)]
fn on_disk_of(meta: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks() * 512
}
#[cfg(not(unix))]
fn on_disk_of(_meta: &fs::Metadata) -> u64 {
    0
}

/// Public wrapper for callers outside this module (multi-selection worker).
pub fn on_disk_bytes(meta: &fs::Metadata) -> u64 {
    on_disk_of(meta)
}

/// `"{data} data / {on-disk} on disk"` — the on-disk half only appears when
/// allocation meaningfully exceeds the data (cluster slack, e.g. exFAT).
fn size_with_on_disk(bytes: u64, on_disk: u64) -> String {
    if on_disk > bytes {
        format!("{} data / {} on disk", human(bytes), human(on_disk))
    } else {
        human(bytes)
    }
}

/// Braille spinner frames, advanced on elapsed time; shown while a folder's
/// size is measured in the background.
const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Loading glyph for `started`; advances every 500 ms, i.e. once per UI tick.
pub fn spinner_char(started: Instant) -> char {
    SPINNER_FRAMES[(started.elapsed().as_millis() / 500) as usize % SPINNER_FRAMES.len()]
}

/// Cancellation flag shared with the background info-worker thread.
#[derive(Clone)]
pub struct WalkHandle(Arc<AtomicBool>);

impl Default for WalkHandle {
    fn default() -> Self {
        WalkHandle(Arc::new(AtomicBool::new(false)))
    }
}

impl WalkHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Messages from background threads. Walk threads (one per folder being
/// measured) stream `Progress` and finish with `Done`; the dialog's
/// metadata worker (one per `?` press) sends `Meta`.
#[derive(Debug)]
pub enum InfoEvent {
    Progress {
        path: String,
        bytes: u64,
        items: u64,
        on_disk: u64,
    },
    Done {
        path: String,
        size: DirSize,
    },
    Meta {
        path: String,
        lines: Vec<String>,
    },
}

/// Size line while the walk is still running (spinner char animated by the
/// renderer): a live lower bound.
pub fn size_line_partial(si: &SizeInfo, spinner: char) -> String {
    format!(
        "Size: {} {} ({} items) — calculating…",
        spinner,
        size_with_on_disk(si.bytes, si.on_disk),
        si.items
    )
}

/// Size line before the first progress tick arrives.
pub fn size_line_started(spinner: char) -> String {
    format!("Size: {spinner} calculating…")
}

/// Size line after the walk was cancelled: an honest lower bound.
pub fn size_line_cancelled(si: &SizeInfo) -> String {
    format!(
        "Size: ≥{} ({} items, partial)",
        size_with_on_disk(si.bytes, si.on_disk),
        si.items
    )
}

/// Final size line for the dialog.
pub fn size_line_final(si: &SizeInfo) -> String {
    format!(
        "Size: {} ({} items)",
        size_with_on_disk(si.bytes, si.on_disk),
        si.items
    )
}

/// Folder-name annotation for the file list:
/// `cybertouch (3.0 GiB data / 123 GiB on disk - last updated: 2026-09-03 14:45)`.
pub fn list_note(si: &SizeInfo) -> String {
    format!(
        "{} - last updated: {}",
        size_with_on_disk(si.bytes, si.on_disk),
        short_date(si.updated)
    )
}
/// `YYYY-MM-DD HH:MM` (UTC) civil-calendar conversion, shared with `date`.
fn short_date(st: SystemTime) -> String {
    let Ok(dur) = st.duration_since(UNIX_EPOCH) else {
        return "before 1970".to_string();
    };
    let secs = dur.as_secs() as i64;
    let days = secs.div_euclid(86400);
    let h = secs.rem_euclid(86400) / 3600;
    let m = secs.rem_euclid(3600) / 60;

    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if mo <= 2 { y + 1 } else { y };

    format!("{year:04}-{mo:02}-{d:02} {h:02}:{m:02}")
}

/// Fast metadata lines that need no filesystem access; the dialog shows
/// these immediately, then the worker's `Meta` event replaces them with
/// the full set.
pub fn build_info_fast(entry: &FEntry) -> Vec<String> {
    vec![
        format!("Name: {}", entry.label),
        format!("Path: {}", entry.path),
        format!("Kind: {}", kind_of(entry)),
        format!(
            "Hidden: {}",
            if entry.label.starts_with('.') {
                "yes"
            } else {
                "no"
            }
        ),
    ]
}

/// Full metadata lines for a file or folder, including the filesystem stat
/// (and a folder's recursive size when known). Runs on the worker thread.
pub fn build_info_full(entry: &FEntry, dir_size: Option<DirSize>) -> Vec<String> {
    let mut lines: Vec<String> = vec![
        format!("Name: {}", entry.label),
        format!("Path: {}", entry.path),
        format!("Kind: {}", kind_of(entry)),
        format!(
            "Hidden: {}",
            if entry.label.starts_with('.') {
                "yes"
            } else {
                "no"
            }
        ),
    ];

    match fs::metadata(&entry.path) {
        Ok(meta) => {
            if entry.is_dir {
                // Folder sizes are injected dynamically by the app from its
                // size cache (partial while walking); the static lines omit
                // the Size line for folders. A caller may pass a final
                // measured size to embed directly.
                if let Some(s) = dir_size {
                    lines.push(format!(
                        "Size: {} ({} items)",
                        size_with_on_disk(s.bytes, s.on_disk),
                        s.items
                    ));
                }
            } else {
                lines.push(format!(
                    "Size: {} ({} bytes)",
                    human(meta.len()),
                    meta.len()
                ));
            }
            lines.push(format!("Added: {}", created_or_modified(&meta)));
            lines.push(format!("Modified: {}", date_result(meta.modified())));
        }
        Err(_) => lines.push("Error reading metadata".to_string()),
    }

    lines
}

fn kind_of(entry: &FEntry) -> String {
    if entry.is_dir {
        return "Folder".to_string();
    }
    let ext = extension(&entry.label).to_lowercase();
    let kind: &str = match ext.as_str() {
        "txt" | "md" | "rst" | "log" | "conf" | "ini" | "toml" | "yml" | "yaml" => "Text file",
        "rs" => "Rust source",
        "py" => "Python source",
        "js" | "ts" | "mjs" | "cjs" => "JavaScript/TS source",
        "json" => "JSON document",
        "html" | "htm" | "css" => "Web document",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "avif" | "ico" => "Image",
        "mp4" | "mkv" | "avi" | "mov" | "webm" => "Video",
        "mp3" | "flac" | "ogg" | "wav" | "m4a" | "aac" => "Audio",
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst" => "Archive",
        "pdf" => "PDF document",
        "doc" | "docx" | "odt" | "rtf" => "Word document",
        "xls" | "xlsx" | "ods" | "csv" | "tsv" => "Spreadsheet",
        "iso" | "img" => "Disk image",
        "exe" | "msi" | "bin" | "dmg" | "sh" => "Executable",
        "sqlite" | "db" | "sqlite3" => "Database",
        "git" => "Git repository",
        _ => "File",
    };
    kind.to_string()
}

fn extension(name: &str) -> &str {
    let mut dot: Option<usize> = None;
    for (i, c) in name.chars().enumerate() {
        if c == '.' {
            dot = Some(i);
        }
    }
    match dot {
        Some(i) if i + 1 < name.chars().count() => &name[i + 1..],
        _ => "",
    }
}

/// "Added": creation time when the platform reports it (Windows/macOS), else
/// the modification time (Linux has no birth time).
fn created_or_modified(meta: &fs::Metadata) -> String {
    match meta.created() {
        Ok(st) => date(st),
        Err(_) => date_result(meta.modified()),
    }
}

fn date_result(r: io::Result<SystemTime>) -> String {
    match r {
        Ok(st) => date(st),
        Err(_) => "unknown".to_string(),
    }
}

/// Formats a `SystemTime` as `YYYY-MM-DD HH:MM:SS UTC` using a dependency-free
/// civil-calendar conversion (Howard Hinnant's `civil_from_days`).
fn date(st: SystemTime) -> String {
    let Ok(dur) = st.duration_since(UNIX_EPOCH) else {
        return "before 1970".to_string();
    };
    let secs = dur.as_secs() as i64;
    let days = secs.div_euclid(86400);
    let h = secs.rem_euclid(86400) / 3600;
    let m = secs.rem_euclid(3600) / 60;
    let s = secs.rem_euclid(60);

    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if mo <= 2 { y + 1 } else { y };

    format!("{year:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
}

/// Recursively sums a directory's data size, file count and allocated size.
/// Unbounded by design: huge trees keep measuring (streaming partial
/// progress every [`PROGRESS_STEP`] files) until done or `cancel`led at the
/// next entry.
pub fn dir_size(
    path: &Path,
    cancel: &WalkHandle,
    on_progress: &mut dyn FnMut(u64, u64, u64),
) -> DirSize {
    let mut bytes = 0u64;
    let mut items = 0u64;
    let mut on_disk = 0u64;
    // The root folder's own allocation counts too (exFAT dirs can hold
    // GiBs of stale clusters).
    if let Ok(meta) = fs::metadata(path) {
        on_disk += on_disk_of(&meta);
    }
    walk(
        path,
        &mut bytes,
        &mut items,
        &mut on_disk,
        cancel,
        on_progress,
    );
    DirSize {
        bytes,
        items,
        on_disk,
    }
}

fn walk(
    path: &Path,
    bytes: &mut u64,
    items: &mut u64,
    on_disk: &mut u64,
    cancel: &WalkHandle,
    on_progress: &mut dyn FnMut(u64, u64, u64),
) {
    if cancel.cancelled() {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for e in entries.flatten() {
        if cancel.cancelled() {
            return;
        }
        // file_type() is free on filesystems that fill d_type (ntfs3 does);
        // only regular files need the extra stat for their size, and
        // symlinks are never followed.
        let Ok(ft) = e.file_type() else {
            continue;
        };
        if ft.is_dir() {
            if let Ok(m) = e.metadata() {
                *on_disk += on_disk_of(&m);
            }
            walk(&e.path(), bytes, items, on_disk, cancel, on_progress);
        } else if ft.is_file() {
            if let Ok(meta) = e.metadata() {
                *bytes += meta.len();
                *on_disk += on_disk_of(&meta);
                *items += 1;
                if (*items).is_multiple_of(PROGRESS_STEP) {
                    on_progress(*bytes, *items, *on_disk);
                }
            }
        }
    }
}

pub fn human(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(tmp: &Path, name: &str) -> std::path::PathBuf {
        let root = tmp.join(format!("ira_walk_{}_{}", std::process::id(), name));
        std::fs::create_dir_all(root.join("d")).unwrap();
        std::fs::write(root.join("a"), vec![0u8; 100]).unwrap();
        std::fs::write(root.join("d").join("b"), vec![0u8; 50]).unwrap();
        root
    }

    #[test]
    fn walk_sums_files_recursively() {
        let root = tree(std::env::temp_dir().as_path(), "sums");
        let mut progress = |_bytes: u64, _items: u64, _on_disk: u64| {};
        let size = dir_size(&root, &WalkHandle::new(), &mut progress);
        assert_eq!(size.bytes, 150);
        assert_eq!(size.items, 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn walk_reports_no_progress_below_step() {
        let root = tree(std::env::temp_dir().as_path(), "progress");
        let mut seen: Vec<(u64, u64, u64)> = Vec::new();
        let mut progress =
            |bytes: u64, items: u64, on_disk: u64| seen.push((bytes, items, on_disk));
        dir_size(&root, &WalkHandle::new(), &mut progress);
        assert!(
            seen.is_empty(),
            "2 files never reach PROGRESS_STEP {seen:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cancelled_walk_measures_nothing() {
        let handle = WalkHandle::new();
        handle.cancel();
        let root = tree(std::env::temp_dir().as_path(), "cancel");
        let mut progress = |_bytes: u64, _items: u64, _on_disk: u64| {};
        let size = dir_size(&root, &handle, &mut progress);
        assert!(handle.cancelled());
        // The walk aborts at the first entry; the walk thread in App checks
        // `handle.cancelled()` and never sends the result.
        assert_eq!(size.bytes, 0);
        assert_eq!(size.items, 0);
        let _ = std::fs::remove_dir_all(&root);
    }
}
