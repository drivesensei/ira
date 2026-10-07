use super::*;

// Serialize these synthetic blockers, while still allowing real peer probes to
// compete for the production lane. Admission is driven by actual host ticks.
static FIXTURE_LOCK: Mutex<()> = Mutex::new(());
fn fixture_lock() -> std::sync::MutexGuard<'static, ()> {
    FIXTURE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
fn drive_until_callback<T>(
    app: &mut App,
    rx: &mpsc::Receiver<T>,
    budget: Duration,
) -> Result<T, mpsc::TryRecvError> {
    let deadline = Instant::now() + budget;
    loop {
        app.tick();
        match rx.try_recv() {
            Ok(value) => return Ok(value),
            Err(mpsc::TryRecvError::Disconnected) => return Err(mpsc::TryRecvError::Disconnected),
            Err(mpsc::TryRecvError::Empty) if Instant::now() >= deadline => {
                return Err(mpsc::TryRecvError::Empty)
            }
            Err(mpsc::TryRecvError::Empty) => thread::yield_now(),
        }
    }
}
fn drive_until_spawn_attempt(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        app.tick();
        if app.transfer_probe_finished.is_some() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "owned spawn failure was never admitted"
        );
        thread::yield_now();
    }
}

fn running_app() -> App {
    let mut app = App::default();
    app.jobs.push(Job {
        id: 1,
        kind: JobKind::Copy,
        overwrite: OverwritePolicy::SkipExisting,
        paths: vec!["fixture-source/item.txt".into()],
        dest_dir: "fixture-destination".into(),
        label: "item.txt".into(),
        total_bytes: Some(1),
        copied_bytes: 0,
        current: String::new(),
        status: JobStatus::Running,
        started_at: Instant::now(),
        control: JobControl::new(),
    });
    app.transfer_dest = Some(TransferDestSync {
        dest_dir: "fixture-destination".into(),
        reveal_path: "fixture-destination/item.txt".into(),
        last_refresh: Instant::now() - Duration::from_secs(2),
    });
    app
}

