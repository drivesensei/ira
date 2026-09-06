use std::fs;
use std::path::PathBuf;

use crate::domain::data::Folder;

/// Letters in the order they appear on an English (QWERTY) keyboard.
pub const KEYBOARD_ORDER: &[char] = &[
    'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', 'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l',
    'z', 'x', 'c', 'v', 'b', 'n', 'm',
];
/// Letters reserved for the common-folder shortcuts (see `folders.rs`).
pub const COMMON_FOLDER_KEYS: &[char] = &['w', 'e', 'r', 't', 'y', 'u', 'i'];

/// Letters reserved for non-bookmark actions (quit, Ctrl-C, top, bottom,
/// toggle, copy, move, image preview).
pub const RESERVED_KEYS: &[char] = &['q', 'c', 'z', 'x', 'b', 'm', 'v'];

/// First letter in keyboard order not present in `used`.
pub fn first_free_shortcut(used: &[char]) -> Option<char> {
    KEYBOARD_ORDER.iter().copied().find(|c| !used.contains(c))
}

/// Next available bookmark shortcut, avoiding common-folder letters, reserved
/// keys, and shortcuts already assigned to bookmarks.
pub fn next_free_shortcut(bookmarks: &[Folder]) -> Option<char> {
    let mut used: Vec<char> = COMMON_FOLDER_KEYS.to_vec();
    used.extend_from_slice(RESERVED_KEYS);
    used.extend(bookmarks.iter().map(|f| f.shortcut));
    first_free_shortcut(&used)
}

/// Path to the persisted bookmarks file (`~/.config/ira/bookmarks`).
fn bookmarks_file() -> Option<PathBuf> {
    dirs_next::config_dir().map(|d| d.join("ira").join("bookmarks"))
}

/// Loads persisted bookmarks as `(label, path)` pairs, in saved order.
pub fn read_bookmarks() -> Vec<(String, String)> {
    let Some(path) = bookmarks_file() else {
        return Vec::new();
    };
    let Ok(contents) = fs::read_to_string(path) else {
        return Vec::new();
    };

    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| match line.split_once('\t') {
            Some((label, path)) => (label.to_string(), path.to_string()),
            None => {
                let label = std::path::Path::new(line)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(line)
                    .to_string();
                (label, line.to_string())
            }
        })
        .collect()
}

/// Persists bookmarks as `label\tpath` lines.
pub fn write_bookmarks(bookmarks: &[Folder]) {
    let Some(path) = bookmarks_file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let content: String = bookmarks
        .iter()
        .map(|b| format!("{}\t{}\n", b.label, b.path))
        .collect();
    let _ = fs::write(path, content);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(shortcut: char) -> Folder {
        Folder::new("x".to_string(), "/x".to_string(), shortcut)
    }

    #[test]
    fn assigns_letters_in_keyboard_order_skipping_used() {
        let mut bookmarks: Vec<Folder> = Vec::new();
        let mut assigned: Vec<char> = Vec::new();
        while let Some(sc) = next_free_shortcut(&bookmarks) {
            assigned.push(sc);
            bookmarks.push(folder(sc));
        }

        assert_eq!(
            assigned,
            vec!['o', 'p', 'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'n']
        );
    }
}
