# Adversarial report: F-002 (mode B), S12 lifecycle round 1

Commit reviewed: `d945cd9bafa1a2dd2f825c8b6342d3119231a14c`, base `dfd78bf`.
Scope: extracted PTY lifecycle and its live-oracle integration. Production read only except three reverted local mutation experiments. Reviewed S8 and the manager branch's approved S12 (the feature checkout's spec does not yet contain S12). No S13/S14 marker changes.

## Verdict

**FINDINGS**: two new open findings (one high, one medium); ADV-29 and ADV-30 reopened. S12 is not ready for verification. Native macOS/Windows evidence remains absent and ADV-40 remains open.

## Findings

- **G-F002-ADV-59, high**: both `shutdown_inner` and `wait_for_child_exit_and_drain` stop receiving when `JoinHandle::is_finished()` is true, although the unbounded channel can still contain output or failure messages. Joining a reader does not empty that queue. A helper flushes exactly 1,048,576 `Q` bytes and signals production through a fixture file. `shutdown_drains_all_queued_output_before_success` retained only 69,632 bytes in one run; `child_exit_drain_consumes_entire_queue` retained 483,328. Different scheduling changes the truncated count. `shutdown_preserves_queued_reader_failure` demonstrated loss of the injected failure: the returned message was only `operation failed`, without cleanup error. Expected: drain every queued message through closure, preserve every reader failure, then report successful cleanup. Reopens ADV-30.
- **G-F002-ADV-60, medium, Unix**: the cleanup clock starts after `Child::kill`. Pinned portable-pty 0.9.0 implements this call with SIGHUP followed by four 50ms sleeps before force-kill when the child ignores SIGHUP. `termination_is_included_in_absolute_cleanup_deadline` configured 20ms child and 20ms reader deadlines but observed 210.967ms (test permits 150ms, including 110ms scheduling tolerance). Expected: termination is included in the absolute deadline, with bounded polling and an explicit retained-resource failure when unavailable. Reopens ADV-29. This test is Unix-specific because the defect is in the pinned Unix kill implementation; native Windows termination remains ADV-40's obligation.

Executable regressions and GAP notes: `tools/ira-parity/tests/f002_lifecycle_adversarial.rs`. No failing probes are ignored.

## Attempts made (including passes)

Environment: Linux, Rust, portable-pty 0.9.0. Commands used `TMPDIR=/home/vlad/.cache/ira-parity-f002-fix-tmp` and `CARGO_TARGET_DIR=/home/vlad/.cache/ira-parity-f002-fix-target`.

- Read the complete lifecycle/runner diff and surrounding call paths; read frozen TUI `main.rs`/`event.rs` and pinned dependency kill/wait implementations.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_lifecycle_session --test f002_lifecycle_child --test f002_contract --test f002_adversarial -- --test-threads=1`: **45 passed, 11 existing GAP ignores** (6 session + 4 child + 29 contract + 6 adversarial). The contract suite drove the real frozen Linux oracle via `tagged_oracle_trace_replays_on_linux_macos_windows`; expected resize trace observation matched and baseline SHA was checked. This is Linux proof only, not native macOS/Windows proof.
- Existing success, live-child kill/reap, reader-active child exit, injected read failure after observation, and primary/fixture-removal error composition probes passed.
- New suite: **5 passed, 4 failed**. Passes: duplicate shutdown; write/resize after closed master give explicit errors; nonexistent executable during partial spawn removes fixture; repeated injected cleanup error and Drop preserve fixture; a live descendant retaining the PTY makes reader cleanup expire, preserves primary error and fixture, and Drop retains fixture. The descendant is explicitly released by a fixture file and finishes before test cleanup. Fixture-file conditions and yielding are used instead of sleep synchronization.
- The descendant test exercises genuine reader timeout/retention on Unix. Source confirms Drop transfers unresolved child/reader/fixture ownership to the static retention list rather than dropping a live JoinHandle. The test does not prove native ConPTY closure or process-lifetime eventual recovery.
- `cargo test ... --tests --no-run`: expected pre-existing S13 compile failure for missing `RunCapture` and observation bundle APIs. Consequently no claim that the entire F-002 feature suite is green.
- Setup failure before child spawn is exercised; thread creation resource exhaustion and OS-level child kill/poll errors were not fault-injected. Native Windows/macOS inaccessible here.

## Mutation checks

All mutations were temporary in `src/lifecycle.rs`, run individually, then restored byte-for-byte:

1. Replace child kill with `Ok(())`: `extracted_session_timeout_terminates_live_child_within_bound` failed on child and reader cleanup deadlines (5s), preserving fixture. **Detected**.
2. Suppress appending the observed reader failure to cleanup errors: `extracted_session_reports_reader_failure_and_preserves_primary_error` failed its cleanup-error assertion. **Detected**.
3. Bypass injected fixture-removal failure: `extracted_session_records_teardown_order_and_composes_cleanup_error` failed its fixture-error assertion. **Detected**.

No surviving mutation, no production diff. The stronger queue tests expose a missing condition despite the existing observed-reader-error mutation being detected.

## Platform and remaining limitations

Pinned ConPTY uses shared ownership and duplicated pipe handles; this implementation closes handles synchronously after child reap. No native evidence establishes that closure remains bounded or drains final output with attached descendants. Preserve ADV-40 and run native Windows/macOS suites after fixes. Also, source inspection shows synchronous `write_all` in input delivery; no blocked-input transport fault was injected in this scoped review, so bounded input delivery is not claimed proven.

Enrichment candidates: none.
