use super::*;
use std::fs;
fn fixture() -> (PathBuf, App) {
    let directory = crate::services::persistence::test_path("receipt").join(format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&directory).unwrap();
    let mut app = App::default();
    app.state_path = Some(directory.join("state"));
    app.bookmarks_path = Some(directory.join("bookmarks"));
    (directory, app)
}
#[test]
fn checked_barrier_retains_write_failure_across_later_barriers_and_actor_stop_retry() {
    let (directory, mut app) = fixture();
    let parent = directory.join("blocked");
    fs::write(&parent, b"parent").unwrap();
    app.state_path = Some(parent.join("state"));
    app.split = true;
    app.persist_state();
    let original = app.checked_persistence_barrier();
    let failure = original
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_err();
    assert_eq!(failure.epoch, 1);
    assert!(!failure.errors.is_empty());
    assert!(app
        .checked_persistence_barrier()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .is_err());
    drop(app);
    fs::remove_file(&parent).unwrap();
    fs::create_dir(&parent).unwrap();
    assert_eq!(
        failure
            .retry
            .retry()
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap()
            .epoch,
        1
    );
    let bytes = fs::read(parent.join("state")).unwrap();
    assert!(bytes.starts_with(b"split=1\n"));
    failure
        .retry
        .retry()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert_eq!(fs::read(parent.join("state")).unwrap(), bytes);
    fs::remove_dir_all(directory).unwrap();
}
#[test]
fn later_success_does_not_let_stale_retry_overwrite_newer_snapshot() {
    let (directory, mut app) = fixture();
    let parent = directory.join("blocked");
    fs::write(&parent, b"x").unwrap();
    app.state_path = Some(parent.join("state"));
    app.split = true;
    app.persist_state();
    let old = app
        .checked_persistence_barrier()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_err();
    fs::remove_file(&parent).unwrap();
    fs::create_dir(&parent).unwrap();
    app.split = false;
    app.persist_state();
    let success = app
        .checked_persistence_barrier()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert_eq!(success.epoch, 2);
    old.retry
        .retry()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert!(fs::read(parent.join("state"))
        .unwrap()
        .starts_with(b"split=0\n"));
    drop(app);
    fs::remove_dir_all(directory).unwrap();
}
#[test]
fn checked_receipt_aggregates_state_and_bookmarks_and_legacy_barrier_is_only_processing() {
    let (directory, mut app) = fixture();
    let parent = directory.join("blocked");
    fs::write(&parent, b"x").unwrap();
    app.state_path = Some(parent.join("state"));
    app.bookmarks_path = Some(parent.join("bookmarks"));
    app.bookmarks = Some(vec![Folder::new("雪".into(), "/synthetic".into(), 'o')]);
    app.persist_state();
    app.persist_bookmarks();
    app.persistence_barrier()
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let failure = app
        .checked_persistence_barrier()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap_err();
    assert_eq!(failure.epoch, 2);
    assert_eq!(failure.errors.len(), 2);
    drop(app);
    fs::remove_file(&parent).unwrap();
    fs::create_dir(&parent).unwrap();
    failure
        .retry
        .retry()
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert_eq!(
        fs::read(parent.join("bookmarks")).unwrap(),
        "雪\t/synthetic\n".as_bytes()
    );
    fs::remove_dir_all(directory).unwrap();
}
