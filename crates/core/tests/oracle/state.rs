// Frozen source-derived oracle, base 25ffceb; test bodies removed only.
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::data::Folder;

/// One persisted folder-size measurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeEntry {
    pub path: String,
    pub bytes: u64,
    pub items: u64,
    pub on_disk: u64,
    pub complete: bool,
    /// Epoch seconds of the last update.
    pub updated_epoch: u64,
}

/// The persisted session state: split layout, each pane's folder, and the
/// folder-size cache (complete measurements only).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionState {
    pub split: bool,
    pub active_pane: usize,
    /// Whether hidden entries (dotfiles) are listed.
    pub show_hidden: bool,
    pub left: Option<Folder>,
    pub right: Option<Folder>,
    /// Per-pane preview mode (0=off, 1=column, 2=grid, 3=details).
    pub preview: [u8; 2],
    /// Last-selected theme preset id (`\` cycles). `None` = never
    /// persisted; `theme.toml` / default apply.
    pub theme: Option<String>,
    pub sizes: Vec<SizeEntry>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            split: false,
            active_pane: 0,
            show_hidden: false,
            left: None,
            right: None,
            // Details (3) is the default mode: a fresh session — and a state
            // file written before `preview=` existed — opens with the size /
            // modified columns rather than the bare list.
            preview: [3, 3],
            theme: None,
            sizes: Vec::new(),
        }
    }
}

/// Path to the session-state file (`~/.config/ira/state`).
fn state_file() -> Option<PathBuf> {
    dirs_next::config_dir().map(|d| d.join("ira").join("state"))
}

/// Loads the persisted session state, if any.
pub fn load_state() -> SessionState {
    let Some(path) = state_file() else {
        return SessionState::default();
    };
    load_state_from(&path)
}

/// Loads the session state from an explicit path (tests use a temp file).
pub fn load_state_from(path: &Path) -> SessionState {
    let Ok(contents) = fs::read_to_string(path) else {
        return SessionState::default();
    };
    parse_state(&contents)
}

/// Persists the session state as `key=value` lines.
pub fn save_state(state: &SessionState) {
    let Some(path) = state_file() else {
        return;
    };
    save_state_to(&path, state);
}

/// Saves the session state to an explicit path (tests use a temp file).
pub fn save_state_to(path: &Path, state: &SessionState) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, serialize_state(state));
}

/// Renders the state as `key=value` lines. Size entries take the form
/// `size=<bytes>\t<items>\t<on_disk>\t<complete 0|1>\t<updated_epoch>\t<path>`
/// with the path LAST (it may contain spaces and `=`, never tab/newline).
fn serialize_state(state: &SessionState) -> String {
    let mut content = String::new();
    content.push_str(&format!("split={}\n", state.split as u8));
    content.push_str(&format!("active={}\n", state.active_pane));
    content.push_str(&format!("hidden={}\n", state.show_hidden as u8));
    content.push_str(&format!("preview0={}\n", state.preview[0]));
    content.push_str(&format!("preview1={}\n", state.preview[1]));
    if let Some(theme) = &state.theme {
        content.push_str(&format!("theme={theme}\n"));
    }
    for (key, folder) in [("left", &state.left), ("right", &state.right)] {
        match folder {
            Some(f) => content.push_str(&format!("{}={}\t{}\n", key, f.label, f.path)),
            None => content.push_str(&format!("{}={}\n", key, "")),
        }
    }
    for e in &state.sizes {
        content.push_str(&format!(
            "size={}\t{}\t{}\t{}\t{}\t{}\n",
            e.bytes, e.items, e.on_disk, e.complete as u8, e.updated_epoch, e.path
        ));
    }
    content
}

/// Parses persisted `key=value` lines; malformed lines are ignored.
fn parse_state(contents: &str) -> SessionState {
    let mut state = SessionState::default();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "split" => state.split = value == "1",
            "active" => state.active_pane = value.parse::<usize>().unwrap_or(0).min(1),
            "hidden" => state.show_hidden = value == "1",
            "preview0" => state.preview[0] = value.parse::<u8>().unwrap_or(0).min(3),
            "preview1" => state.preview[1] = value.parse::<u8>().unwrap_or(0).min(3),
            // `icons=` lines written by releases <= 0.1.7 fall through to the
            // ignore arm: the icon set is re-detected from fonts each launch.
            "theme" => {
                // Store the canonical id so aliases normalize on the next save.
                if let Some(p) = crate::theme::ThemePreset::parse(value) {
                    state.theme = Some(p.id().to_string());
                }
            }
            "left" => state.left = parse_folder(value),
            "right" => state.right = parse_folder(value),
            "size" => {
                if let Some(entry) = parse_size_entry(value) {
                    state.sizes.push(entry);
                }
            }
            _ => {}
        }
    }
    state
}

/// Parses the fields after `size=`; the path is the last tab-separated field.
fn parse_size_entry(value: &str) -> Option<SizeEntry> {
    let mut parts = value.splitn(6, '\t');
    let bytes = parts.next()?.parse().ok()?;
    let items = parts.next()?.parse().ok()?;
    let on_disk = parts.next()?.parse().ok()?;
    let complete = match parts.next()? {
        "1" => true,
        "0" => false,
        _ => return None,
    };
    let updated_epoch = parts.next()?.parse().ok()?;
    let path = parts.next()?.to_string();
    if path.is_empty() {
        return None;
    }
    Some(SizeEntry {
        path,
        bytes,
        items,
        on_disk,
        complete,
        updated_epoch,
    })
}

/// Parses a `label\tpath` value into a folder; tolerates a bare path (no tab).
fn parse_folder(value: &str) -> Option<Folder> {
    if value.is_empty() {
        return None;
    }
    let (label, path) = value
        .split_once('\t')
        .map(|(l, p)| (l.to_string(), p.to_string()))
        .unwrap_or_else(|| {
            let label = std::path::Path::new(value)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(value)
                .to_string();
            (label, value.to_string())
        });
    if path.is_empty() {
        return None;
    }
    Some(Folder::new(label, path, '#'))
}
