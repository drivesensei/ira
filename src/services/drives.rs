use std::io::Result as IOResult;

use crate::domain::data::Folder;

#[cfg(target_os = "macos")]
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
                        label: file_name,
                        shortcut: (i + 1) as u8 as char,
                        device: None,
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
                                format!("{} ({})", maybe_drive.clone(), label)
                            } else {
                                maybe_drive.clone()
                            }
                        },
                        shortcut: i as u8 as char,
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
    fn parses_udisksctl_mount_point() {
        let out = "Mounted /dev/sdb1 at /run/media/vlad/SEAGATE SATA HDD.";
        assert_eq!(
            parse_mount_point(out).as_deref(),
            Some("/run/media/vlad/SEAGATE SATA HDD")
        );
    }
}
