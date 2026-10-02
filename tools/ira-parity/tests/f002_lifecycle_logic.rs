//! S12 logic review: use the same owned session as the live oracle adapter.
use ira_parity::lifecycle::{PtySession, PtySessionConfig};
use portable_pty::{CommandBuilder, PtySize};
use std::time::{Duration, Instant};

#[test]
fn logic_helper_child() {
    if let Ok(marker) = std::env::var("IRA_LOGIC_CHILD_MARKER") {
        use std::io::Write;
        let bytes = vec![b'x'; 1024 * 1024];
        std::io::stdout().write_all(&bytes).unwrap();
        println!("LOGIC_FINAL_OUTPUT_MARKER");
        std::io::stdout().flush().unwrap();
        std::fs::write(marker, b"done").unwrap();
    }
}

fn completed_producer() -> PtySession {
    let fixture = tempfile::tempdir().unwrap();
    let marker = fixture.path().join("producer-done");
    let mut cmd = CommandBuilder::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "logic_helper_child", "--nocapture"]);
    cmd.env("IRA_LOGIC_CHILD_MARKER", &marker);
    let session = PtySession::spawn(
        cmd,
        fixture,
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        },
        PtySessionConfig::new(Duration::from_secs(3), Duration::from_secs(3)),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.exists() && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(
        marker.exists(),
        "producer must have queued its final output before drain"
    );
    session
}

// GAP(G-F002-LOG-07) sev=high kind=behavior-divergence feature=F-002
//   what: the child-exit drain stops at reader-thread completion with queued bytes unconsumed.
//   tui-ref: src/main.rs:71-89 emits persistence and terminal cleanup before exiting.
//   new-ref: tools/ira-parity/src/lifecycle.rs:299-322
//   repro: let a real PTY helper finish a 1 MiB payload before beginning the drain.
//   expected: final marker and all payload bytes are returned before fixture removal.
//   actual: the implementation breaks when reader.is_finished(), leaving queued bytes behind.
//   cover: gap_log_07_child_exit_drain_keeps_all_queued_output
#[test]
fn gap_log_07_child_exit_drain_keeps_all_queued_output() {
    let mut session = completed_producer();
    let output = session
        .wait_for_child_exit_and_drain(Duration::from_secs(5))
        .unwrap();
    let cleanup = session.shutdown();
    cleanup.unwrap();
    assert!(
        output
            .windows(b"LOGIC_FINAL_OUTPUT_MARKER".len())
            .any(|chunk| chunk == b"LOGIC_FINAL_OUTPUT_MARKER"),
        "final bytes were dropped; returned {} bytes",
        output.len()
    );
    assert_eq!(
        output.iter().filter(|&&byte| byte == b'x').count(),
        1024 * 1024
    );
}

// GAP(G-F002-LOG-08) sev=high kind=behavior-divergence feature=F-002
//   what: shutdown joins a finished reader without consuming all queued bytes/errors.
//   tui-ref: migration/specs/F-002.md:S12 requires servicing/draining output and all cleanup errors.
//   new-ref: tools/ira-parity/src/lifecycle.rs:439-476
//   repro: producer queues its complete payload, then shutdown runs without prior reads.
//   expected: shutdown retains every queued output byte in session.output().
//   actual: joining the producer does not drain its mpsc receiver.
//   cover: gap_log_08_shutdown_drains_queued_reader_messages
#[test]
fn gap_log_08_shutdown_drains_queued_reader_messages() {
    let mut session = completed_producer();
    session.shutdown().unwrap();
    assert!(
        session
            .output()
            .windows(b"LOGIC_FINAL_OUTPUT_MARKER".len())
            .any(|chunk| chunk == b"LOGIC_FINAL_OUTPUT_MARKER"),
        "shutdown lost queued bytes; retained {}",
        session.output().len()
    );
}

#[test]
fn failed_spawn_removes_fixture_and_repeated_shutdown_is_safe() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().to_path_buf();
    let cmd = CommandBuilder::new(path.join("missing-executable"));
    assert!(PtySession::spawn(
        cmd,
        fixture,
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0
        },
        PtySessionConfig::new(Duration::from_secs(2), Duration::from_secs(2))
    )
    .is_err());
    assert!(!path.exists());
    let mut session = completed_producer();
    session.shutdown().unwrap();
    session.shutdown().unwrap();
}

