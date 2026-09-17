//! Profile font lookups from editor / terminal config files.
//!
//! Windows Terminal and VS Code both keep a `settings.json` written in
//! JSONC (comments and trailing commas allowed), so a tolerant pre-pass
//! turns it into strict JSON before `serde_json` parses it. All parsing is
//! pure and tested on every platform; only path discovery is OS-specific.
//!
//! The Windows Terminal size and line-height lookups run on the per-frame
//! overlay draw path (`services::overlay::resolve_cell_px`), so they share
//! one memoized read: the candidate walk, the read and the parse happen at
//! most once per settings-file revision, and later calls cost one
//! `fs::metadata`.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use serde_json::Value;

/// Largest `settings.json` this module reads. Windows Terminal writes a few
/// KiB, so the cap is far past any real file; it exists so a pathological or
/// hostile file cannot be allocated into memory on the draw thread. A larger
/// file is skipped, which degrades to the window-frame cell estimate.
const MAX_SETTINGS_BYTES: u64 = 4 * 1024 * 1024;

/// Environment variable holding the path of a `settings.json` to use ahead
/// of every discovered location.
const SETTINGS_ENV: &str = "IRA_WT_SETTINGS";

/// File that marks a Windows Terminal portable install directory.
const PORTABLE_MARKER: &str = ".portable";

/// `settings.json` locations inside a `%LOCALAPPDATA%`-style root: packaged
/// (Store), Preview, then the unpackaged install.
const WT_LOCAL_REL: [&str; 3] = [
    "Packages/Microsoft.WindowsTerminal_8wekyb3d8bbwe/LocalState/settings.json",
    "Packages/Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe/LocalState/settings.json",
    "Microsoft/Windows Terminal/settings.json",
];

/// Font face of the Windows Terminal profile this process runs in.
///
/// Looks at `IRA_WT_SETTINGS` first, then the packaged (Store), Preview and
/// unpackaged install locations on Windows and, for WSL, the same locations
/// under every `/mnt/c/Users/*` profile, then portable installs. `None` when
/// no settings file is readable.
pub fn windows_terminal_font_face(profile_id: Option<&str>) -> Option<String> {
    windows_terminal_settings_paths()
        .into_iter()
        .filter_map(|p| read_settings_capped(&p))
        .find_map(|(src, _)| wt_profile_font_face(&src, profile_id))
}

/// Point size of the Windows Terminal profile font (`font.size` / `fontSize`).
pub fn windows_terminal_font_size(profile_id: Option<&str>) -> Option<f64> {
    wt_font_metrics(profile_id).0
}

/// Line-box multiplier from `font.lineHeight` / `font.cellHeight` when the
/// value is a simple number or percent. `None` for missing or exotic units.
pub fn windows_terminal_line_height(profile_id: Option<&str>) -> Option<f64> {
    wt_font_metrics(profile_id).1
}

/// `(font size, line height)` of `profile_id`, both from one memoized
/// read+parse of the same settings file.
fn wt_font_metrics(profile_id: Option<&str>) -> (Option<f64>, Option<f64>) {
    cached_wt_metrics(
        wt_settings_cache(),
        windows_terminal_settings_paths,
        profile_id,
    )
}

/// Fingerprint of a settings file revision: the two facts a cheap
/// `fs::metadata` confirms before a cached parse is trusted again.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
}

/// One parsed settings file: where it came from, the revision it was parsed
/// from, and the two values the overlay needs.
struct CachedMetrics {
    path: PathBuf,
    stamp: FileStamp,
    size: Option<f64>,
    line_height: Option<f64>,
}

impl CachedMetrics {
    /// Whether the cached parse still describes what is on disk. Only a
    /// `fs::metadata` call: a vanished or rewritten file invalidates.
    fn is_current(&self) -> bool {
        let Ok(meta) = std::fs::metadata(&self.path) else {
            return false;
        };
        meta.len() == self.stamp.len && meta.modified().ok() == self.stamp.modified
    }
}

