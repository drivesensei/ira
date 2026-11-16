use super::*;

#[test]
fn parses_image_mount_points_and_unescapes_them() {
    let plist = "<dict>\n\t<key>mount-point</key>\n\t<string>/Volumes/Foo &amp; Bar</string>\n\
                 \t<key>image-path</key>\n\t<string>/x.dmg</string>\n\
                 \t<key>mount-point</key>\n\t<string>/Volumes/App</string>\n</dict>";
    assert_eq!(
        parse_image_mounts(plist),
        ["/Volumes/Foo & Bar", "/Volumes/App"]
    );
}

#[test]
fn keeps_only_local_browsable_non_image_mounts() {
    let images = ["/Volumes/App".to_string()];
    let keep = |line: &str| storage_mount_point(line, &images);
    assert_eq!(
        keep("/dev/disk6s2 on /Volumes/My USB (exfat, local, nodev, nosuid, fskit)").as_deref(),
        Some("/Volumes/My USB")
    );
    assert_eq!(
        keep("/dev/disk3s1s1 on / (apfs, sealed, local, read-only, journaled)").as_deref(),
        Some("/")
    );
    assert_eq!(
        keep("/dev/disk9s1 on /Volumes/App (apfs, local, read-only)"),
        None,
        "disk image"
    );
    assert_eq!(
        keep("/dev/disk3s3 on /Volumes/Recovery (apfs, local, journaled, nobrowse)"),
        None
    );
    assert_eq!(
        keep("//me@nas/share on /Volumes/share (smbfs, nodev, nosuid)"),
        None,
        "network"
    );
}

#[test]
fn real_mount_table_keeps_the_boot_volume() {
    let mounts = storage_mounts().expect("mount runs");
    assert!(mounts.iter().any(|m| m == "/"), "{mounts:?}");
}
