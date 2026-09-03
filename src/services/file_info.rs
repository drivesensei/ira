use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::services::list_files::FEntry;

const MAX_ENTRIES: u64 = 100_000;

/// Builds human-readable metadata lines for a file or folder.
pub fn build_info(entry: &FEntry) -> Vec<String> {
    let mut lines: Vec<String> = vec![
        format!("Name: {}", entry.label),
        format!("Path: {}", entry.path),
        format!("Kind: {}", kind_of(entry)),
        format!(
            "Hidden: {}",
            if entry.label.starts_with('.') { "yes" } else { "no" }
        ),
    ];

    match fs::metadata(&entry.path) {
        Ok(meta) => {
            if entry.is_dir {
                let (bytes, items, overflow) = dir_size(Path::new(&entry.path));
                if overflow {
                    lines.push(format!(
                        "Size: >{} ({} items, partial)",
                        human(bytes),
                        items
                    ));
                } else {
                    lines.push(format!(
                        "Size: {} ({} items, {} bytes)",
                        human(bytes),
                        items,
                        bytes
                    ));
                }
            } else {
                lines.push(format!("Size: {} ({} bytes)", human(meta.len()), meta.len()));
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

/// Recursively sums a directory's bytes and entry count, capped so a huge tree
/// can't stall the UI for long.
fn dir_size(path: &Path) -> (u64, u64, bool) {
    let mut bytes = 0u64;
    let mut items = 0u64;
    let mut overflow = false;
    walk(path, &mut bytes, &mut items, &mut overflow);
    (bytes, items, overflow)
}

fn walk(path: &Path, bytes: &mut u64, items: &mut u64, overflow: &mut bool) {
    if *overflow || *items >= MAX_ENTRIES {
        *overflow = true;
        return;
    }
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for e in entries.flatten() {
            walk(&e.path(), bytes, items, overflow);
            if *overflow {
                return;
            }
        }
    } else {
        *bytes += meta.len();
        *items += 1;
    }
}

fn human(bytes: u64) -> String {
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