/// Memoized settings parses, keyed by profile id. The draw path is the only
/// writer, so the lock is held across the read: a second parse of the same
/// revision is exactly what the memo exists to avoid.
fn wt_settings_cache() -> &'static Mutex<HashMap<Option<String>, CachedMetrics>> {
    static CACHE: LazyLock<Mutex<HashMap<Option<String>, CachedMetrics>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    &CACHE
}

/// `(size, line height)` for `profile_id`, from at most one read+parse of
/// the first candidate in `paths` that carries a value for it.
///
/// A cached entry is trusted while its `(mtime, len)` fingerprint matches, so
/// a repeat call in the same frame costs one `fs::metadata`, and `paths` is
/// not even called. A rewrite that preserves both the length and the mtime is
/// not noticed — an edit to a settings file changes its mtime. A miss is not
/// cached: a settings file written or deleted after start is picked up by the
/// next call.
fn cached_wt_metrics(
    cache: &Mutex<HashMap<Option<String>, CachedMetrics>>,
    paths: impl FnOnce() -> Vec<PathBuf>,
    profile_id: Option<&str>,
) -> (Option<f64>, Option<f64>) {
    let key = profile_id.map(str::to_string);
    let mut entries = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = entries.get(&key).filter(|hit| hit.is_current()) {
        return (hit.size, hit.line_height);
    }
    let Some(fresh) = load_wt_metrics(&paths(), profile_id) else {
        entries.remove(&key);
        return (None, None);
    };
    let (size, line_height) = (fresh.size, fresh.line_height);
    entries.insert(key, fresh);
    (size, line_height)
}

/// Reads and parses settings candidates until one carries a value for
/// `profile_id`. A parseable file that carries nothing is returned only when
/// no candidate carries anything, so a settings file without `font.size` is
/// still cached instead of re-read every frame.
fn load_wt_metrics(paths: &[PathBuf], profile_id: Option<&str>) -> Option<CachedMetrics> {
    let mut parseable: Option<CachedMetrics> = None;
    for path in paths {
        let Some((src, stamp)) = read_settings_capped(path) else {
            continue;
        };
        let Some(v) = parse_jsonc(&src) else {
            continue;
        };
        let entry = CachedMetrics {
            path: path.clone(),
            stamp,
            size: wt_font_size_of(&v, profile_id),
            line_height: wt_line_height_of(&v, profile_id),
        };
        if entry.size.is_some() || entry.line_height.is_some() {
            return Some(entry);
        }
        parseable.get_or_insert(entry);
    }
    parseable
}

/// Candidate `settings.json` locations for Windows Terminal, in preference
/// order: an explicit [`SETTINGS_ENV`] path, the `%LOCALAPPDATA%` (and WSL
/// `/mnt/c/Users/*/AppData/Local`) layouts, then portable installs.
fn windows_terminal_settings_paths() -> Vec<PathBuf> {
    let mut local_roots: Vec<PathBuf> = Vec::new();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        local_roots.push(PathBuf::from(local));
    }
    // WSL: the Windows user profiles are mounted under /mnt/c/Users.
    if let Ok(users) = std::fs::read_dir("/mnt/c/Users") {
        for u in users.flatten() {
            local_roots.push(u.path().join("AppData").join("Local"));
        }
    }
    // A portable install keeps its settings beside the executable. The
    // hosting terminal's own executable directory is not reachable from
    // here, so `current_exe` stands in for it: a portable install launched
    // from the same directory as this process is found, anything else simply
    // degrades to the window-frame estimate
    // (`services::overlay::cell_px_from_window`) — never an error.
    let portable_bases: Vec<PathBuf> = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .into_iter()
        .collect();
    wt_settings_candidates(
        std::env::var(SETTINGS_ENV).ok().as_deref(),
        &local_roots,
        &portable_bases,
    )
}

