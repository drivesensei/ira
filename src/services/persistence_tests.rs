use super::*;
use crate::domain::data::Folder;
use crate::services::{
    bookmarks::{try_write_bookmarks_to, write_bookmarks_to},
    state::{save_state_to, try_save_state_to, SessionState},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = test_path("fixtures").join(format!("{}", NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn checked_state_and_bookmarks_preserve_exact_legacy_bytes_and_old_apis() {
    let f = Fixture::new();
    let state = f.0.join("state");
    let bookmarks = f.0.join("bookmarks");
    let data = SessionState::default();
    try_save_state_to(&state, &data).unwrap();
    assert_eq!(
        fs::read(&state).unwrap(),
        b"split=0\nactive=0\nhidden=0\npreview0=3\npreview1=3\nleft=\nright=\n"
    );
    save_state_to(&state, &data);
    let folders = vec![
        Folder::new("雪=one".into(), "/tmp/a b".into(), 'o'),
        Folder::new("next".into(), "/a=b".into(), 'p'),
    ];
    try_write_bookmarks_to(&bookmarks, &folders).unwrap();
    assert_eq!(
        fs::read(&bookmarks).unwrap(),
        "雪=one\t/tmp/a b\nnext\t/a=b\n".as_bytes()
    );
    write_bookmarks_to(&bookmarks, &folders);
    try_write_bookmarks_to(&bookmarks, &[]).unwrap();
    assert_eq!(fs::read(&bookmarks).unwrap(), b"");
}
#[test]
fn checked_write_reports_parent_file_failure_and_preserves_old_file_on_readonly_failure() {
    let f = Fixture::new();
    let parent = f.0.join("parent");
    fs::write(&parent, b"old parent").unwrap();
    let error = try_save_state_to(&parent.join("state"), &SessionState::default()).unwrap_err();
    assert_eq!(error.stage, "create directory");
    assert_eq!(fs::read(&parent).unwrap(), b"old parent");
    let destination = f.0.join("state");
    fs::write(&destination, b"old state").unwrap();
    let mut permissions = fs::metadata(&destination).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&destination, permissions.clone()).unwrap();
    assert!(try_save_state_to(&destination, &SessionState::default()).is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"old state");
    assert!(!fs::read_dir(&f.0).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".ira-persist")));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o600);
        fs::set_permissions(&destination, permissions).unwrap();
    }
}
#[test]
fn publish_failure_cleans_only_owned_temp_and_preserves_destination_directory() {
    let f = Fixture::new();
    let destination = f.0.join("state");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("old"), b"retained").unwrap();
    let error = publish(&destination, b"new state").unwrap_err();
    assert_eq!(error.stage, "publish");
    assert_eq!(fs::read(destination.join("old")).unwrap(), b"retained");
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
}
#[cfg(unix)]
#[test]
fn atomic_publication_preserves_existing_symlink_and_target_permissions() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let f = Fixture::new();
    let target = f.0.join("target");
    let link = f.0.join("link");
    fs::write(&target, b"old").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    symlink(&target, &link).unwrap();
    publish(&link, b"new").unwrap();
    assert!(link.is_symlink());
    assert_eq!(fs::read(&target).unwrap(), b"new");
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[cfg(unix)]
#[test]
fn relative_symlink_chain_and_dangling_target_keep_legacy_write_destination() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let target = f.0.join("target");
    let middle = f.0.join("middle");
    let link = f.0.join("link");
    symlink("target", &middle).unwrap();
    symlink("middle", &link).unwrap();
    publish(&link, b"created through aliases").unwrap();
    assert!(link.is_symlink());
    assert!(middle.is_symlink());
    assert_eq!(fs::read(target).unwrap(), b"created through aliases");
}
#[cfg(unix)]
#[test]
fn hard_link_destination_is_rejected_without_modifying_either_alias() {
    let f = Fixture::new();
    let target = f.0.join("target");
    let alias = f.0.join("alias");
    fs::write(&target, b"old").unwrap();
    fs::hard_link(&target, &alias).unwrap();
    assert_eq!(publish(&target, b"new").unwrap_err().stage, "identity");
    assert_eq!(fs::read(target).unwrap(), b"old");
    assert_eq!(fs::read(alias).unwrap(), b"old");
}
