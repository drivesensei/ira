//! Recovery is automatic after bounded foreground cleanup, not caller-managed retry.
#![cfg(unix)]
use ira_parity::lifecycle::{PtySession, PtySessionConfig};
use portable_pty::{CommandBuilder, PtySize};
use std::{
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};
const MODE: &str = "IRA_T017_RECOVERY_HELPER";
#[test]
fn recovery_helper() {
    if std::env::var(MODE).as_deref() != Ok("hold") {
        return;
    }
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }
    println!("READY {}", std::process::id());
    std::io::stdout().flush().unwrap();
    loop {
        std::thread::park();
    }
}
fn start(config: PtySessionConfig) -> PtySession {
    let fixture = tempfile::tempdir().unwrap();
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.args(["--exact", "recovery_helper", "--nocapture"]);
    command.env_clear();
    command.env(MODE, "hold");
    for key in [
        "HOME",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_DATA_HOME",
        "TMPDIR",
    ] {
        let p = fixture.path().join(key);
        std::fs::create_dir(&p).unwrap();
        command.env(key, p);
    }
    command.env("TERM", "xterm-256color");
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
fn pid(session: &mut PtySession) -> libc::pid_t {
    let out = session
        .wait_for_output(b"READY ", Duration::from_secs(5))
        .unwrap();
    let text = String::from_utf8_lossy(&out);
    text.split("READY ")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap()
}
fn await_removed(path: &std::path::Path) {
    let end = Instant::now() + Duration::from_secs(3);
    while path.exists() {
        assert!(
            Instant::now() < end,
            "active cleanup did not remove fixture"
        );
        std::thread::yield_now();
    }
}
struct FailureCleanup {
    pid: libc::pid_t,
    path: PathBuf,
}
impl Drop for FailureCleanup {
    fn drop(&mut self) {
        if self.path.exists() {
            // Test-only failure cleanup owns this synthetic PID and fixture.
            unsafe {
                libc::kill(self.pid, libc::SIGKILL);
                libc::waitpid(self.pid, std::ptr::null_mut(), 0);
            }
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
#[test]
fn zero_budget_drop_eventually_reaps_and_removes_fixture_without_retry() {
    let mut session = start(PtySessionConfig::new(
        Duration::ZERO,
        Duration::from_millis(20),
    ));
    let pid = pid(&mut session);
    let path = session.fixture_path().to_path_buf();
    let _guard = FailureCleanup {
        pid,
        path: path.clone(),
    };
    let begin = Instant::now();
    let error = session.shutdown().unwrap_err();
    assert!(error.cleanup().contains("child cleanup deadline"));
    assert!(path.exists());
    drop(session);
    assert!(begin.elapsed() < Duration::from_millis(150));
    await_removed(&path);
    let mut status = 0;
    let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
    assert_eq!(
        result, -1,
        "deferred owner must reap before fixture removal"
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}
#[test]
fn reap_gate_distinguishes_deadline_expiry_from_eventual_owned_recovery() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let gate = Arc::new(AtomicBool::new(false));
    let mut session = start(
        PtySessionConfig::new(Duration::from_millis(20), Duration::from_millis(20))
            .with_reap_poll_gate(gate.clone()),
    );
    let pid = pid(&mut session);
    let path = session.fixture_path().to_owned();
    let _guard = FailureCleanup {
        pid,
        path: path.clone(),
    };
    let begin = Instant::now();
    let error = session
        .shutdown_with_primary_error("scenario deadline")
        .unwrap_err();
    assert_eq!(error.primary(), "scenario deadline");
    assert!(!error.child_reaped());
    assert!(error.cleanup().contains("child cleanup deadline"));
    drop(session);
    assert!(begin.elapsed() < Duration::from_millis(150));
    assert!(path.exists());
    gate.store(true, Ordering::Release);
    await_removed(&path);
    let result = unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) };
    assert_eq!(result, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}
#[test]
fn default_controlling_terminal_shutdown_is_retained_as_separate_case() {
    let mut session = start(PtySessionConfig::new(
        Duration::from_millis(20),
        Duration::from_millis(20),
    ));
    let _ = pid(&mut session);
    let path = session.fixture_path().to_owned();
    let report = session.shutdown().unwrap();
    assert!(report.child_reaped && report.reader_joined);
    assert!(!path.exists());
}
#[test]
fn exact_twenty_millisecond_shutdown_budgets_pass_250_native_repeats() {
    for iteration in 0..250 {
        let mut session = start(PtySessionConfig::new(
            Duration::from_millis(20),
            Duration::from_millis(20),
        ));
        let pid = pid(&mut session);
        let path = session.fixture_path().to_owned();
        let _guard = FailureCleanup { pid, path };
        let begin = Instant::now();
        let result = session.shutdown();
        assert!(result.is_ok(), "iteration {iteration}: {result:?}");
        assert!(begin.elapsed() < Duration::from_millis(150));
    }
}
