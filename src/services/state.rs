use std::fs;
use std::path::PathBuf;

use crate::domain::data::Folder;

/// The persisted session state: split layout and each pane's folder.
#[derive(Debug, Default, Clone)]
pub struct SessionState {
    pub split: bool,
    pub active_pane: usize,
    pub left: Option<Folder>,
    pub right: Option<Folder>,
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
    let Ok(contents) = fs::read_to_string(path) else {
        return SessionState::default();
    };

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
            "left" => state.left = parse_folder(value),
            "right" => state.right = parse_folder(value),
            _ => {}
        }
    }
    state
}

/// Persists the session state as `key=value` lines.
pub fn save_state(state: &SessionState) {
    let Some(path) = state_file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }

    let mut content = String::new();
    content.push_str(&format!("split={}\n", state.split as u8));
    content.push_str(&format!("active={}\n", state.active_pane));
    for (key, folder) in [("left", &state.left), ("right", &state.right)] {
        match folder {
            Some(f) => content.push_str(&format!("{}={}\t{}\n", key, f.label, f.path)),
            None => content.push_str(&format!("{}={}\n", key, "")),
        }
    }
    let _ = fs::write(path, content);
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

#[cfg(test)]
mod tests {
    use super::parse_folder;

    #[test]
    fn parses_label_path_pairs_and_bare_paths() {
        let f = parse_folder("Home\t/home/vlad").unwrap();
        assert_eq!(f.label, "Home");
        assert_eq!(f.path, "/home/vlad");

        assert!(parse_folder("").is_none());
        assert!(parse_folder("label\t").is_none()); // empty path

        let bare = parse_folder("/home/vlad").unwrap();
        assert_eq!(bare.label, "vlad");
        assert_eq!(bare.path, "/home/vlad");
    }
}