/// Ordered candidate list: the override, then the `%LOCALAPPDATA%` layouts,
/// then portable installs under `portable_bases`. Split out of
/// [`windows_terminal_settings_paths`] so tests drive it with explicit roots
/// instead of process globals.
fn wt_settings_candidates(
    override_path: Option<&str>,
    local_roots: &[PathBuf],
    portable_bases: &[PathBuf],
) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Some(p) = override_path.map(str::trim).filter(|p| !p.is_empty()) {
        paths.push(PathBuf::from(p));
    }
    for root in local_roots {
        paths.extend(WT_LOCAL_REL.iter().map(|rel| root.join(rel)));
    }
    paths.extend(
        portable_bases
            .iter()
            .filter_map(|base| portable_settings_path(base)),
    );
    paths
}

/// `settings/settings.json` of a portable install: Windows Terminal marks a
/// portable directory with a `.portable` file next to the executable. `None`
/// when `base_dir` is not a portable install.
fn portable_settings_path(base_dir: &Path) -> Option<PathBuf> {
    base_dir
        .join(PORTABLE_MARKER)
        .is_file()
        .then(|| base_dir.join("settings").join("settings.json"))
}

/// One settings file, whole, plus the [`FileStamp`] of the revision read.
///
/// `None` when the file is missing, unreadable, not UTF-8, or larger than
/// [`MAX_SETTINGS_BYTES`]. `take` bounds the read itself, so a file that
/// grows between the metadata call and the read is still refused rather than
/// allocated.
fn read_settings_capped(path: &Path) -> Option<(String, FileStamp)> {
    let file = std::fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    if meta.len() > MAX_SETTINGS_BYTES {
        return None;
    }
    let mut src = String::new();
    file.take(MAX_SETTINGS_BYTES + 1)
        .read_to_string(&mut src)
        .ok()?;
    if src.len() as u64 > MAX_SETTINGS_BYTES {
        return None;
    }
    Some((
        src,
        FileStamp {
            modified: meta.modified().ok(),
            len: meta.len(),
        },
    ))
}

/// Parsed settings DOM. `None` for invalid JSON(C): callers treat that as
/// "no settings", never as an error.
fn parse_jsonc(src: &str) -> Option<Value> {
    serde_json::from_str(&strip_jsonc(src)).ok()
}

/// The profile named by `profile_id` (`guid`, case-insensitive) and the
/// `profiles.defaults` object, from a parsed settings DOM. Very old settings
/// files had `profiles` as a bare array, so a list-only document has no
/// defaults.
fn wt_profiles<'a>(
    v: &'a Value,
    profile_id: Option<&str>,
) -> (Option<&'a Value>, Option<&'a Value>) {
    let Some(profiles) = v.get("profiles") else {
        return (None, None);
    };
    let (list, defaults) = match profiles {
        Value::Array(list) => (Some(list), None),
        Value::Object(_) => (
            profiles.get("list").and_then(Value::as_array),
            profiles.get("defaults"),
        ),
        _ => (None, None),
    };
    let hit = match (list, profile_id) {
        (Some(list), Some(id)) => list.iter().find(|p| {
            p.get("guid")
                .and_then(Value::as_str)
                .is_some_and(|g| g.eq_ignore_ascii_case(id))
        }),
        _ => None,
    };
    (hit, defaults)
}

