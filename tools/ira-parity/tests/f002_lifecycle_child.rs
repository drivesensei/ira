//! Real child/PTY primitives for F-002's lifecycle test fixture.
//!
//! These tests prove only that the helper protocol can exercise a real child
//! through `portable-pty`; they do not exercise the still-missing extracted
//! `PtySession` implementation. That implementation's obligations are covered
//! by the compile-time contract in `f002_lifecycle_session.rs`.

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::Read;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const MODE: &str = "IRA_PARITY_PTY_HELPER_MODE";

/// The PTY parent starts this same test executable with `--exact` and selects
/// one helper mode through an environment variable. On an ordinary cargo test
/// run the variable is absent and this fixture exits immediately.
#[test]
fn helper_child_entry() {
    match std::env::var(MODE).as_deref() {
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

fn spawn(mode: &str) -> (Box<dyn Child + Send + Sync>, Box<dyn MasterPty + Send>) {
    let system = native_pty_system();
    let pair = system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open PTY");
    let mut command = CommandBuilder::new(current_test_exe());
    command.args(["--exact", "helper_child_entry", "--nocapture"]);
    command.env(MODE, mode);
    let child = pair
        .slave
        .spawn_command(command)
        .expect("spawn helper test child");
    drop(pair.slave);
    (child, pair.master)
}

fn current_test_exe() -> std::path::PathBuf {
    std::env::current_exe().expect("current test executable")
}

fn await_marker(rx: &mpsc::Receiver<Vec<u8>>, marker: &[u8], timeout: Duration) -> Vec<u8> {
    let end = Instant::now() + timeout;
    let mut seen = Vec::new();
    while Instant::now() < end {
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(20)) {
            seen.extend_from_slice(&chunk);
            if seen.windows(marker.len()).any(|window| window == marker) {
                return seen;
            }
        }
    }
    panic!(
        "timed out waiting for {:?}; received {} bytes",
        marker,
        seen.len()
    );
}

fn start_reader(master: &dyn MasterPty) -> (mpsc::Receiver<Vec<u8>>, thread::JoinHandle<Vec<u8>>) {
    let mut reader = master.try_clone_reader().expect("clone PTY reader");
    let (tx, rx) = mpsc::channel();
    let join = thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    output.extend_from_slice(&buffer[..count]);
                    let _ = tx.send(buffer[..count].to_vec());
                }
            }
        }
        output
    });
    (rx, join)
}

#[test]
fn real_pty_helper_exits_successfully() {
    let (mut child, master) = spawn("success");
    let (rx, reader) = start_reader(&*master);
    await_marker(&rx, b"IRA_HELPER_READY_SUCCESS", Duration::from_secs(5));
    let status = child.wait().expect("wait for helper");
    let output = reader.join().expect("reader thread joins");
    assert!(status.success());
    assert!(output
        .windows(b"IRA_HELPER_READY_SUCCESS".len())
        .any(|window| window == b"IRA_HELPER_READY_SUCCESS"));
}

#[test]
fn real_pty_helper_remains_live_until_deadline_parent_terminates_and_reaps_it() {
    let started = Instant::now();
    let (mut child, master) = spawn("hold");
    let (rx, reader) = start_reader(&*master);
    await_marker(&rx, b"IRA_HELPER_READY_HOLD", Duration::from_secs(5));
    assert!(child.try_wait().expect("check helper status").is_none());
    child.kill().expect("terminate live helper");
    let status = child.wait().expect("reap terminated helper");
    let _ = reader.join().expect("reader thread joins");
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "teardown was unbounded"
    );
    assert!(!status.success());
}

#[test]
fn real_pty_reader_drains_while_helper_exits() {
    let (mut child, master) = spawn("flood");
    let (_, reader) = start_reader(&*master);
    let status = child.wait().expect("wait for output helper");
    let output = reader
        .join()
        .expect("reader drains and joins after child exit");
    assert!(status.success());
    assert!(
        output.len() >= 200 * 1024,
        "drained only {} bytes",
        output.len()
    );
}
