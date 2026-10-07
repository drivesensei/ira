use super::*;
use ira::services::persistence::PersistenceError;
use std::{
    cell::Cell,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ira-terminal-exit-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn app(&self, fail: bool) -> App {
        let mut app = App::default();
        let parent = self.0.join("parent");
        if fail {
            fs::write(&parent, b"old parent").unwrap();
        } else {
            fs::create_dir(&parent).unwrap();
        }
        app.state_path = Some(parent.join("state"));
        app
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn failed_state_write_returns_original_error_after_cleanup_once() {
    let f = Fixture::new();
    let mut app = f.app(true);
    let expected = app.try_persist_state().unwrap_err();
    let calls = Cell::new(0);
    let result = finish_exit(&mut app, |_| {
        calls.set(calls.get() + 1);
        Ok(())
    });
    assert_eq!(calls.get(), 1);
    let error = result.expect_err("failed session publication must not report successful exit");
    assert_eq!(error.downcast_ref::<PersistenceError>(), Some(&expected));
    assert_eq!(fs::read(f.0.join("parent")).unwrap(), b"old parent");
}
#[test]
fn successful_state_write_and_cleanup_finish_once() {
    let f = Fixture::new();
    let mut app = f.app(false);
    let calls = Cell::new(0);
    finish_exit(&mut app, |_| {
        calls.set(calls.get() + 1);
        Ok(())
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert!(fs::read(f.0.join("parent/state"))
        .unwrap()
        .starts_with(b"split=0\n"));
}
#[test]
fn cleanup_failure_after_successful_save_is_returned() {
    let f = Fixture::new();
    let mut app = f.app(false);
    let calls = Cell::new(0);
    let result = finish_exit(&mut app, |_| {
        calls.set(calls.get() + 1);
        Err(io::Error::other("synthetic cleanup failure").into())
    });
    assert_eq!(calls.get(), 1);
    assert_eq!(result.unwrap_err().to_string(), "synthetic cleanup failure");
    assert!(f.0.join("parent/state").exists());
}
#[test]
fn both_failures_preserve_original_save_error_and_still_cleanup_once() {
    let f = Fixture::new();
    let mut app = f.app(true);
    let expected = app.try_persist_state().unwrap_err();
    let calls = Cell::new(0);
    let result = finish_exit(&mut app, |_| {
        calls.set(calls.get() + 1);
        Err(io::Error::other("synthetic cleanup failure").into())
    });
    assert_eq!(calls.get(), 1);
    let error = result.unwrap_err();
    assert_eq!(error.downcast_ref::<PersistenceError>(), Some(&expected));
    assert_eq!(fs::read(f.0.join("parent")).unwrap(), b"old parent");
}
