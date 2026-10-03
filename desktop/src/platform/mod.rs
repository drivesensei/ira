//! Explicit native requests run on workers, never during Render.
use ira_core::model::HostRequest;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
pub fn execute(request: HostRequest) -> Result<(), String> {
    match request {
        HostRequest::OpenFile(path) => open(&path),
        HostRequest::Reveal {
            target,
            is_dir,
            cwd,
        } => reveal(&target, is_dir, &cwd),
        HostRequest::Terminal(path) => terminal(&path),
        _ => Err("Native request requires its dedicated input/clipboard adapter".into()),
    }
}
fn spawn(program: &str, args: &[&std::ffi::OsStr], cwd: Option<&Path>) -> Result<(), String> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}
#[cfg(target_os = "macos")]
fn open(path: &Path) -> Result<(), String> {
    spawn("open", &[path.as_os_str()], None).map_err(|e| format!("Failed to open: {e}"))
}
#[cfg(target_os = "windows")]
fn open(path: &Path) -> Result<(), String> {
    spawn("explorer", &[path.as_os_str()], None).map_err(|e| format!("Failed to open: {e}"))
}
#[cfg(target_os = "linux")]
fn open(path: &Path) -> Result<(), String> {
    spawn("xdg-open", &[path.as_os_str()], None).map_err(|e| format!("Failed to open: {e}"))
}
fn reveal(target: &Path, is_dir: bool, cwd: &Path) -> Result<(), String> {
    if spawn_first_available(
        &file_manager_candidates(&target.to_string_lossy(), is_dir),
        &cwd.to_string_lossy(),
    ) {
        Ok(())
    } else {
        Err("No file browser found".into())
    }
}
fn terminal(path: &Path) -> Result<(), String> {
    if spawn_first_available(
        &terminal_candidates(&path.to_string_lossy()),
        &path.to_string_lossy(),
    ) {
        Ok(())
    } else {
        Err("No terminal emulator found".into())
    }
}
/// Terminal emulators to try for `0`, in preference order, with the args
/// that make them open in `dir`. Pure so tests can inspect the table.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn terminal_candidates(dir: &str) -> Vec<(String, Vec<String>)> {
    vec![
        (
            "foot".into(),
            vec!["--working-directory".into(), dir.into()],
        ),
        (
            "alacritty".into(),
            vec!["--working-directory".into(), dir.into()],
        ),
        ("kitty".into(), vec!["--directory".into(), dir.into()]),
        (
            "gnome-terminal".into(),
            vec![format!("--working-directory={dir}")],
        ),
        ("konsole".into(), vec!["--workdir".into(), dir.into()]),
        (
            "xfce4-terminal".into(),
            vec!["--working-directory".into(), dir.into()],
        ),
        ("xterm".into(), vec![]),
    ]
}

/// File browsers to try for the file-browser reveal (`-`), in preference order.
///
/// Linux: the default handler first (`xdg-open`), then the common file
/// managers by name. A directory target opens itself; a file target opens the
/// folder holding it, because no file manager exposes a portable "select this
/// file" request.
#[cfg(not(any(target_os = "macos", windows)))]
pub fn file_manager_candidates(target: &str, is_dir: bool) -> Vec<(String, Vec<String>)> {
    let dir = if is_dir {
        target.to_string()
    } else {
        std::path::Path::new(target)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| target.to_string())
    };
    let mut v = vec![("xdg-open".to_string(), vec![dir.clone()])];
    for program in ["nautilus", "dolphin", "thunar", "nemo", "caja", "pcmanfm"] {
        v.push((program.to_string(), vec![dir.clone()]));
    }
    v
}

/// Finder: `open -R <path>` reveals (selects) the item in its window.
/// `/usr/bin/open` always exists and is on `PATH`.
#[cfg(target_os = "macos")]
pub fn file_manager_candidates(target: &str, _is_dir: bool) -> Vec<(String, Vec<String>)> {
    vec![(
        "open".to_string(),
        vec!["-R".to_string(), target.to_string()],
    )]
}

/// File Explorer: `/select,<path>` opens the containing folder with the item
/// selected (works for files and folders alike). `explorer` splits on commas,
/// so a path containing one is quoted.
#[cfg(windows)]
pub fn file_manager_candidates(target: &str, _is_dir: bool) -> Vec<(String, Vec<String>)> {
    let arg = if target.contains(',') {
        format!("/select,\"{target}\"")
    } else {
        format!("/select,{target}")
    };
    vec![("explorer".to_string(), vec![arg])]
}

