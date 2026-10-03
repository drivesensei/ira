//! D-003 intentional safety exception: frozen oracle remains unchanged.
use super::*;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("ira-overwrite-safety-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::create_dir(root.join("dst")).unwrap();
        Self(root)
    }
    fn path(&self, relative: &str) -> PathBuf { self.0.join(relative) }
}
impl Drop for Fixture { fn drop(&mut self) { fs::remove_dir_all(&self.0).unwrap(); } }
fn transfer(f: &Fixture, kind: JobKind, name: &str, policy: OverwritePolicy) -> Result<(), JobError> {
    let (tx, _rx) = mpsc::channel();
    run_batch(1, kind, &[f.path(&format!("src/{name}")).to_string_lossy().into_owned()], &f.path("dst"), policy, &JobControl::new(), &tx)
}
#[test]
fn overwrite_directory_failure_preserves_existing_tree_and_source() {
    for kind in [JobKind::Copy, JobKind::Move] {
        let f = Fixture::new();
        fs::create_dir(f.path("src/tree")).unwrap();
        fs::write(f.path("src/tree/source.txt"), b"SOURCE").unwrap();
        fs::create_dir(f.path("dst/tree")).unwrap();
        fs::write(f.path("dst/tree/precious.txt"), b"PRECIOUS").unwrap();
        assert!(transfer(&f, kind, "tree", OverwritePolicy::Overwrite).is_err());
        assert_eq!(fs::read(f.path("dst/tree/precious.txt")).unwrap(), b"PRECIOUS");
        assert_eq!(fs::read(f.path("src/tree/source.txt")).unwrap(), b"SOURCE");
    }
}
#[test]
fn missing_source_overwrite_failure_preserves_existing_file() {
    for kind in [JobKind::Copy, JobKind::Move] {
        let f = Fixture::new();
        fs::write(f.path("dst/absent.txt"), b"PRECIOUS").unwrap();
        assert!(transfer(&f, kind, "absent.txt", OverwritePolicy::Overwrite).is_err());
        assert_eq!(fs::read(f.path("dst/absent.txt")).unwrap(), b"PRECIOUS");
    }
}
#[test]
fn successful_file_overwrite_preserves_copy_and_move_semantics() {
    for kind in [JobKind::Copy, JobKind::Move] {
        let f = Fixture::new();
        fs::write(f.path("src/file.txt"), b"NEW").unwrap();
        fs::write(f.path("dst/file.txt"), b"OLD").unwrap();
        transfer(&f, kind, "file.txt", OverwritePolicy::Overwrite).unwrap();
        assert_eq!(fs::read(f.path("dst/file.txt")).unwrap(), b"NEW");
        assert_eq!(f.path("src/file.txt").exists(), kind == JobKind::Copy);
    }
}
#[cfg(unix)]
#[test]
fn failed_overwrite_preserves_destination_symlink_and_its_target() {
    let f = Fixture::new();
    fs::write(f.path("precious.txt"), b"PRECIOUS").unwrap();
    create_symlink("../precious.txt", f.path("dst/absent.txt")).unwrap();
    assert!(transfer(&f, JobKind::Copy, "absent.txt", OverwritePolicy::Overwrite).is_err());
    assert_eq!(fs::read_link(f.path("dst/absent.txt")).unwrap(), Path::new("../precious.txt"));
    assert_eq!(fs::read(f.path("precious.txt")).unwrap(), b"PRECIOUS");
}
