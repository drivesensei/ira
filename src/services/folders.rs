use crate::domain::data::Folder;
use dirs_next;

pub fn list_common_folders() -> Vec<Folder> {
    let mut result: Vec<Folder> = Vec::new();
    if let Some(home) = dirs_next::home_dir() {
        if let Some(homestr) = home.to_str() {
            result.push(Folder::new("Home".to_string(), homestr.to_string(), 'w'));
        }
    }

    if let Some(desktop) = dirs_next::desktop_dir() {
        if let Some(desktopstr) = desktop.to_str() {
            result.push(Folder::new("Desktop".to_string(), desktopstr.to_string(), 'e'));
        }
    }

    if let Some(document) = dirs_next::document_dir() {
        if let Some(documentstr) = document.to_str() {
            result.push(Folder::new("Documents".to_string(), documentstr.to_string(), 'r'));
        }
    }

    if let Some(download) = dirs_next::download_dir() {
        if let Some(downloadstr) = download.to_str() {
            result.push(Folder::new("Downloads".to_string(), downloadstr.to_string(), 't'));
        }
    }

    if let Some(music) = dirs_next::audio_dir() {
        if let Some(musicstr) = music.to_str() {
            result.push(Folder::new("Music".to_string(), musicstr.to_string(), 'y'));
        }
    }

    if let Some(video) = dirs_next::video_dir() {
        if let Some(videostr) = video.to_str() {
            result.push(Folder::new("Videos".to_string(), videostr.to_string(), 'u'));
        }
    }

    if let Some(public) = dirs_next::public_dir() {
        if let Some(publicstr) = public.to_str() {
            result.push(Folder::new("Public".to_string(), publicstr.to_string(), 'i'));
        }
    }

    result
}
