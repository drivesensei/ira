use std::path::PathBuf;

use cli_log::{error, info};
use dirs_next::config_dir;

pub fn get_config_file_path() -> Option<PathBuf> {
    if let Some(mut config_path) = config_dir() {
        config_path.push("ira");
        if let Err(e) = std::fs::create_dir_all(&config_path) {
            error!("failed to create config dir: {}", e);
            return None;
        }
        config_path.push("bs.json");
        info!("config path: {:?}", config_path);
        Some(config_path)
    } else {
        error!("Could not determine the configuration directory for the OS.");
        None
    }
}
