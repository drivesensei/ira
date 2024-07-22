pub struct FEntry {
    pub path: String,
    pub label: String,
}

pub fn list_files(path: &str) -> Result<Vec<FEntry>, std::io::Error> {
    let mut drives = Vec::new();
    let entries = std::fs::read_dir(path)?;

    for entry in entries {
        match entry {
            Ok(entry) => {
                if let Some(label) = entry.file_name().to_str() {
                    let path = entry.path();
                    drives.push(FEntry {
                        path: path.to_string_lossy().into_owned(),
                        label: label.to_string(),
                    });
                }
            }
            Err(e) => eprintln!("Error reading entry: {}", e),
        }
    }

    Ok(drives)
}