/// Terminal emulators to try for `0` on macOS, in preference order.
///
/// GUI terminal apps are not on `PATH`, so they launch through `open`:
/// `open -a <App> <dir>` asks LaunchServices to open the folder, which
/// Terminal.app and iTerm2 honor by opening a new window already `cd`'d
/// into it; `open -na <App> --args …` starts a fresh instance for
/// terminals whose GUI launcher needs explicit CLI flags. `/usr/bin/open`
/// always exists, so app presence must be decided here: the terminal ira
/// itself runs in (via `TERM_PROGRAM`) is certainly installed, every
/// other app-bundle entry is checked against /Applications (and
/// ~/Applications) first.
#[cfg(target_os = "macos")]
pub fn terminal_candidates(dir: &str) -> Vec<(String, Vec<String>)> {
    let mut v = Vec::new();
    match std::env::var("TERM_PROGRAM").as_deref() {
        Ok("Apple_Terminal") => v.push(open_in_app("Terminal", dir)),
        Ok(t) if t.contains("iTerm") => v.push(open_in_app("iTerm", dir)),
        Ok("ghostty") => v.push(open_new_app_args("Ghostty", &["--working-directory", dir])),
        Ok("WezTerm") => v.push(open_new_app_args("WezTerm", &["start", "--cwd", dir])),
        _ => {}
    }
    // Homebrew formula binaries that accept a working directory.
    v.push(("kitty".into(), vec!["--directory".into(), dir.into()]));
    v.push((
        "alacritty".into(),
        vec!["--working-directory".into(), dir.into()],
    ));
    v.push((
        "wezterm".into(),
        vec!["start".into(), "--cwd".into(), dir.into()],
    ));
    // Cask-installed app bundles (no CLI on PATH).
    if mac_app_installed("iTerm") {
        v.push(open_in_app("iTerm", dir));
    }
    if mac_app_installed("WezTerm") {
        v.push(open_new_app_args("WezTerm", &["start", "--cwd", dir]));
    }
    if mac_app_installed("Ghostty") {
        v.push(open_new_app_args("Ghostty", &["--working-directory", dir]));
    }
    // Terminal.app ships with every macOS install.
    v.push(open_in_app("Terminal", dir));
    v
}

/// Windows Terminal is the default console host on Windows 11 and exposes
/// its app-execution alias on `PATH`; `-d` opens a new window in `dir`.
/// The conhost fallback goes through `start`, whose `/D` flag sets the
/// working directory of the new `cmd` window (the empty string is the
/// window title `start` requires before a quoted path).
#[cfg(target_os = "windows")]
pub fn terminal_candidates(dir: &str) -> Vec<(String, Vec<String>)> {
    vec![
        ("wt".into(), vec!["-d".into(), dir.into()]),
        (
            "cmd".into(),
            vec![
                "/C".into(),
                "start".into(),
                String::new(),
                "/D".into(),
                dir.into(),
                "cmd.exe".into(),
            ],
        ),
    ]
}

/// `open -a <app> <dir>` on macOS: LaunchServices opens the folder with
/// the app; terminal apps open a new window already cd'd into it.
#[cfg(target_os = "macos")]
fn open_in_app(app: &str, dir: &str) -> (String, Vec<String>) {
    ("open".into(), vec!["-a".into(), app.into(), dir.into()])
}

/// `open -na <app> --args <args>` on macOS: launch a NEW app instance
/// passing CLI arguments through to its binary.
#[cfg(target_os = "macos")]
fn open_new_app_args(app: &str, args: &[&str]) -> (String, Vec<String>) {
    let mut v = vec!["-na".into(), app.into(), "--args".into()];
    v.extend(args.iter().map(|a| (*a).into()));
    ("open".into(), v)
}

/// True when an `.app` bundle for `app` exists in /Applications or
/// ~/Applications — the places LaunchServices finds apps without PATH.
#[cfg(target_os = "macos")]
fn mac_app_installed(app: &str) -> bool {
    let bundle = format!("{app}.app");
    let mut dirs = vec![PathBuf::from("/Applications")];
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join("Applications"));
    }
    dirs.iter().any(|d| d.join(&bundle).is_dir())
}

/// True when `program` is an executable file found on `PATH`
/// (existence on the path is treated as sufficient).
fn which(program: &str) -> bool {
    let Ok(path_var) = std::env::var("PATH") else {
        return false;
    };
    // Windows executables carry an extension, so a bare `explorer` / `wt` /
    // `cmd` is never itself a file there and the `.exe` spelling has to be
    // probed too (harmless on Unix, where such a name simply does not exist).
    let with_exe = format!("{program}.exe");
    std::env::split_paths(&path_var)
        .any(|dir| dir.join(program).is_file() || dir.join(&with_exe).is_file())
}

/// Spawns the first candidate that exists, detached, with `cwd` as its
/// working directory. `false` = nothing could be started (the caller reports
/// the list it tried). Success is its own feedback: a window opens.
fn spawn_first_available(candidates: &[(String, Vec<String>)], cwd: &str) -> bool {
    for (program, args) in candidates {
        if !which(program) {
            continue;
        }
        let mut cmd = std::process::Command::new(program);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .current_dir(cwd);
        if cmd.spawn().is_ok() {
            return true;
        }
    }
    false
}

pub fn drives() -> std::io::Result<Vec<ira_core::domain::data::Folder>> {
    #[cfg(target_os = "windows")]
    {
        ira_core::services::drives::list_drives_with_labels(&windows::Labels)
    }
    #[cfg(not(target_os = "windows"))]
    {
        ira_core::services::drives::list_drives()
    }
}
#[cfg(target_os = "windows")]
mod windows {
    pub struct Labels;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetVolumeInformationW(
            root: *const u16,
            label: *mut u16,
            len: u32,
            serial: *mut u32,
            max_component: *mut u32,
            flags: *mut u32,
            fs: *mut u16,
            fs_len: u32,
        ) -> i32;
    }
    impl ira_core::services::windows_drives_labels::VolumeLabelProvider for Labels {
        fn volume_label(
            &self,
            drive: &str,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let root: Vec<u16> = drive.encode_utf16().chain(Some(0)).collect();
            let mut label = [0u16; 256];
            // Root is NUL-terminated; the output buffer lives throughout this call.
            let ok = unsafe {
                GetVolumeInformationW(
                    root.as_ptr(),
                    label.as_mut_ptr(),
                    label.len() as u32,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                )
            };
            if ok == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let end = label.iter().position(|c| *c == 0).unwrap_or(label.len());
            Ok(String::from_utf16_lossy(&label[..end])
                .chars()
                .filter(char::is_ascii_graphic)
                .collect())
        }
    }
}
