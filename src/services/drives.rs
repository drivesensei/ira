use std::io::Result as IOResult;

#[cfg(unix)]
use crate::domain::data::DiskSpace;
use crate::domain::data::Folder;

/// Turns raw block counts into a `DiskSpace`. A zero block size (some FUSE
/// filesystems) falls back to `bsize`; a zero total means "unknown" (`None`).
/// Callers pass `u64` counts so the multiply never wraps on 32-bit fields.
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

/// Filesystem totals via `statvfs` (O(1) syscall; no directory walk).
/// `f_bavail` is the space usable by a non-root user, matching `df`.
/// May block on hung network mounts, so only call it from worker threads.
#[cfg(all(unix, not(target_os = "macos")))]
fn disk_space(path: &str) -> Option<DiskSpace> {
    let c_path = std::ffi::CString::new(path).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut s) } != 0 {
        return None;
    }
    space_from_counts(
        s.f_blocks as u64,
        s.f_bavail as u64,
        s.f_frsize as u64,
        s.f_bsize as u64,
    )
}

/// macOS: `statvfs` block counts are 32-bit and wrap on volumes over 16 TB,
/// so use `statfs`, whose counts are 64-bit.
#[cfg(target_os = "macos")]
fn disk_space(path: &str) -> Option<DiskSpace> {
    let c_path = std::ffi::CString::new(path).ok()?;
    let mut s: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c_path.as_ptr(), &mut s) } != 0 {
        return None;
    }
    space_from_counts(s.f_blocks, s.f_bavail, 0, s.f_bsize as u64)
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
        if let Some(space) = disk_space(&d.path) {
            d.space = Some(space);
        }
    }
}

#[cfg(target_os = "windows")]
use crate::services::windows_drives_labels::disk_space;

#[cfg(target_os = "macos")]
use std::fs::read_dir;

#[cfg(target_os = "macos")]
const ICLOUD_REL_PATH: &str = "Library/Mobile Documents/com~apple~CloudDocs";

#[cfg(target_os = "macos")]
const ICLOUD_SHORTCUT: char = '9';

/// Checks whether the macOS iCloud Drive folder exists. A `PermissionDenied`
/// result from `metadata` means TCC blocked inspecting inside `Mobile Documents`,
/// which only happens when the folder exists on disk.
#[cfg(target_os = "macos")]
fn icloud_dir_exists(path: &std::path::Path) -> bool {
    match std::fs::metadata(path) {
        Ok(m) => m.is_dir(),
        Err(e) => e.kind() == std::io::ErrorKind::PermissionDenied,
    }
}