// GAP(G-F002-LOG-09) sev=high kind=behavior-divergence feature=F-002
//   what: shutdown terminates/reaps before releasing input writer and slave handles.
//   tui-ref: migration/specs/F-002.md:S12 phases 1 then 2 (D-0014).
//   new-ref: tools/ira-parity/src/lifecycle.rs:379-445
//   repro: child handles HUP by reading the line/EOF sent by UnixMasterWriter::drop.
//   expected: input is already closed when termination is signaled, so handler completes.
//   actual: handler blocks; portable-pty force-kills it after its 200 ms grace period.
//   cover: gap_log_09_stop_input_precedes_child_reap
//   platform: native Unix reproduction; Windows EOF/ConPTY order requires native evidence.
#[cfg(unix)]
#[test]
fn gap_log_09_stop_input_precedes_child_reap() {
    let marker_root = tempfile::tempdir().unwrap();
    let marker = marker_root.path().join("input-closed-before-signal");
    let fixture = tempfile::tempdir().unwrap();
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.args(["-c", "trap 'IFS= read -r line; printf done > \"$1\"; exit' HUP; printf LOGIC_READY; while :; do :; done", "logic-child"]);
    cmd.arg(&marker);
    let mut session = PtySession::spawn(
        cmd,
        fixture,
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        },
        PtySessionConfig::new(Duration::from_secs(2), Duration::from_secs(2)),
    )
    .unwrap();
    session.write_input(b"").unwrap();
    session
        .wait_for_output(b"LOGIC_READY", Duration::from_secs(5))
        .unwrap();
    session.shutdown().unwrap();
    assert!(
        marker.exists(),
        "termination ran before input-close bytes were available"
    );
}

// GAP(G-F002-LOG-10) sev=high kind=test-gap feature=F-002
//   what: a final observation mismatch is discarded if subsequent session shutdown fails.
//   tui-ref: migration/specs/F-002.md:S8/S12 preserve operation plus cleanup errors.
//   new-ref: tools/ira-parity/src/runner.rs:552-574
//   repro: expected_observations_compare returns (false, diagnostics), then shutdown returns Err.
//   expected: RunError includes both mismatch diagnostics and cleanup failure.
//   actual: shutdown's map_err returns first with only cleanup; diagnostics are unreachable.
//   cover: missing runner session-factory/config injection for deterministic cleanup failure
//   confidence: low pending deterministic runtime injection; supported by control-flow reading.

// GAP(G-F002-LOG-11) sev=high kind=behavior-divergence feature=F-002
//   what: cleanup deadline starts after Child::kill, excluding its blocking grace period.
//   tui-ref: migration/specs/F-002.md:S8/S12 absolute bounded cleanup phases.
//   new-ref: tools/ira-parity/src/lifecycle.rs:388-392; portable-pty 0.9.0 lib.rs:341-376
//   repro: Unix child ignores HUP; configure 20 ms reap and 20 ms reader deadlines.
//   expected: complete or fail within the configured bounds (150 ms generous test ceiling).
//   actual: kill sleeps 4*50 ms before the cleanup deadline is even created.
//   cover: gap_log_11_cleanup_deadline_includes_termination
#[cfg(unix)]
#[test]
fn gap_log_11_cleanup_deadline_includes_termination() {
    let fixture = tempfile::tempdir().unwrap();
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.args(["-c", "trap '' HUP; printf LOGIC_READY; while :; do :; done"]);
    let mut session = PtySession::spawn(
        cmd,
        fixture,
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        },
        PtySessionConfig::new(Duration::from_millis(20), Duration::from_millis(20)),
    )
    .unwrap();
    session
        .wait_for_output(b"LOGIC_READY", Duration::from_secs(5))
        .unwrap();
    let start = Instant::now();
    let result = session.shutdown();
    let elapsed = start.elapsed();
    result.unwrap();
    assert!(
        elapsed < Duration::from_millis(150),
        "configured cleanup deadline excluded kill: {elapsed:?}"
    );
}
