//! Real PTY lifecycle attacks; fixture files synchronize helper output completion.
use ira_parity::lifecycle::{PtySession, PtySessionConfig};
use portable_pty::{CommandBuilder, PtySize};
use std::{
    io::Write,
    time::{Duration, Instant},
};

const MODE: &str = "IRA_ADV_LIFECYCLE_MODE";
const ROOT: &str = "IRA_ADV_LIFECYCLE_ROOT";
const PAYLOAD: usize = 1024 * 1024;

#[test]
#[allow(clippy::zombie_processes)] // The helper is intentionally orphaned when its PTY parent is terminated.
fn lifecycle_attack_helper() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os(ROOT).unwrap());
    match mode.as_str() {
        "flood" => {
            let mut out = std::io::stdout().lock();
            out.write_all(&vec![b'Q'; PAYLOAD]).unwrap();
            out.flush().unwrap();
            std::fs::write(root.join("output-produced"), b"done").unwrap();
        }
        #[cfg(unix)]
        "parent" => {
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "lifecycle_attack_helper", "--nocapture"])
                .env(MODE, "descendant")
                .spawn()
                .unwrap();
            std::fs::write(root.join("descendant-pid"), child.id().to_string()).unwrap();
            loop {
                std::thread::park();
            }
        }
        #[cfg(unix)]
        "descendant" => {
            unsafe extern "C" {
                fn signal(signal: i32, handler: usize) -> usize;
            }
            unsafe {
                signal(1, 1);
            }
            std::fs::write(root.join("output-produced"), b"ready").unwrap();
            let end = Instant::now() + Duration::from_secs(10);
            while !root.join("release").exists() && Instant::now() < end {
                std::thread::yield_now();
            }
            std::fs::write(root.join("descendant-ended"), b"done").unwrap();
        }
        #[cfg(unix)]
        "ignore-hup" => {
            unsafe extern "C" {
                fn signal(signal: i32, handler: usize) -> usize;
            }
            // POSIX SIGHUP = 1, SIG_IGN = 1 on the Linux/macOS targets.
            unsafe {
                signal(1, 1);
            }
            println!("READY");
            std::io::stdout().flush().unwrap();
            loop {
                std::thread::park();
            }
        }
        "one" => {
            println!("READY");
            std::io::stdout().flush().unwrap();
            std::fs::write(root.join("output-produced"), b"done").unwrap();
        }
        _ => panic!("unexpected helper mode"),
    }
}

fn start(mode: &str, config: PtySessionConfig) -> PtySession {
    let fixture = tempfile::tempdir().unwrap();
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.args(["--exact", "lifecycle_attack_helper", "--nocapture"]);
    command.env(MODE, mode);
    command.env(ROOT, fixture.path());
    PtySession::spawn(
        command,
        fixture,
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        },
        config,
    )
    .unwrap()
}

fn config() -> PtySessionConfig {
    PtySessionConfig::new(Duration::from_secs(3), Duration::from_secs(2))
}

fn await_output_produced(session: &PtySession) {
    let end = Instant::now() + Duration::from_secs(8);
    while !session.fixture_path().join("output-produced").exists() {
        assert!(Instant::now() < end, "helper output deadline exceeded");
        std::thread::yield_now();
    }
}

// GAP-FIXED(G-F002-ADV-59) sev=high kind=behavior-divergence feature=F-002
//   what:     Joining a finished reader abandons bytes and errors still queued in its channel.
//   tui-ref:  migration/specs/F-002.md S8/S12
//   oracle:   real PTY helper writes exactly 1 MiB of Q and signals flush completion
//   repro:    Let reader enqueue the flood; shutdown, then inspect all accumulated output.
//   expected: Every written payload byte is drained before fixture removal.
//   actual:   shutdown stops receiving as soon as JoinHandle reports finished; queued data is lost.
//   cover:    shutdown_drains_all_queued_output_before_success
//   fixed-by: lifecycle follow-up; both adversarial 1 MiB output paths and queued failure pass.
#[test]
fn shutdown_drains_all_queued_output_before_success() {
    let mut session = start("flood", config());
    await_output_produced(&session);
    session.shutdown().unwrap();
    assert_eq!(
        session.output().iter().filter(|b| **b == b'Q').count(),
        PAYLOAD
    );
}

#[test]
fn child_exit_drain_consumes_entire_queue() {
    let mut session = start("flood", config());
    await_output_produced(&session);
    let output = session
        .wait_for_child_exit_and_drain(Duration::from_secs(5))
        .unwrap();
    let count = output.iter().filter(|b| **b == b'Q').count();
    session.shutdown().unwrap();
    assert_eq!(
        count, PAYLOAD,
        "ADV-59: child exit drain must consume the complete queued output"
    );
}