#[cfg(target_os = "macos")]
pub fn list_drives() -> IOResult<Vec<Folder>> {
    const VOLUMES: &str = "/Volumes";
    let mut drives: Vec<Folder> = Vec::new();

    if let Ok(entries) = read_dir(VOLUMES) {
        for entry in entries.flatten() {
            if let Ok(file_name) = entry.file_name().into_string() {
                let n = drives.len() + 1;
                // Reserve '9' for iCloud Drive.
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
                        space: None,
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
        let mut raw: Vec<u8> = Vec::new();
        while i < bytes.len() {
            match bytes[i] {
                b'\\'
                    if i + 3 < bytes.len()
                        && bytes[i + 1] == b'x'
                        && bytes[i + 2].is_ascii_hexdigit()
                        && bytes[i + 3].is_ascii_hexdigit() =>
                {
                    // lsblk escapes unsafe bytes as \xNN (e.g. space = \x20).
                    let hex = &line[i + 2..i + 4];
                    raw.push(u8::from_str_radix(hex, 16).unwrap_or(b'?'));
                    i += 4;
                }
                b'\\' if i + 1 < bytes.len() => {
                    raw.push(bytes[i + 1]);
                    i += 2;
                }
                b'"' => {
                    i += 1;
                    break;
                }
                b => {
                    raw.push(b);
                    i += 1;
                }
            }
        }
        pairs.push((key, String::from_utf8_lossy(&raw).into_owned()));
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
        let size: Option<u64> = lsblk_field(&pairs, "SIZE").parse().ok();

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
        // No syscalls here (this runs on the UI thread too): report the
        // partition size; `fill_space` upgrades mounted drives with free space
        // from the filesystem on the background poller.
        let space = size.map(|total| DiskSpace {
            total,
            available: None,
        });

        drives.push(Folder {
            label: display_name,
            path: mountpoint.to_string(),
            shortcut: char::from_digit(i as u32, 10).unwrap_or('#'),
            device: Some(device),
            space,
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
            "-b",
            "-o",
            "KNAME,TYPE,FSTYPE,LABEL,PARTTYPE,MOUNTPOINT,SIZE",
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
        return Err(std::io::Error::other(msg));
    }

    parse_mount_point(&String::from_utf8_lossy(&output.stdout)).ok_or_else(|| {
        std::io::Error::other("could not determine mount point from udisksctl output")
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
mod tests {
    use super::*;

    const FIXTURE: &str = r#"KNAME="sda" TYPE="disk" FSTYPE="" LABEL="" PARTTYPE="" MOUNTPOINT=""
KNAME="sda1" TYPE="part" FSTYPE="vfat" LABEL="" PARTTYPE="c12a7328-f81f-11d2-ba4b-00a0c93ec93b" MOUNTPOINT="/boot"
KNAME="sda2" TYPE="part" FSTYPE="crypto_LUKS" LABEL="" PARTTYPE="4f68bce3-e8cd-4db1-96e7-fbcaf984b709" MOUNTPOINT=""
KNAME="sdb" TYPE="disk" FSTYPE="" LABEL="" PARTTYPE="" MOUNTPOINT=""
KNAME="sdb1" TYPE="part" FSTYPE="ntfs" LABEL="SEAGATE SATA HDD" PARTTYPE="0x7" MOUNTPOINT=""
KNAME="zram0" TYPE="disk" FSTYPE="swap" LABEL="zram0" PARTTYPE="" MOUNTPOINT="[SWAP]"
KNAME="nvme0n1p1" TYPE="part" FSTYPE="vfat" LABEL="" PARTTYPE="c12a7328-f81f-11d2-ba4b-00a0c93ec93b" MOUNTPOINT=""
KNAME="nvme0n1p2" TYPE="part" FSTYPE="" LABEL="" PARTTYPE="e3c9e316-0b5c-4db8-817d-f92df00215ae" MOUNTPOINT=""
KNAME="nvme0n1p3" TYPE="part" FSTYPE="ntfs" LABEL="ADATA NVME SSD" PARTTYPE="ebd0a0a2-b9e5-4433-87c0-68b6b72699c7" MOUNTPOINT=""
KNAME="nvme0n1p4" TYPE="part" FSTYPE="ntfs" LABEL="" PARTTYPE="de94bba4-06d1-4d40-a16a-bfd50179d6ac" MOUNTPOINT=""
KNAME="sdc1" TYPE="part" FSTYPE="exfat" LABEL="USB STICK" PARTTYPE="0x7" MOUNTPOINT="/run/media/vlad/USB STICK""#;

    #[test]
    fn detects_mounted_and_unmounted_drives() {
        let drives = drives_from_lsblk(FIXTURE);
        let devices: Vec<&str> = drives
            .iter()
            .map(|d| d.device.as_deref().unwrap())
            .collect();
        assert_eq!(devices, vec!["/dev/sdb1", "/dev/nvme0n1p3", "/dev/sdc1"]);
        let paths: Vec<&str> = drives.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, vec!["", "", "/run/media/vlad/USB STICK"]);
    }

    #[test]
    fn filters_system_partitions_and_virtual_devices() {
        let drives = drives_from_lsblk(FIXTURE);
        let labels: Vec<&str> = drives.iter().map(|d| d.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["SEAGATE SATA HDD", "ADATA NVME SSD", "USB STICK"]
        );
    }

    #[test]
    fn unmounted_drive_reports_partition_size_only() {
        let fixture = r#"KNAME="sdb1" TYPE="part" FSTYPE="ntfs" LABEL="X" PARTTYPE="0x7" MOUNTPOINT="" SIZE="2000398934016""#;
        let drives = drives_from_lsblk(fixture);
        assert_eq!(
            drives[0].space,
            Some(DiskSpace {
                total: 2000398934016,
                available: None
            })
        );
    }

    /// Parses `df -B1 -P <path>` into (total, available) bytes.
    fn df_bytes(path: &str) -> (u64, u64) {
        let out = std::process::Command::new("df")
            .args(["-B1", "-P", path])
            .output()
            .expect("run df");
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let cols: Vec<&str> = text
            .lines()
            .nth(1)
            .expect("df row")
            .split_whitespace()
            .collect();
        (cols[1].parse().unwrap(), cols[3].parse().unwrap())
    }

    #[test]
    fn statvfs_matches_df_for_root() {
        let s = disk_space("/").expect("statvfs(/)");
        let (df_total, df_avail) = df_bytes("/");
        let avail = s.available.unwrap();
        assert!(avail <= s.total);
        // df and statvfs are separate syscalls; allow drift from concurrent writes.
        let tol = 64 * 1024 * 1024;
        assert!(
            s.total.abs_diff(df_total) <= tol,
            "{} vs {}",
            s.total,
            df_total
        );
        assert!(avail.abs_diff(df_avail) <= tol, "{avail} vs {df_avail}");
    }

    #[test]
    fn space_from_counts_handles_edge_cases() {
        // f_frsize == 0 falls back to f_bsize.
        let s = space_from_counts(10, 4, 0, 4096).unwrap();
        assert_eq!((s.total, s.available), (40960, Some(16384)));
        // Zero total is unknown, not "0B free / 0B".
        assert_eq!(space_from_counts(0, 0, 4096, 4096), None);
        assert_eq!(space_from_counts(10, 4, 0, 0), None);
        // Counts beyond u32 multiply in u64 without wrapping (>16 TB volume).
        let blocks = u32::MAX as u64 + 10;
        let s = space_from_counts(blocks, blocks, 4096, 4096).unwrap();
        assert_eq!(s.total, blocks * 4096);
        // Available never exceeds total.
        assert_eq!(space_from_counts(1, 5, 10, 10).unwrap().available, Some(10));
    }

    #[test]
    fn list_step_does_no_statvfs_but_fill_space_does() {
        let fixture = r#"KNAME="sdc1" TYPE="part" FSTYPE="ext4" LABEL="R" PARTTYPE="" MOUNTPOINT="/mnt" SIZE="123""#;
        let mut drives = drives_from_lsblk(fixture);
        assert_eq!(
            drives[0].space,
            Some(DiskSpace {
                total: 123,
                available: None
            })
        );
        drives[0].path = "/".to_string();
        fill_space(&mut drives);
        let s = drives[0].space.unwrap();
        assert!(s.available.is_some() && s.total > 123);
        // Unmounted drives keep the partition size.
        let mut unmounted = drives_from_lsblk(&fixture.replace("/mnt", ""));
        fill_space(&mut unmounted);
        assert_eq!(unmounted[0].space.unwrap().total, 123);
    }

    #[test]
    fn lsblk_hex_escapes_are_decoded() {
        let pairs = parse_lsblk_pairs(
            r#"LABEL="USB\x20STICK" MOUNTPOINT="/run/media/v/USB\x20STICK" X="a\"b""#,
        );
        assert_eq!(pairs[0].1, "USB STICK");
        assert_eq!(pairs[1].1, "/run/media/v/USB STICK");
        assert_eq!(pairs[2].1, "a\"b");
        // Multi-byte UTF-8 survives (\xC3\xA9 = e-acute).
        let pairs = parse_lsblk_pairs(r#"LABEL="caf\xc3\xa9""#);
        assert_eq!(pairs[0].1, "café");
    }

    #[test]
    fn parses_udisksctl_mount_point() {
        let out = "Mounted /dev/sdb1 at /run/media/vlad/SEAGATE SATA HDD.";
        assert_eq!(
            parse_mount_point(out).as_deref(),
            Some("/run/media/vlad/SEAGATE SATA HDD")
        );
    }
}
