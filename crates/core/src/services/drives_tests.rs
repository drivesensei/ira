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
