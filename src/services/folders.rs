use crate::domain::data::Folder;
use dirs_next;

pub fn list_common_folders() -> Vec<Folder> {
    let mut result: Vec<Folder> = Vec::new();
    if let Some(home) = dirs_next::home_dir() {
        if let Some(homestr) = home.to_str() {
            result.push(Folder {
                label: "Home".to_string(),
                path: homestr.to_string(),
                shortcut: 'w',
            });
        }
    }

    if let Some(desktop) = dirs_next::desktop_dir() {
        if let Some(desktopstr) = desktop.to_str() {
            result.push(Folder {
                label: "Desktop".to_string(),
                path: desktopstr.to_string(),
                shortcut: 'e',
            });
        }
    }

    if let Some(document) = dirs_next::document_dir() {
        if let Some(documentstr) = document.to_str() {
            result.push(Folder {
                label: "Documents".to_string(),
                path: documentstr.to_string(),
                shortcut: 'r',
            });
        }
    }

    if let Some(download) = dirs_next::download_dir() {
        if let Some(downloadstr) = download.to_str() {
            result.push(Folder {
                label: "Downloads".to_string(),
                path: downloadstr.to_string(),
                shortcut: 't',
            });
        }
    }

    if let Some(music) = dirs_next::audio_dir() {
        if let Some(musicstr) = music.to_str() {
            result.push(Folder {
                label: "Music".to_string(),
                path: musicstr.to_string(),
                shortcut: 'y',
            });
        }
    }

    if let Some(video) = dirs_next::video_dir() {
        if let Some(videostr) = video.to_str() {
            result.push(Folder {
                label: "Videos".to_string(),
                path: videostr.to_string(),
                shortcut: 'u',
            });
        }
    }

    if let Some(public) = dirs_next::public_dir() {
        if let Some(publicstr) = public.to_str() {
            result.push(Folder {
                label: "Public".to_string(),
                path: publicstr.to_string(),
                shortcut: 'i',
            });
        }
    }

    result
}
