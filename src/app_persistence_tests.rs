use super::*;
#[test]
fn terminal_checked_state_failure_is_reported_and_visible_without_saved_claim() {
    let directory = std::env::temp_dir().join(format!(
        "ira-terminal-persistence-feedback-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let blocked = directory.join("blocked");
    std::fs::write(&blocked, b"old parent").unwrap();
    let mut app = App::default();
    app.state_path = Some(blocked.join("state"));
    assert!(app.try_persist_state().is_err());
    app.cycle_theme();
    let status = app.status.as_ref().unwrap();
    assert!(status.is_error);
    assert!(status.text.contains("Persistence failed"));
    assert_eq!(std::fs::read(&blocked).unwrap(), b"old parent");
    drop(app);
    std::fs::remove_dir_all(directory).unwrap();
}
