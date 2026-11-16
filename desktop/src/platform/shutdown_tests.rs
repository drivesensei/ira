use super::*;
use crate::runtime::{Command, Completion};
use ira_core::application::App;
fn actor(state: std::path::PathBuf, bookmarks: std::path::PathBuf) -> Runtime {
    let mut app = App::default();
    app.state_path = Some(state);
    app.bookmarks_path = Some(bookmarks);
    Runtime::with_factory(7, move || app)
}
#[test]
fn failed_actual_actor_receipt_never_closes_and_retries_original_epoch() {
    let _scope = crate::test_support::enter();
    let fixture = crate::test_support::current().unwrap().directory.clone();
    let parent = fixture.join("blocked-config");
    std::fs::write(&parent, b"task-only obstruction").unwrap();
    let mut runtime = actor(parent.join("state"), fixture.join("bookmarks"));
    let writer = Writer::new(None); // explicitly disabled, never default config
    let mut coordinator = Coordinator::new(&writer);
    runtime.stop(&[]);
    let deadline = Instant::now() + Duration::from_secs(3);
    let failure = loop {
        if let Some(ShutdownState::Failure(error)) = runtime.shutdown_state() {
            break error;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    };
    assert!(matches!(
        coordinator.poll(&runtime),
        Status::PendingError(_)
    ));
    assert!(!runtime.shutdown_complete());
    while let Some(completion) = runtime.try_completion() {
        assert!(
            !matches!(completion, Completion::Closed),
            "failed write is never Closed"
        );
    }
    let before = runtime.enqueue(Command::HostResult(Ok(())), None);
    assert!(runtime.is_stopping()); // post-stop gateway admits no command
    assert!(before > 0);
    std::fs::remove_file(&parent).unwrap();
    std::fs::create_dir(&parent).unwrap();
    coordinator.retry(&runtime);
    let deadline = Instant::now() + Duration::from_secs(3);
    while coordinator.poll(&runtime) != Status::Ready {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(
        matches!(runtime.shutdown_state(), Some(ShutdownState::Success { epoch }) if epoch == failure.epoch)
    );
    assert!(parent.join("state").is_file());
    assert!(runtime.shutdown_complete());
    // No job/editor commands were enqueued or replayed; task files are retained.
}
#[test]
fn original_geometry_receipt_survives_budget_and_late_success_is_observed() {
    let _scope = crate::test_support::enter();
    let fixture = crate::test_support::current().unwrap().directory.clone();
    let runtime = actor(fixture.join("state"), fixture.join("bookmarks"));
    runtime.stop(&[]);
    let writer = Writer::new(Some(fixture.join("desktop-window")));
    writer.save(super::super::geometry::Geometry {
        x: 42.,
        y: 0.,
        width: 1080.,
        height: 720.,
        mode: 0,
    });
    let actual = writer.final_receipt();
    let (release, wait) = mpsc::channel();
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let checked = actual.recv().unwrap(); // actual publisher outcome, not fabricated success
        wait.recv().unwrap();
        send.send(checked).unwrap();
    });
    let mut coordinator = Coordinator {
        receipt: receive,
        geometry: None,
        disconnected: false,
        started: Instant::now(),
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    while !runtime.shutdown_complete() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    while coordinator.started.elapsed() < Duration::from_millis(2100) {
        assert_ne!(coordinator.poll(&runtime), Status::Ready);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(
        coordinator.poll(&runtime),
        Status::PendingError(_)
    ));
    coordinator.retry(&runtime); // pending receipt stays ORIGINAL, no geometry recapture
    assert!(coordinator.geometry.is_none());
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while coordinator.poll(&runtime) != Status::Ready {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(coordinator.geometry.as_ref().unwrap().as_ref().unwrap(), &1);
    assert!(fixture.join("desktop-window").is_file());
}
