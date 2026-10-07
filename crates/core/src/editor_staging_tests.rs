use super::*;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ira-stage-tests-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn collision_retry_never_modifies_existing_directory() {
    let f = Fixture::new();
    let collision = f.0.join("collision");
    fs::create_dir(&collision).unwrap();
    fs::write(collision.join("foreign"), b"keep").unwrap();
    let mut attempts = 0;
    let created = private_directory(&f.0, || {
        attempts += 1;
        if attempts == 1 {
            "collision".into()
        } else {
            "private".into()
        }
    })
    .unwrap();
    assert_eq!(attempts, 2);
    assert_eq!(fs::read(collision.join("foreign")).unwrap(), b"keep");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&created).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    fs::remove_dir(&created).unwrap();
}
#[test]
fn collision_exhaustion_is_bounded_and_preserves_existing_entry() {
    let f = Fixture::new();
    let collision = f.0.join("collision");
    fs::write(&collision, b"foreign").unwrap();
    let mut attempts = 0;
    let error = private_directory(&f.0, || {
        attempts += 1;
        "collision".into()
    })
    .unwrap_err();
    assert_eq!(attempts, 64);
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(collision).unwrap(), b"foreign");
}
#[test]
fn failed_publication_preserves_target_and_cleans_private_stage() {
    let f = Fixture::new();
    let target = f.0.join("directory");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("keep"), b"untouched").unwrap();
    let permissions = fs::metadata(target.join("keep")).unwrap().permissions();
    assert!(save(&target, b"replacement", permissions).is_err());
    assert_eq!(fs::read(target.join("keep")).unwrap(), b"untouched");
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
}
#[test]
fn successful_publication_cleans_private_stage_and_preserves_permissions() {
    let f = Fixture::new();
    let target = f.0.join("file");
    fs::write(&target, b"old").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, Permissions::from_mode(0o640)).unwrap();
    }
    let permissions = fs::metadata(&target).unwrap().permissions();
    save(&target, b"new", permissions.clone()).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"new");
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode(),
            permissions.mode()
        );
    }
}