#[test]
fn shutdown_preserves_queued_reader_failure() {
    let mut session = start("one", config().with_reader_failure_after_bytes(1));
    await_output_produced(&session);
    let error = session
        .shutdown_with_primary_error("operation failed")
        .unwrap_err();
    assert_eq!(error.primary(), "operation failed");
    assert!(
        error.cleanup().contains("injected PTY reader failure"),
        "ADV-59: queued reader error lost: {error}"
    );
}

#[test]
fn duplicate_shutdown_is_safe_and_closed_io_fails() {
    let mut session = start("one", config());
    session
        .wait_for_output(b"READY", Duration::from_secs(5))
        .unwrap();
    session.shutdown().unwrap();
    session.shutdown().unwrap();
    assert!(session.write_input(b"a").unwrap_err().contains("closed"));
    assert!(session
        .resize(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0
        })
        .unwrap_err()
        .contains("closed"));
}

#[test]
fn partial_spawn_failure_removes_fixture() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().to_path_buf();
    let missing = path.join("missing-program");
    let result = PtySession::spawn(
        CommandBuilder::new(missing),
        fixture,
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        },
        config(),
    );
    assert!(result.is_err());
    assert!(!path.exists());
}

// GAP-FIXED(G-F002-ADV-60) sev=medium kind=behavior-divergence feature=F-002
//   what:     The cleanup deadline starts after a blocking portable-pty kill operation.
//   tui-ref:  migration/specs/F-002.md S8/S12; portable-pty 0.9.0 lib.rs ChildKiller::kill
//   oracle:   helper ignores SIGHUP; pinned dependency spends 200ms in its kill grace period
//   repro:    Shutdown with 20ms child/reader budgets after helper readiness.
//   expected: Absolute cleanup budgets include child termination, with scheduling tolerance.
//   actual:   Unix Child::kill sleeps four times before the harness starts its reap deadline.
//   cover:    termination_is_included_in_absolute_cleanup_deadline
//   fixed-by: lifecycle follow-up; Linux uses nonblocking cloned HUP signal and bounded SIGKILL escalation.
#[cfg(unix)]
#[test]
fn termination_is_included_in_absolute_cleanup_deadline() {
    let mut session = start(
        "ignore-hup",
        PtySessionConfig::new(Duration::from_millis(20), Duration::from_millis(20)),
    );
    session
        .wait_for_output(b"READY", Duration::from_secs(5))
        .unwrap();
    let begin = Instant::now();
    let result = session.shutdown();
    let elapsed = begin.elapsed();
    assert!(result.is_ok(), "cleanup failed: {result:?}");
    assert!(
        elapsed < Duration::from_millis(150),
        "ADV-60: 40ms budgets plus 110ms tolerance exceeded: {elapsed:?}"
    );
}

#[cfg(unix)]
#[test]
fn reader_timeout_then_drop_retains_fixture_until_descendant_released() {
    let mut session = start(
        "parent",
        PtySessionConfig::new(Duration::from_secs(1), Duration::from_millis(100)),
    );
    await_output_produced(&session);
    let path = session.fixture_path().to_path_buf();
    let begin = Instant::now();
    let error = session
        .shutdown_with_primary_error("scenario failed")
        .unwrap_err();
    assert!(begin.elapsed() < Duration::from_secs(2));
    assert_eq!(error.primary(), "scenario failed");
    assert!(error.child_reaped());
    assert!(error.cleanup().contains("reader cleanup deadline"));
    assert!(path.exists());
    drop(session);
    assert!(
        path.exists(),
        "Drop must retain a fixture while reader ownership remains"
    );
    std::fs::write(path.join("release"), b"release").unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    while !path.join("descendant-ended").exists() {
        assert!(Instant::now() < end, "descendant failed to stop");
        std::thread::yield_now();
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn duplicate_failed_cleanup_and_drop_preserve_fixture() {
    let mut session = start("one", config().with_fixture_remove_failure());
    session
        .wait_for_output(b"READY", Duration::from_secs(5))
        .unwrap();
    let path = session.fixture_path().to_path_buf();
    assert!(session
        .shutdown()
        .unwrap_err()
        .cleanup()
        .contains("fixture"));
    assert!(session
        .shutdown()
        .unwrap_err()
        .cleanup()
        .contains("fixture"));
    drop(session);
    assert!(path.exists());
    std::fs::remove_dir_all(path).unwrap();
}
