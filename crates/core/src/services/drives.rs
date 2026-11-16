use std::io::Result as IOResult;

#[cfg(unix)]
use crate::domain::data::DiskSpace;
use crate::domain::data::Folder;

/// Turns raw block counts into a `DiskSpace`. Counts are in fragment units
/// (`f_frsize`); macOS reports a 1 MiB `f_bsize` beside a 4 KiB fragment, so
/// using `f_bsize` inflated sizes 256x. A zero fragment size (some FUSE
/// filesystems) falls back to `bsize`; a zero total means "unknown" (`None`).
#[cfg(unix)]
fn space_from_counts(blocks: u64, bavail: u64, frsize: u64, bsize: u64) -> Option<DiskSpace> {
    let unit = if frsize == 0 { bsize } else { frsize };
    let total = blocks.saturating_mul(unit);
    if total == 0 {
        return None;
    }
    Some(DiskSpace {
        total,
        available: Some(bavail.saturating_mul(unit).min(total)),
    })
}

#[cfg(unix)]
fn disk_space(path: &str) -> Option<DiskSpace> {
    let stat = rustix::fs::statvfs(path).ok()?;
    space_from_counts(stat.f_blocks, stat.f_bavail, stat.f_frsize, stat.f_bsize)
}

/// Fills in free/total space for mounted drives. This issues `statvfs`-style
/// calls that can block on stale network mounts, so it must only run on a
/// background thread (the drive poller), never on the UI thread. Drives whose
/// space cannot be read keep whatever `list_drives` reported.
pub fn fill_space(drives: &mut [Folder]) {
    for d in drives.iter_mut() {
        if d.path.is_empty() {
            continue;
        }
        #[cfg(unix)]
        if let Some(space) = disk_space(&d.path) {
            d.space = Some(space);
        }
    }
}

#[cfg(target_os = "macos")]
use std::fs::read_dir;

#[cfg(target_os = "macos")]
const ICLOUD_REL_PATH: &str = "Library/Mobile Documents/com~apple~CloudDocs";

#[cfg(target_os = "macos")]
const ICLOUD_SHORTCUT: char = '9';

#[cfg(target_os = "macos")]
fn icloud_dir_exists(path: &std::path::Path) -> bool {
    match std::fs::metadata(path) {
        Ok(m) => m.is_dir(),
        Err(e) => e.kind() == std::io::ErrorKind::PermissionDenied,
    }
}

/// Mount points of attached disk images (mounted `.dmg` apps, simulator
/// runtimes), read from `hdiutil info` in one fast call.
#[cfg(target_os = "macos")]
fn disk_image_mounts() -> Vec<String> {
    std::process::Command::new("/usr/bin/hdiutil")
        .args(["info", "-plist"])
        .output()
        .map(|o| parse_image_mounts(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// Extracts every `mount-point` value from `hdiutil info -plist` output.
#[cfg(target_os = "macos")]
fn parse_image_mounts(plist: &str) -> Vec<String> {
    let mut mounts = Vec::new();
    let mut lines = plist.lines().map(str::trim);
    while let Some(line) = lines.next() {
        if line != "<key>mount-point</key>" {
            continue;
        }
        let value = lines
            .next()
            .and_then(|l| l.strip_prefix("<string>"))
            .and_then(|l| l.strip_suffix("</string>"));
        if let Some(v) = value {
            mounts.push(
                v.replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&amp;", "&"),
            );
        }
    }
    mounts
}

/// Parses one `mount` line (`<dev> on <path> (<fs>, <flag>, ...)`) and keeps
/// physical storage: local, shown by Finder (`nobrowse` unset) and not a
/// disk image. Returns the mount point when kept.
#[cfg(target_os = "macos")]
fn storage_mount_point(line: &str, image_mounts: &[String]) -> Option<String> {
    let (_, rest) = line.split_once(" on ")?;
    let (mount_point, flags) = rest.rsplit_once(" (")?;
    let flags: Vec<&str> = flags.trim_end_matches(')').split(", ").collect();
    let keep = flags.contains(&"local")
        && !flags.contains(&"nobrowse")
        && !image_mounts.iter().any(|m| m == mount_point);
    keep.then(|| mount_point.to_string())
}

/// Mount points of physical storage volumes. `mount` reads the kernel's
/// cached table (`MNT_NOWAIT`), so a hung network share cannot block. `None`
/// when it cannot run, so the caller shows every volume instead of none.
#[cfg(target_os = "macos")]
fn storage_mounts() -> Option<Vec<String>> {
    let out = std::process::Command::new("/sbin/mount").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let images = disk_image_mounts();
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| storage_mount_point(l, &images))
            .collect(),
    )
}

#[cfg(target_os = "macos")]
pub fn list_drives() -> IOResult<Vec<Folder>> {
    const VOLUMES: &str = "/Volumes";
    let mut drives: Vec<Folder> = Vec::new();
    let storage = storage_mounts();

    if let Ok(entries) = read_dir(VOLUMES) {
        for entry in entries.flatten() {
            // "Macintosh HD" is a symlink to "/"; match the mount it points at.
            let mut target = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_symlink()) {
                target = std::fs::read_link(&target).unwrap_or(target);
            }
            let hidden = storage
                .as_ref()
                .is_some_and(|mounts| !mounts.iter().any(|m| std::path::Path::new(m) == target));
            if hidden {
                continue;
            }
            if let Ok(file_name) = entry.file_name().into_string() {
                let n = drives.len() + 1;
                let shortcut = if n < 9 {
                    char::from_digit(n as u32, 10).unwrap_or('#')
                } else {
                    '#'
                };
                drives.push(Folder {
                    path: format!("{}/{}", VOLUMES, file_name),
                    label: file_name,
                    shortcut,
                    device: None,
                    space: None,
                });
            }
        }
    }

    if let Some(icloud) = dirs_next::home_dir().map(|h| h.join(ICLOUD_REL_PATH)) {
        if icloud_dir_exists(&icloud) {
            if let Some(path_str) = icloud.to_str() {
                drives.push(Folder {
                    path: path_str.to_string(),
                    label: "iCloud".to_string(),
                    shortcut: ICLOUD_SHORTCUT,
                    device: None,
                    space: None,
                });
            }
        }
    }

    Ok(drives)
}

