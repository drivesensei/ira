use super::*;
fn value(x: f32) -> Geometry {
    Geometry {
        x,
        y: 0.,
        width: 1080.,
        height: 720.,
        mode: 0,
    }
}
#[test]
fn failed_geometry_receipt_survives_writer_and_retries_frozen_value() {
    let _scope = crate::test_support::enter();
    let fixture = crate::test_support::current().unwrap().directory.clone();
    let parent = fixture.join("blocked");
    fs::write(&parent, b"temporary obstruction").unwrap();
    let path = parent.join("desktop-window");
    let writer = Writer::new(Some(path.clone()));
    writer.save(value(42.));
    let original = writer.checked_barrier();
    let failure = original
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
        .unwrap_err();
    assert_eq!(failure.epoch, 1);
    drop(writer);
    fs::remove_file(&parent).unwrap();
    fs::create_dir(&parent).unwrap();
    let receipt = failure
        .retry
        .retry()
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
        .unwrap();
    assert_eq!(receipt.epoch, failure.epoch);
    assert_eq!(
        Geometry::parse(&fs::read_to_string(&path).unwrap())
            .unwrap()
            .x,
        42.
    );
    // A successful retry is idempotent, not a second filesystem publication.
    fs::remove_file(&path).unwrap();
    assert_eq!(
        failure
            .retry
            .retry()
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap()
            .unwrap()
            .epoch,
        1
    );
    assert!(!path.exists());
}
#[test]
fn old_geometry_failure_cannot_overwrite_newer_success() {
    let _scope = crate::test_support::enter();
    let fixture = crate::test_support::current().unwrap().directory.clone();
    let parent = fixture.join("blocked");
    fs::write(&parent, b"temporary obstruction").unwrap();
    let path = parent.join("desktop-window");
    let writer = Writer::new(Some(path.clone()));
    writer.save(value(42.));
    let failure = writer
        .checked_barrier()
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
        .unwrap_err();
    fs::remove_file(&parent).unwrap();
    fs::create_dir(&parent).unwrap();
    writer.save(value(99.));
    let receipt = writer
        .checked_barrier()
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
        .unwrap();
    assert_eq!(receipt.epoch, 2);
    let stale = failure
        .retry
        .retry()
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap()
        .unwrap_err();
    assert!(stale.message.contains("superseded"));
    assert_eq!(
        Geometry::parse(&fs::read_to_string(&path).unwrap())
            .unwrap()
            .x,
        99.
    );
}