/// `font.face` (1.10+) or legacy flat `fontFace` of one profile object.
fn wt_font_face_of_profile(p: &Value) -> Option<String> {
    p.get("font")
        .and_then(|f| f.get("face"))
        .and_then(Value::as_str)
        .or_else(|| p.get("fontFace").and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `font.size` (1.10+) or legacy `fontSize` of one profile object.
fn wt_font_size_of_profile(p: &Value) -> Option<f64> {
    p.get("font")
        .and_then(|f| f.get("size"))
        .and_then(Value::as_f64)
        .or_else(|| p.get("fontSize").and_then(Value::as_f64))
        .filter(|s| *s > 0.0)
}

/// `font.lineHeight` then `font.cellHeight` of one profile object.
fn wt_line_height_of_profile(p: &Value) -> Option<f64> {
    let font = p.get("font")?;
    font.get("lineHeight")
        .and_then(parse_wt_line_height)
        .or_else(|| font.get("cellHeight").and_then(parse_wt_line_height))
}

/// Font face for `profile_id`, else `profiles.defaults`, from a parsed DOM.
fn wt_font_face_of(v: &Value, profile_id: Option<&str>) -> Option<String> {
    let (hit, defaults) = wt_profiles(v, profile_id);
    hit.and_then(wt_font_face_of_profile)
        .or_else(|| defaults.and_then(wt_font_face_of_profile))
}

/// `font.size` for `profile_id`, else `profiles.defaults`, from a parsed DOM.
fn wt_font_size_of(v: &Value, profile_id: Option<&str>) -> Option<f64> {
    let (hit, defaults) = wt_profiles(v, profile_id);
    hit.and_then(wt_font_size_of_profile)
        .or_else(|| defaults.and_then(wt_font_size_of_profile))
}

/// Line height for `profile_id`, else `profiles.defaults`, from a parsed DOM.
fn wt_line_height_of(v: &Value, profile_id: Option<&str>) -> Option<f64> {
    let (hit, defaults) = wt_profiles(v, profile_id);
    hit.and_then(wt_line_height_of_profile)
        .or_else(|| defaults.and_then(wt_line_height_of_profile))
}

/// Resolves the font face from a Windows Terminal `settings.json` body:
/// the profile whose `guid` matches `profile_id` (case-insensitive), else
/// `profiles.defaults`. Both the `font.face` object form (1.10+) and the
/// legacy flat `fontFace` key are understood.
pub fn wt_profile_font_face(jsonc: &str, profile_id: Option<&str>) -> Option<String> {
    wt_font_face_of(&parse_jsonc(jsonc)?, profile_id)
}

/// `font.size` (1.10+) or legacy `fontSize`, profile then `profiles.defaults`.
pub fn wt_profile_font_size(jsonc: &str, profile_id: Option<&str>) -> Option<f64> {
    wt_font_size_of(&parse_jsonc(jsonc)?, profile_id)
}

/// Simple line-box multiplier from `font.lineHeight` or `font.cellHeight`.
/// JSON number, numeric string ("1.2"), or percent ("120%") only — px/pt/ch
/// and keywords ("leading") are ignored rather than guessed.
pub fn parse_wt_line_height(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().filter(|x| *x > 0.0 && *x < 10.0),
        Value::String(s) => parse_wt_line_height_str(s),
        _ => None,
    }
}

fn parse_wt_line_height_str(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty()
        || s.eq_ignore_ascii_case("leading")
        || s.eq_ignore_ascii_case("default")
        || s.eq_ignore_ascii_case("none")
    {
        return None;
    }
    if let Some(pct) = s.strip_suffix('%') {
        let n: f64 = pct.trim().parse().ok()?;
        let m = n / 100.0;
        return (m > 0.0 && m < 10.0).then_some(m);
    }
    if s.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let n: f64 = s.parse().ok()?;
    (n > 0.0 && n < 10.0).then_some(n)
}

/// `font.lineHeight` then `font.cellHeight`, profile then `profiles.defaults`.
pub fn wt_profile_line_height(jsonc: &str, profile_id: Option<&str>) -> Option<f64> {
    wt_line_height_of(&parse_jsonc(jsonc)?, profile_id)
}

/// Font family of the VS Code-family integrated terminal hosting this
/// process (`TERM_PROGRAM=vscode` is set by VS Code, Cursor, VSCodium...).
pub fn vscode_terminal_font_family() -> Option<String> {
    vscode_settings_paths()
        .into_iter()
        .filter_map(|p| read_settings_capped(&p))
        .find_map(|(src, _)| vscode_font_family(&src))
}

/// User `settings.json` of the VS Code-family editors, in preference
/// order. `dirs_next::config_dir()` is `%APPDATA%` on Windows,
/// `~/Library/Application Support` on macOS and `~/.config` on Linux,
/// which is where all of them keep `User/settings.json`.
fn vscode_settings_paths() -> Vec<PathBuf> {
    let Some(cfg) = dirs_next::config_dir() else {
        return Vec::new();
    };
    ["Code", "Code - Insiders", "Cursor", "VSCodium"]
        .iter()
        .map(|app| cfg.join(app).join("User").join("settings.json"))
        .collect()
}

/// `terminal.integrated.fontFamily`, falling back to `editor.fontFamily`
/// (VS Code uses the editor font for the terminal when unset).
pub fn vscode_font_family(jsonc: &str) -> Option<String> {
    let v: Value = serde_json::from_str(&strip_jsonc(jsonc)).ok()?;
    ["terminal.integrated.fontFamily", "editor.fontFamily"]
        .iter()
        .find_map(|k| {
            v.get(*k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
}

/// Turns JSONC into strict JSON: drops `//` and `/* */` comments outside
/// strings and commas that directly precede `}` / `]`.
pub fn strip_jsonc(src: &str) -> String {
    // A BOM is invisible in an editor but `serde_json` rejects the whole
    // document over it, so it is dropped before anything else.
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    // Pass 1: comments.
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_str = false;
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            _ => out.push(c),
        }
    }
    // Pass 2: trailing commas.
    let bytes: Vec<char> = out.chars().collect();
    let mut result = String::with_capacity(out.len());
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            result.push(c);
            if c == '\\' && i + 1 < bytes.len() {
                result.push(bytes[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                result.push(c);
            }
            ',' => {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && matches!(bytes[j], '}' | ']') {
                    // Drop the comma; the closer is emitted on its turn.
                } else {
                    result.push(c);
                }
            }
            _ => result.push(c),
        }
        i += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const WT_SETTINGS: &str = r#"
    // This file was initially generated by Windows Terminal
    {
        "$help": "https://aka.ms/terminal-documentation",
        "defaultProfile": "{61c54bbd-c2c6-5271-96e7-009a87ff44bf}",
        /* block
           comment */
        "profiles":
        {
            "defaults": {
                "font": { "face": "Cascadia Mono", "size": 11, },
            },
            "list":
            [
                {
                    "guid": "{61c54bbd-c2c6-5271-96e7-009a87ff44bf}",
                    "name": "Windows PowerShell",
                    "font": { "face": "JetBrainsMono Nerd Font" },
                },
                {
                    "guid": "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}",
                    "name": "Command Prompt // not a comment",
                    "fontFace": "Consolas",
                },
                {
                    "guid": "{2c4de342-38b7-51cf-b940-2309a097f518}",
                    "name": "Ubuntu",
                },
            ],
        },
    }
    "#;

    /// Empty, per-process directory for a test's settings tree.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ira_wt_font_{}_{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes `body`, then stamps `modified` on it (the file system's own
    /// value is kept for `None`) so cache invalidation is deterministic.
    fn write_stamped(path: &Path, body: &str, modified: Option<SystemTime>) {
        std::fs::write(path, body).unwrap();
        if let Some(t) = modified {
            std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(t)
                .unwrap();
        }
    }

    fn mtime(path: &Path) -> SystemTime {
        std::fs::metadata(path).unwrap().modified().unwrap()
    }

    /// Restores an environment variable when the test ends, pass or fail.
    struct EnvGuard {
        key: &'static str,
        was: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let was = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, was }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.was.take() {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn strip_jsonc_removes_comments_and_trailing_commas_only_outside_strings() {
        let v: Value = serde_json::from_str(&strip_jsonc(WT_SETTINGS)).expect("valid JSON");
        assert_eq!(
            v["profiles"]["list"][1]["name"],
            "Command Prompt // not a comment"
        );
        assert_eq!(v["profiles"]["defaults"]["font"]["size"], 11);
        let escaped = strip_jsonc(r#"{"a": "quote \" then // no comment", }"#);
        let v: Value = serde_json::from_str(&escaped).unwrap();
        assert_eq!(v["a"], "quote \" then // no comment");
    }

    #[test]
    fn wt_face_prefers_the_active_profile_then_defaults() {
        assert_eq!(
            wt_profile_font_face(WT_SETTINGS, Some("{61C54BBD-C2C6-5271-96E7-009A87FF44BF}")),
            Some("JetBrainsMono Nerd Font".into())
        );
        // Legacy flat key.
        assert_eq!(
            wt_profile_font_face(WT_SETTINGS, Some("{0caa0dad-35be-5f56-a8ff-afceeeaa6101}")),
            Some("Consolas".into())
        );
        // Profile without a font: defaults apply.
        assert_eq!(
            wt_profile_font_face(WT_SETTINGS, Some("{2c4de342-38b7-51cf-b940-2309a097f518}")),
            Some("Cascadia Mono".into())
        );
        // Unknown / missing profile id: defaults.
        assert_eq!(
            wt_profile_font_face(WT_SETTINGS, None),
            Some("Cascadia Mono".into())
        );
        // No font anywhere: None, caller assumes the stock font.
        assert_eq!(
            wt_profile_font_face(r#"{"profiles": {"list": [{"guid": "{x}"}]}}"#, Some("{x}")),
            None
        );
        assert_eq!(wt_profile_font_face("not json", None), None);
    }

    #[test]
    fn wt_size_prefers_the_active_profile_then_defaults() {
        // Profile font object has no size → defaults 11.
        assert_eq!(
            wt_profile_font_size(WT_SETTINGS, Some("{61C54BBD-C2C6-5271-96E7-009A87FF44BF}")),
            Some(11.0)
        );
        assert_eq!(wt_profile_font_size(WT_SETTINGS, None), Some(11.0));
        let sized = r#"{"profiles":{"defaults":{"font":{"size":12}},"list":[{"guid":"{a}","font":{"size":14}}]}}"#;
        assert_eq!(wt_profile_font_size(sized, Some("{a}")), Some(14.0));
        let legacy = r#"{"profiles":[{"guid":"{a}","fontSize":16}]}"#;
        assert_eq!(wt_profile_font_size(legacy, Some("{a}")), Some(16.0));
        assert_eq!(wt_profile_font_size("not json", None), None);
    }

    #[test]
    fn wt_line_height_parses_simple_values_only() {
        assert_eq!(parse_wt_line_height(&serde_json::json!(1.2)), Some(1.2));
        assert_eq!(parse_wt_line_height(&serde_json::json!("1.2")), Some(1.2));
        assert_eq!(parse_wt_line_height(&serde_json::json!("120%")), Some(1.2));
        assert_eq!(parse_wt_line_height(&serde_json::json!("leading")), None);
        assert_eq!(parse_wt_line_height(&serde_json::json!("12px")), None);
        assert_eq!(parse_wt_line_height(&serde_json::json!("12pt")), None);
        assert_eq!(parse_wt_line_height(&serde_json::json!("1ch")), None);
        let profile = r#"{"profiles":{"defaults":{"font":{"size":12,"cellHeight":"1.0"}},"list":[{"guid":"{a}","font":{"lineHeight":1.4}}]}}"#;
        assert_eq!(wt_profile_line_height(profile, Some("{a}")), Some(1.4));
        assert_eq!(wt_profile_line_height(profile, None), Some(1.0));
        assert_eq!(wt_profile_line_height(WT_SETTINGS, None), None);
    }

    #[test]
    fn wt_face_handles_the_legacy_bare_array_layout() {
        let legacy = r#"{"profiles": [{"guid": "{a}", "fontFace": "Cascadia Code NF"}]}"#;
        assert_eq!(
            wt_profile_font_face(legacy, Some("{a}")),
            Some("Cascadia Code NF".into())
        );
    }

    #[test]
    fn vscode_terminal_font_falls_back_to_editor_font() {
        let both = r#"{
            // comment
            "editor.fontFamily": "Consolas",
            "terminal.integrated.fontFamily": "'Hack Nerd Font Mono'",
        }"#;
        assert_eq!(
            vscode_font_family(both),
            Some("'Hack Nerd Font Mono'".into())
        );
        let editor_only = r#"{"editor.fontFamily": "Fira Code"}"#;
        assert_eq!(vscode_font_family(editor_only), Some("Fira Code".into()));
        assert_eq!(
            vscode_font_family(r#"{"terminal.integrated.fontFamily": ""}"#),
            None
        );
        assert_eq!(vscode_font_family("{}"), None);
    }

    #[test]
    fn strip_jsonc_drops_a_leading_bom() {
        let bom = format!(
            "\u{feff}{}",
            r#"{"profiles":{"defaults":{"font":{"size":13,"face":"Cascadia Mono"}}}}"#
        );
        assert!(!strip_jsonc(&bom).starts_with('\u{feff}'));
        // Every lookup goes through this parse, so all three recover.
        assert!(parse_jsonc(&bom).is_some());
        assert_eq!(wt_profile_font_size(&bom, None), Some(13.0));
        assert_eq!(
            wt_profile_font_face(&bom, None),
            Some("Cascadia Mono".into())
        );
        // Only the leading BOM is a marker; the same character inside a
        // string is content and survives.
        let inner = "{\"a\":\"\u{feff}\"}";
        assert!(strip_jsonc(inner).contains('\u{feff}'));
        assert!(parse_jsonc(inner).is_some());
    }

    #[test]
    fn settings_env_override_is_consulted_first() {
        let dir = temp_dir("env_override");
        let path = dir.join("settings.json");
        write_stamped(
            &path,
            r#"{"profiles":{"list":[{"guid":"{ira-wt-env-override}","font":{"size":15,"lineHeight":1.25}}]}}"#,
            None,
        );
        let _guard = EnvGuard::set(SETTINGS_ENV, &path);
        // The override is the only file that knows this profile, so a value
        // can only have come from it.
        assert_eq!(
            windows_terminal_font_size(Some("{ira-wt-env-override}")),
            Some(15.0)
        );
        assert_eq!(
            windows_terminal_line_height(Some("{ira-wt-env-override}")),
            Some(1.25)
        );
    }

    #[test]
    fn settings_candidates_order_override_then_localappdata_then_portable() {
        let base = temp_dir("candidates");
        let local = base.join("Local");
        let portable = base.join("wt-portable");
        std::fs::create_dir_all(portable.join("settings")).unwrap();
        std::fs::write(portable.join(PORTABLE_MARKER), b"").unwrap();
        let override_path = base.join("explicit.json");
        let paths = wt_settings_candidates(
            Some(override_path.to_str().unwrap()),
            std::slice::from_ref(&local),
            std::slice::from_ref(&portable),
        );
        assert_eq!(paths.first(), Some(&override_path));
        let expected_local: Vec<PathBuf> = WT_LOCAL_REL.iter().map(|rel| local.join(rel)).collect();
        assert_eq!(paths[1..1 + expected_local.len()].to_vec(), expected_local);
        assert_eq!(
            paths.last(),
            Some(&portable.join("settings").join("settings.json"))
        );
        // A blank override is not a path; nothing else is configured.
        assert_eq!(
            wt_settings_candidates(Some("  "), &[], &[]),
            Vec::<PathBuf>::new()
        );
    }

    #[test]
    fn portable_install_needs_the_marker_file() {
        let base = temp_dir("portable");
        let settings = base.join("settings");
        std::fs::create_dir_all(&settings).unwrap();
        std::fs::write(settings.join("settings.json"), "{}").unwrap();
        // Same layout without the marker: not a portable install.
        assert_eq!(
            wt_settings_candidates(None, &[], std::slice::from_ref(&base)),
            Vec::<PathBuf>::new()
        );
        std::fs::write(base.join(PORTABLE_MARKER), b"").unwrap();
        assert_eq!(
            wt_settings_candidates(None, &[], &[base]),
            vec![settings.join("settings.json")]
        );
    }

    #[test]
    fn oversized_settings_files_are_skipped_not_read() {
        let dir = temp_dir("oversized");
        let small = dir.join("small.json");
        std::fs::write(&small, r#"{"profiles":{"defaults":{"font":{"size":11}}}}"#).unwrap();
        let big = dir.join("big.json");
        std::fs::write(&big, vec![b' '; MAX_SETTINGS_BYTES as usize + 1]).unwrap();

        let (src, stamp) = read_settings_capped(&small).expect("a normal file is read");
        assert!(src.starts_with('{'));
        assert_eq!(stamp.len, std::fs::metadata(&small).unwrap().len());
        assert!(read_settings_capped(&big).is_none());

        // Past the cap the file is skipped, not parsed, and the walk simply
        // continues to the next candidate.
        let cache = Mutex::new(HashMap::new());
        assert_eq!(
            cached_wt_metrics(&cache, || vec![big.clone()], None),
            (None, None)
        );
        assert_eq!(
            cached_wt_metrics(&cache, || vec![big, small], None),
            (Some(11.0), None)
        );
    }

    #[test]
    fn settings_cache_reuses_one_parse_and_revalidates_on_mtime_and_len() {
        let dir = temp_dir("memo");
        let path = dir.join("settings.json");
        let a = r#"{"profiles":{"defaults":{"font":{"size":11,"lineHeight":1.3}}}}"#;
        let b = r#"{"profiles":{"defaults":{"font":{"size":22,"lineHeight":2.2}}}}"#;
        let c = r#"{"profiles":{"defaults":{"font":{"size":111,"lineHeight":1.3}}}}"#;
        let d = r#"{"profiles":{"defaults":{"font":{"size":222,"lineHeight":9.9}}}}"#;
        assert_eq!(
            a.len(),
            b.len(),
            "length-only invalidation needs equal lengths"
        );
        assert_eq!(
            c.len(),
            d.len(),
            "mtime-only invalidation needs equal lengths"
        );
        write_stamped(&path, a, None);
        let stamp = mtime(&path);

        let cache = Mutex::new(HashMap::new());
        assert_eq!(
            cached_wt_metrics(&cache, || vec![path.clone()], None),
            (Some(11.0), Some(1.3))
        );
        // A hit never re-resolves the candidates and never re-reads: the
        // closure panics if the memo misses.
        let hit = || panic!("memo hit must not resolve paths");
        assert_eq!(
            cached_wt_metrics(&cache, hit, None),
            (Some(11.0), Some(1.3))
        );
        // Same fingerprint, different bytes: still a hit, still the first
        // parse.
        write_stamped(&path, b, Some(stamp));
        assert_eq!(
            cached_wt_metrics(&cache, hit, None),
            (Some(11.0), Some(1.3))
        );
        // Length changed with the mtime held: invalidated.
        write_stamped(&path, c, Some(stamp));
        assert_eq!(
            cached_wt_metrics(&cache, || vec![path.clone()], None),
            (Some(111.0), Some(1.3))
        );
        // Mtime changed with the length held: invalidated.
        write_stamped(&path, d, Some(stamp + Duration::from_secs(2)));
        assert_eq!(
            cached_wt_metrics(&cache, || vec![path.clone()], None),
            (Some(222.0), Some(9.9))
        );
        // Deleted file: invalidated, and the miss stays uncached.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            cached_wt_metrics(&cache, || vec![path.clone()], None),
            (None, None)
        );
    }

    #[test]
    fn settings_cache_is_keyed_by_profile_id() {
        let dir = temp_dir("per_profile");
        let path = dir.join("settings.json");
        write_stamped(
            &path,
            r#"{"profiles":{"defaults":{"font":{"size":11,"lineHeight":1.4}},"list":[{"guid":"{a}","font":{"size":14,"lineHeight":1.1}}]}}"#,
            None,
        );
        let cache = Mutex::new(HashMap::new());
        assert_eq!(
            cached_wt_metrics(&cache, || vec![path.clone()], Some("{a}")),
            (Some(14.0), Some(1.1))
        );
        assert_eq!(
            cached_wt_metrics(&cache, || vec![path.clone()], None),
            (Some(11.0), Some(1.4))
        );
        // Both entries answer from the one cached parse.
        assert_eq!(
            cached_wt_metrics(
                &cache,
                || panic!("memo hit must not resolve paths"),
                Some("{a}")
            ),
            (Some(14.0), Some(1.1))
        );
    }
}
