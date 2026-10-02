//! Compile-time red contract for the session extracted from
//! `runner::run_oracle_with_baseline`. This target intentionally imports the
//! smallest proposed test-facing API. On the preserved base it must fail to
//! compile because the extracted session module does not exist yet.

use ira_parity::lifecycle::{CleanupEvent, PtySession, PtySessionConfig};
use portable_pty::{CommandBuilder, PtySize};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const HELPER_MODE: &str = "IRA_PARITY_PTY_HELPER_MODE";

#[test]
fn helper_child_entry() {
    match std::env::var(HELPER_MODE).as_deref() {
        Ok("success") => println!("IRA_HELPER_READY_SUCCESS"),
        Ok("hold") => {
            println!("IRA_HELPER_READY_HOLD");
            loop {
                thread::park();
            }
        }
        Ok("flood") => {
            let block = "x".repeat(1024);
            for _ in 0..256 {
                println!("{block}");
            }
        }
        _ => {}
    }
}

fn helper(mode: &str) -> CommandBuilder {
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.args(["--exact", "helper_child_entry", "--nocapture"]);
    command.env(HELPER_MODE, mode);
    command
}

fn config() -> PtySessionConfig {
    PtySessionConfig::new(Duration::from_secs(3), Duration::from_secs(2))
}

fn size() -> PtySize {
    PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[test]
fn extracted_session_success_reaps_reader_and_removes_fixture() {
    let fixture = tempfile::tempdir().unwrap();
    let fixture_path = fixture.path().to_path_buf();
    let mut session = PtySession::spawn(helper("success"), fixture, size(), config()).unwrap();
    session
        .wait_for_output(b"IRA_HELPER_READY_SUCCESS", Duration::from_secs(5))
        .unwrap();
    let report = session.shutdown().unwrap();
    assert!(report.child_reaped);
    assert!(report.reader_joined);
    assert!(report.handles_closed_before_fixture_removed);
    assert!(!fixture_path.exists());
}

#[test]
fn extracted_session_timeout_terminates_live_child_within_bound() {
    let fixture = tempfile::tempdir().unwrap();
    let mut session = PtySession::spawn(helper("hold"), fixture, size(), config()).unwrap();
    session
        .wait_for_output(b"IRA_HELPER_READY_HOLD", Duration::from_secs(5))
        .unwrap();
    let started = Instant::now();
    let report = session.shutdown().unwrap();
    assert!(started.elapsed() < Duration::from_secs(4));
    assert!(report.child_reaped);
    assert!(report.reader_joined);
}

#[test]
fn extracted_session_observes_child_exit_while_reader_is_active() {
    let fixture = tempfile::tempdir().unwrap();
    let mut session = PtySession::spawn(helper("flood"), fixture, size(), config()).unwrap();
    let output = session
        .wait_for_child_exit_and_drain(Duration::from_secs(8))
        .unwrap();
    assert!(output.len() >= 200 * 1024);
    let report = session.shutdown().unwrap();
    assert!(report.reader_joined);
    assert!(report.child_reaped);
}

#[test]
fn extracted_session_reports_reader_failure_and_preserves_primary_error() {
    let fixture = tempfile::tempdir().unwrap();
    let mut session = PtySession::spawn(
        helper("hold"),
        fixture,
        size(),
        config().with_reader_failure_after_bytes(1),
    )
    .unwrap();
    let failure = session
        .wait_for_output(b"NEVER", Duration::from_millis(100))
        .expect_err("injected PTY reader failure must surface");
    let error = session.shutdown_with_primary_error(failure).unwrap_err();
    assert!(error.primary().contains("reader"));
    assert!(error.cleanup().contains("reader"));
    assert!(error.cleanup().contains("reap") || error.child_reaped());
}

#[test]
fn extracted_session_records_teardown_order_and_composes_cleanup_error() {
    let fixture = tempfile::tempdir().unwrap();
    let fixture_path: PathBuf = fixture.path().to_path_buf();
    let mut session = PtySession::spawn(
        helper("hold"),
        fixture,
        size(),
        config().with_fixture_remove_failure(),
    )
    .unwrap();
    session
        .wait_for_output(b"IRA_HELPER_READY_HOLD", Duration::from_secs(5))
        .unwrap();
    let error = session
        .shutdown_with_primary_error("scenario assertion failed")
        .unwrap_err();
    assert!(error.primary().contains("scenario assertion failed"));
    assert!(error.cleanup().contains("fixture"));
    let events = error.events();
    assert!(
        events.iter().position(|e| *e == CleanupEvent::ChildReaped)
            < events
                .iter()
                .position(|e| *e == CleanupEvent::HandlesClosed)
    );
    assert!(
        events
            .iter()
            .position(|e| *e == CleanupEvent::HandlesClosed)
            < events.iter().position(|e| *e == CleanupEvent::ReaderJoined)
    );
    assert!(
        events.iter().position(|e| *e == CleanupEvent::ReaderJoined)
            < events
                .iter()
                .position(|e| *e == CleanupEvent::FixtureRemovalAttempted)
    );
    assert!(
        fixture_path.exists(),
        "injected removal failure should preserve fixture"
    );
    std::fs::remove_dir_all(&fixture_path).unwrap();
}
