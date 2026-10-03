use std::fs::metadata;
use std::io::Result;
use std::path::Path;

use crate::domain::data::Folder;

pub fn get_directory(path: &str) -> Result<Option<Folder>> {
    let m = metadata(path)?;
    if m.is_dir() {
        let dir_path = Path::new(path);
        let label = dir_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        Ok(Some(Folder::new(label, path.to_string(), '#')))
    } else {
        Ok(None)
    }
}

pub fn get_parent_directory(dir_path: &str) -> Result<Option<Folder>> {
    let p = Path::new(dir_path);

    let parent_path = match p.parent() {
        Some(path) => path,
        None => return Ok(None),
    };

    let label = parent_path
        .file_name()
        .and_then(|dname| dname.to_str())
        .unwrap_or_default()
        .to_string();

    Ok(Some(Folder::new(
        label,
        parent_path.to_str().unwrap_or_default().to_string(),
        '#',
    )))
}

#[cfg(test)]
#[path = "is_dir_tests.rs"]
mod tests;