#[cfg(target_os = "windows")]
use std::fs::metadata;

#[cfg(target_os = "windows")]
use crate::services::windows_drives_labels::VolumeLabelProvider;

#[cfg(target_os = "windows")]
pub fn list_drives_with_labels(provider: &impl VolumeLabelProvider) -> IOResult<Vec<Folder>> {
    let mut i = 0;
    let drives: Vec<Folder> = ('A'..'Z')
        .filter_map(|letter| {
            let maybe_drive = format!("{}:\\", letter);
            if metadata(&maybe_drive).is_ok() {
                if let Ok(label) = provider.volume_label(&maybe_drive) {
                    i += 1;
                    Some(Folder {
                        path: maybe_drive.clone(),
                        label: {
                            if label.len() > 1 {
                                format!("{} ({})", maybe_drive.clone(), label)
                            } else {
                                maybe_drive.clone()
                            }
                        },
                        shortcut: char::from_digit(i as u32, 10).unwrap_or('#'),
                        device: None,
                        space: None,
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
const DISK_FS: &[&str] = &[
    "btrfs", "exfat", "ext2", "ext3", "ext4", "f2fs", "fuseblk", "hfs", "hfsplus", "iso9660",
    "jfs", "ntfs", "ntfs3", "reiserfs", "udf", "ufs", "vfat", "xfs", "zfs",
];

// Partition-type GUIDs that denote system/utility partitions rather than user
// data drives: EFI System, Microsoft Reserved, Windows Recovery Environment.
#[cfg(target_os = "linux")]
const SYSTEM_PART_TYPES: &[&str] = &[
    "c12a7328-f81f-11d2-ba4b-00a0c93ec93b", // EFI System Partition
    "e3c9e316-0b5c-4db8-817d-f92df00215ae", // Microsoft Reserved
    "de94bba4-06d1-4d40-a16a-bfd50179d6ac", // Microsoft Windows Recovery Environment
];

#[cfg(target_os = "linux")]
const MEDIA_ROOTS: &[&str] = &["/run/media", "/media", "/mnt"];

#[cfg(target_os = "linux")]
fn is_media_mount(mount_point: &str) -> bool {
    MEDIA_ROOTS.iter().any(|root| {
        mount_point
            .strip_prefix(root)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// Parses one line of `lsblk -P` output: `KEY="value" KEY="value" ...`.
#[cfg(target_os = "linux")]
fn parse_lsblk_pairs(line: &str) -> Vec<(String, String)> {
    let bytes = line.as_bytes();
    let mut pairs = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let key_start = i;
        while i < bytes.len() && bytes[i] != b'=' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let key = line[key_start..i].to_string();
        i += 1; // skip '='
        if i >= bytes.len() || bytes[i] != b'"' {
            break;
        }
        i += 1; // skip opening quote
        let mut value = String::new();
        while i < bytes.len() {
            match bytes[i] {
                b'\\' if i + 1 < bytes.len() => {
                    value.push(bytes[i + 1] as char);
                    i += 2;
                }
                b'"' => {
                    i += 1;
                    break;
                }
                b => {
                    value.push(b as char);
                    i += 1;
                }
            }
        }
        pairs.push((key, value));
    }
    pairs
}

#[cfg(target_os = "linux")]
fn lsblk_field<'a>(pairs: &'a [(String, String)], key: &str) -> &'a str {
    pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .unwrap_or("")
}

/// Turns `lsblk -P -n -o KNAME,TYPE,FSTYPE,LABEL,PARTTYPE,MOUNTPOINT` output
/// into a list of drives. Includes mounted media and unmounted filesystem
/// partitions; excludes virtual devices, swap/LUKS/LVM, system partition types
/// (EFI/MSR/Windows-recovery), and filesystems already mounted at system paths.
#[cfg(target_os = "linux")]
fn drives_from_lsblk(output: &str) -> Vec<Folder> {
    let mut drives: Vec<Folder> = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let pairs = parse_lsblk_pairs(line);
        let kname = lsblk_field(&pairs, "KNAME");
        let typ = lsblk_field(&pairs, "TYPE");
        let fstype = lsblk_field(&pairs, "FSTYPE");
        let label = lsblk_field(&pairs, "LABEL");
        let parttype = lsblk_field(&pairs, "PARTTYPE");
        let mountpoint = lsblk_field(&pairs, "MOUNTPOINT");

        if !DISK_FS.contains(&fstype) {
            continue;
        }
        if typ != "part" && typ != "disk" {
            continue;
        }
        if SYSTEM_PART_TYPES.contains(&parttype) {
            continue;
        }
        if !mountpoint.is_empty() && !is_media_mount(mountpoint) {
            continue;
        }

        let device = format!("/dev/{}", kname);
        let display_name = if label.is_empty() {
            kname.to_string()
        } else {
            label.to_string()
        };
        let i = drives.len() + 1;

        drives.push(Folder {
            label: display_name,
            path: mountpoint.to_string(),
            shortcut: i as u8 as char,
            device: Some(device),
        });
    }

    drives
}

#[cfg(target_os = "linux")]
pub fn list_drives() -> IOResult<Vec<Folder>> {
    let output = std::process::Command::new("lsblk")
        .args([
            "-P",
            "-n",
            "-o",
            "KNAME,TYPE,FSTYPE,LABEL,PARTTYPE,MOUNTPOINT",
        ])
        .output()?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    Ok(drives_from_lsblk(&String::from_utf8_lossy(&output.stdout)))
}

/// Extracts the mount point from `udisksctl mount` output, e.g.
/// `Mounted /dev/sdb1 at /run/media/vlad/LABEL.`
#[cfg(target_os = "linux")]
fn parse_mount_point(stdout: &str) -> Option<String> {
    let at = stdout.find(" at ")?;
    let path = stdout[at + 4..].trim();
    let path = path.strip_suffix('.').unwrap_or(path);
    if path.is_empty() {
        return None;
    }
    Some(path.to_string())
}

/// Mounts a block device the same way the desktop file manager does (udisks2),
/// and returns the resulting mount point.
#[cfg(target_os = "linux")]
pub fn mount_drive(device: &str) -> IOResult<String> {
    let output = std::process::Command::new("udisksctl")
        .args(["mount", "-b", device])
        .output()?;

    if !output.status.success() {
        let msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let msg = if msg.is_empty() {
            "udisksctl mount failed".to_string()
        } else {
            msg
        };
        return Err(std::io::Error::new(std::io::ErrorKind::Other, msg));
    }

    parse_mount_point(&String::from_utf8_lossy(&output.stdout)).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            "could not determine mount point from udisksctl output",
        )
    })
}

/// On-demand mounting is Linux-only (udisks2); macOS and Windows volumes are
/// always mounted, so this is never reachable there.
#[cfg(not(target_os = "linux"))]
pub fn mount_drive(_device: &str) -> IOResult<String> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "on-demand drive mounting is not supported on this platform",
    ))
}

/// Unmounts (and spins down, when the device is a whole drive) a removable
/// device via udisks2, mirroring the desktop file manager's eject.
#[cfg(target_os = "linux")]
pub fn eject_drive(device: &str) -> IOResult<()> {
    let output = std::process::Command::new("udisksctl")
        .args(["unmount", "-b", device])
        .output()?;

    if !output.status.success() {
        let msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(std::io::Error::other(if msg.is_empty() {
            "udisksctl unmount failed".to_string()
        } else {
            msg
        }));
    }
    Ok(())
}

/// Drive ejecting is Linux-only; on other platforms drives are ejected via
/// the desktop environment or OS tray instead.
#[cfg(not(target_os = "linux"))]
pub fn eject_drive(_device: &str) -> IOResult<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "drive ejecting is not supported on this platform",
    ))
}

#[cfg(all(test, target_os = "linux"))]
#[path = "drives_tests.rs"]
mod tests;

#[cfg(all(test, target_os = "macos"))]
#[path = "drives_macos_tests.rs"]
mod macos_tests;