#[test]
fn tick_returns_while_transfer_metadata_is_blocked() {
    let _fixture = fixture_lock();
    let mut app = running_app();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    let returned = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed_returned = Arc::clone(&returned);
    let release_rx = Arc::new(Mutex::new(release_rx));
    app.transfer_probe_test = Some(Arc::new(move |_| {
        entered_tx.send(()).unwrap();
        release_rx
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        true
    }));
    // Controller always releases, even if admission or the original 50ms check fails.
    let controller = thread::spawn(move || {
        let entered = entered_rx.recv_timeout(Duration::from_secs(1)).is_ok();
        let _ = ready_tx.send(());
        let deadline = Instant::now() + Duration::from_millis(50);
        while entered
            && !observed_returned.load(std::sync::atomic::Ordering::Acquire)
            && Instant::now() < deadline
        {
            thread::yield_now();
        }
        let returned = entered && observed_returned.load(std::sync::atomic::Ordering::Acquire);
        let _ = release_tx.send(());
        (entered, returned)
    });
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        returned.store(false, std::sync::atomic::Ordering::Release);
        app.tick();
        returned.store(true, std::sync::atomic::Ordering::Release);
        if ready_rx.try_recv().is_ok() || Instant::now() >= deadline {
            break;
        }
        thread::yield_now();
    }
    let (entered, returned) = controller.join().unwrap();
    assert!(entered, "metadata seam must have been entered");
    assert!(returned, "tick blocked until metadata release");
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("ira-t041-probe-{}-{id}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        for name in ["a.txt", "item.txt"] {
            std::fs::write(path.join(name), "fixture").unwrap();
        }
        Self(path)
    }
    fn app(&self) -> App {
        let mut app = running_app();
        let path = self.0.to_string_lossy().into_owned();
        app.jobs[0].dest_dir = path.clone();
        app.transfer_dest.as_mut().unwrap().dest_dir = path.clone();
        app.transfer_dest.as_mut().unwrap().reveal_path =
            self.0.join("item.txt").to_string_lossy().into_owned();
        app.panes[1].folder = Some(Folder::new("destination".into(), path, '#'));
        app.panes[1].files = ["a.txt", "item.txt"]
            .into_iter()
            .map(|name| FEntry {
                path: self.0.join(name).to_string_lossy().into_owned(),
                label: name.into(),
                is_dir: false,
                size: 7,
                modified: None,
            })
            .collect();
        app.panes[1].selected = vec![true, false];
        app.panes[1].state.select(Some(0));
        app.panes[1].render_scroll = 17;
        app
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn queue_result(app: &mut App, exists: bool) {
    app.transfer_probe_pending = Some(app.transfer_generation);
    app.transfer_probe_last_attempt = Some(Instant::now());
    app.transfer_probe_tx
        .send((
            app.transfer_generation,
            app.transfer_dest.as_ref().unwrap().clone(),
            app.panes.each_ref().map(|p| p.listing_generation),
            exists,
        ))
        .unwrap();
}

#[test]
fn accepted_probe_preserves_reveal_and_settles_selected_row() {
    let _fixture = fixture_lock();
    let fixture = Fixture::new();
    let mut app = fixture.app();
    queue_result(&mut app, true);
    app.tick();
    assert_eq!(app.panes[1].state.selected(), Some(1));
    assert!(app.panes[1].files[1].path.ends_with("item.txt"));
    assert_eq!(app.panes[1].selected, vec![false, false]);
    assert!(app.panes[1].pending_select.is_none());
    assert!(app.file_list_settled(1));
    assert_eq!(app.panes[1].render_scroll, 0);
}

#[test]
fn user_navigation_while_probe_pending_does_not_yank_cursor_or_selection() {
    let _fixture = fixture_lock();
    let fixture = Fixture::new();
    let mut app = fixture.app();
    queue_result(&mut app, true);
    app.panes[1].user_navigated = true;
    app.tick();
    assert_eq!(app.panes[1].state.selected(), Some(0));
    assert_eq!(app.panes[1].selected, vec![true, false]);
    assert_eq!(app.panes[1].render_scroll, 17);
    assert!(app.panes[1].pending_select.is_none());
    assert_eq!(app.panes[1].listing_generation, 0);
}

#[test]
fn navigation_away_does_not_refresh_current_folder() {
    let _fixture = fixture_lock();
    let fixture = Fixture::new();
    let mut app = fixture.app();
    queue_result(&mut app, true);
    app.panes[1].folder = Some(Folder::new("other".into(), "other-folder".into(), '#'));
    app.tick();
    assert_eq!(app.panes[1].listing_generation, 0);
    assert_eq!(app.panes[1].selected, vec![true, false]);
}

#[test]
fn away_and_back_or_new_listing_invalidates_probe_for_that_pane() {
    let _fixture = fixture_lock();
    let fixture = Fixture::new();
    let mut app = fixture.app();
    queue_result(&mut app, true);
    app.panes[1].listing_generation += 1;
    app.tick();
    assert_eq!(app.panes[1].listing_generation, 1);
    assert_eq!(app.panes[1].state.selected(), Some(0));
    assert_eq!(app.panes[1].selected, vec![true, false]);
    assert_eq!(app.panes[1].render_scroll, 17);
}

#[test]
fn changed_sync_identity_rejects_old_destination_reveal_or_refresh_time() {
    let _fixture = fixture_lock();
    for dimension in 0..3 {
        let fixture = Fixture::new();
        let mut app = fixture.app();
        queue_result(&mut app, true);
        let sync = app.transfer_dest.as_mut().unwrap();
        match dimension {
            0 => sync.dest_dir.push_str("/other"),
            1 => sync.reveal_path.push_str(".other"),
            _ => sync.last_refresh = Instant::now(),
        }
        app.tick();
        assert_eq!(app.panes[1].listing_generation, 0);
        assert_eq!(app.panes[1].selected, vec![true, false]);
    }
}

#[test]
fn superseded_generation_does_not_clear_current_pending_probe() {
    let _fixture = fixture_lock();
    let fixture = Fixture::new();
    let mut app = fixture.app();
    queue_result(&mut app, true);
    app.invalidate_transfer_probe();
    app.transfer_probe_pending = Some(app.transfer_generation);
    app.transfer_probe_last_attempt = Some(Instant::now());
    app.tick();
    assert_eq!(app.transfer_probe_pending, Some(app.transfer_generation));
    assert_eq!(app.panes[1].listing_generation, 0);
}

#[test]
fn terminal_events_invalidate_probe_before_late_install() {
    let _fixture = fixture_lock();
    for event in [
        JobEvent::Done { id: 1 },
        JobEvent::Cancelled { id: 1 },
        JobEvent::Failed {
            id: 1,
            error: "synthetic".into(),
        },
    ] {
        let mut app = running_app();
        queue_result(&mut app, true);
        let old = app.transfer_generation;
        app.job_tx.send(event).unwrap();
        app.tick();
        assert!(app.transfer_generation > old);
        assert!(app.transfer_dest.is_none());
        assert!(app.transfer_probe_pending.is_none());
        assert!(!matches!(
            app.jobs[0].status,
            JobStatus::Running | JobStatus::Paused
        ));
    }
}

#[test]
fn direct_terminal_state_rejects_queued_result() {
    let _fixture = fixture_lock();
    let fixture = Fixture::new();
    let mut app = fixture.app();
    queue_result(&mut app, true);
    app.jobs[0].status = JobStatus::Done;
    app.tick();
    assert!(app.transfer_dest.is_none());
    assert!(app.transfer_probe_pending.is_none());
    assert_eq!(app.panes[1].selected, vec![true, false]);
}

#[test]
fn missing_destination_preserves_listing_and_attempt_throttle() {
    let _fixture = fixture_lock();
    let fixture = Fixture::new();
    let mut app = fixture.app();
    queue_result(&mut app, false);
    app.tick();
    assert_eq!(app.panes[1].listing_generation, 0);
    assert_eq!(app.panes[1].selected, vec![true, false]);
    let (called_tx, called_rx) = mpsc::channel();
    app.transfer_probe_test = Some(Arc::new(move |_| {
        called_tx.send(()).unwrap();
        false
    }));
    for _ in 0..8 {
        app.tick();
    }
    assert!(called_rx.try_recv().is_err());
    app.transfer_probe_last_attempt = None;
    drive_until_callback(&mut app, &called_rx, Duration::from_secs(1)).unwrap();
}

#[test]
fn pending_probe_bounds_in_flight_work_including_paused_job() {
    let _fixture = fixture_lock();
    let mut app = running_app();
    app.jobs[0].status = JobStatus::Paused;
    app.transfer_probe_pending = Some(app.transfer_generation);
    let (called_tx, called_rx) = mpsc::channel();
    app.transfer_probe_test = Some(Arc::new(move |_| {
        called_tx.send(()).unwrap();
        false
    }));
    for _ in 0..8 {
        app.tick();
    }
    assert!(called_rx.try_recv().is_err());
    app.transfer_probe_pending = None;
    drive_until_callback(&mut app, &called_rx, Duration::from_secs(1)).unwrap();
}

#[test]
fn superseded_probe_does_not_retarget_new_transfer() {
    let _fixture = fixture_lock();
    let fixture = Fixture::new();
    let mut app = fixture.app();
    queue_result(&mut app, true);
    let dest = fixture.0.join("new-destination");
    std::fs::create_dir(&dest).unwrap();
    let old_generation = app.transfer_generation;
    app.spawn_transfer_jobs(
        JobKind::Copy,
        vec![fixture.0.join("a.txt").to_string_lossy().into_owned()],
        dest.to_string_lossy().into_owned(),
        OverwritePolicy::SkipExisting,
    );
    assert!(app.transfer_generation > old_generation);
    app.tick();
    assert_eq!(app.panes[1].listing_generation, 0);
    // Wait for this owned one-file worker before fixture cleanup.
    let deadline = Instant::now() + Duration::from_secs(2);
    while matches!(app.jobs[1].status, JobStatus::Running) && Instant::now() < deadline {
        app.tick();
        thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(app.jobs[1].status, JobStatus::Done);
}

#[test]
fn viewing_destination_subfolder_preserves_current_entry() {
    let _fixture = fixture_lock();
    let fixture = Fixture::new();
    let mut app = fixture.app();
    let inside = fixture.0.join("inside");
    std::fs::create_dir(&inside).unwrap();
    std::fs::write(inside.join("kept.txt"), "kept").unwrap();
    let kept = inside.join("kept.txt").to_string_lossy().into_owned();
    app.panes[1].folder = Some(Folder::new(
        "inside".into(),
        inside.to_string_lossy().into_owned(),
        '#',
    ));
    app.panes[1].files = vec![FEntry {
        path: kept.clone(),
        label: "kept.txt".into(),
        is_dir: false,
        size: 4,
        modified: None,
    }];
    app.panes[1].selected = vec![true];
    queue_result(&mut app, true);
    app.tick();
    assert_eq!(app.panes[1].state.selected(), Some(0));
    assert_eq!(app.panes[1].files[0].path, kept);
    assert!(app.panes[1].pending_select.is_none());
    assert!(app.file_list_settled(1));
}

#[path = "app_transfer_probe_occupancy_tests.rs"]
mod occupancy;
