use ira::app::App;

#[test]
fn refresh_drives_populates_the_drive_list() {
    let mut app = App::default();
    assert!(app.drives.is_none());
    app.refresh_drives();
    assert!(
        app.drives.is_some(),
        "refresh_drives should populate the drive list from the platform"
    );
}

#[test]
fn tick_refreshes_drives() {
    let mut app = App::default();
    app.tick(); // exercises drain_jobs + refresh_drives
    assert!(
        app.drives.is_some(),
        "tick should leave the drive list populated"
    );
}