use std::io::Result as IOResult;

use crate::domain::data::Folder;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::fs::read_dir;

#[cfg(target_os = "macos")]
pub fn list_drives() -> IOResult<Vec<Folder>> {
    const VOLUMES: &str = "/Volumes";
    let mut drives: Vec<Folder> = Vec::new();

    if let Ok(entries) = read_dir(VOLUMES) {
        for (i, entry) in entries.enumerate() {
            if let Ok(entry) = entry {
                if let Ok(file_name) = entry.file_name().into_string() {
                    drives.push(Folder {
                        path: format!("{}/{}", VOLUMES, file_name),
                        label: format!("[{}] 🖥️ {}", i + 1, file_name),
                        shortcut: (i + 1) as u8 as char,
                    });
                }
            }
        }
    }
    Ok(drives)
}

#[cfg(target_os = "windows")]
use std::fs::metadata;

#[cfg(target_os = "windows")]
use crate::services::windows_drives_labels::get_volume_label;

#[cfg(target_os = "windows")]
pub fn list_drives() -> IOResult<Vec<Folder>> {
    let mut i = 0;
    let drives: Vec<Folder> = ('A'..'Z')
        .filter_map(|letter| {
            let maybe_drive = format!("{}:\\", letter);
            if metadata(&maybe_drive).is_ok() {
                if let Ok(label) = get_volume_label(&maybe_drive) {
                    i += 1;
                    Some(Folder {
                        path: maybe_drive.clone(),
                        label: {
                            if label.len() > 1 {
                                format!("[{}] 🖥️ {} ({})", i, maybe_drive.clone(), label)
                            } else {
                                format!("[{}] 🖥️ {}", i, maybe_drive.clone())
                            }
                        },
                        shortcut: i as u8 as char,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();
    Ok(drives)
}

#[cfg(target_os = "linux")]
pub fn list_drives() -> IOResult<Vec<Folder>> {
    // Intenta listar los discos en /mnt y /media
    const MOUNT_POINTS: [&str; 2] = ["/mnt", "/media"];
    let mut drives: Vec<Folder> = Vec::new();
    let mut i = 0;

    for mount_point in MOUNT_POINTS.iter() {
        if let Ok(entries) = read_dir(mount_point) {
            for entry in entries {
                if let Ok(entry) = entry {
                    if let Ok(file_name) = entry.file_name().into_string() {
                        i += 1;
                        drives.push(Folder {
                            path: format!("{}/{}", mount_point, file_name),
                            label: format!("[{}] 🖥️ {}", i, file_name),
                            shortcut: i as u8 as char,
                        });
                    }
                }
            }
        }
    }
    Ok(drives)
}
