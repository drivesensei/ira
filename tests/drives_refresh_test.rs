use std::time::{Duration, Instant};

use ira::app::App;

/// The drive list is now produced by a background poller, so after
/// `tick`/`refresh_drives` it appears within a couple of seconds, never
/// blocking the render thread. This test tolerates that async arrival.
#[test]
fn tick_leaves_drive_list_populated_after_poll() {
    let mut app = App::new(); // spawns the background drive poller
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        app.tick(); // drains jobs + consumes the latest polled drives
        if app.drives.is_some() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "drive poller never published a drive list within 6s"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn refresh_drives_never_blocks() {
    let mut app = App::default(); // no poller running
    let t = Instant::now();
    app.refresh_drives();
    // With no background poller, refresh must return immediately and not
    // block on lsblk (which would be a regression).
    assert!(t.elapsed() < Duration::from_millis(50));
}