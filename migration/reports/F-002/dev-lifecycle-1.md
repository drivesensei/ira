# F-002 lifecycle implementation report

## Status and scope

STATUS: DONE for the serialized S12 lifecycle slice; not a claim that F-002 is complete.
ROW(S): F-002
CHANGED: `tools/ira-parity/src/lifecycle.rs`, `src/lib.rs`, `src/runner.rs`, lifecycle and contract tests, and runner-include test wrappers.
COMMITS: recorded in the commit history for this report.

The adapter now uses `PtySession` to own the fixture, PTY master/slave, writer, child, and reader thread. The same session creates and drives the real TUI child in `run_oracle_with_baseline`; the former inline `PtyChildLifecycle` was removed. Errors after session startup go through bounded shutdown and retain the primary operation error alongside cleanup failures.

Teardown drops writer/slave ownership, polls the child, terminates a still-live child, polls/reaps it under a deadline while continuing to receive output, closes the PTY endpoints, joins the reader under its own deadline, then removes the fixture. If child or reader cleanup fails, the session retains those handles and fixture; if the owning session is dropped after a failed cleanup attempt, remaining resources are held in a process-lifetime retention list rather than dropping a live `JoinHandle`. An injected fixture-removal failure relinquishes the tempdir without deleting the directory; its test removes the preserved path after asserting retention.

## Evidence

- Red baseline: `f002_lifecycle_session --no-run` failed because `ira_parity::lifecycle` did not exist; recorded in `migration/reports/F-002/lifecycle-red-1.md`.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_lifecycle_session -- --test-threads=1`: 6 passed. Covers real child success, bounded termination, output drain while child exits, injected reader failure, primary/cleanup composition, handle ordering, and fixture retention/removal.
- `cargo test ... --test f002_lifecycle_child -- --test-threads=1`: 4 passed against a real helper child and PTY.
- `cargo test ... --test f002_contract -- --test-threads=1`: 29 passed, 11 ignored with existing GAP reasons. Includes both lifecycle orchestration probes and the live Linux baseline replay.
- `cargo test ... --test f002_adversarial -- --test-threads=1`: 6 passed.
- `cargo test ... --test f002_contract tagged_oracle_trace_replays_on_linux_macos_windows -- --exact --test-threads=1`: passed on Linux against the frozen oracle tag.
- `cargo fmt --manifest-path tools/ira-parity/Cargo.toml -- --check`: passed.
- `cargo clippy --manifest-path tools/ira-parity/Cargo.toml --locked --lib -- -D warnings`: passed.
- `cargo clippy --all-targets` / full integration-test compilation remains blocked by the intentionally red S13 `f002_capture_bundle_contract` compile contract (missing `RunCapture` and observation-bundle APIs). S14 `f002_paste_protocol` remains red as expected: actual bytes are plain `pasted`, expected bytes are bracketed-paste framed.

## GAPs, findings, and risks

G-F002-ADV-29 and G-F002-ADV-30 are marked `GAP-FIXED` pending adversarial verification; their original scripted probes remain supplemental, and the new helper-child tests exercise the extracted session. No S13/S14 behavior was implemented. G-F002-ADV-27 concerns baseline worktree cleanup, not this PTY lifecycle slice, and remains open.

The pinned Unix implementation maps PTY `EIO` to EOF; Linux real-child tests confirm closing the owned master lets the cloned reader finish and join. macOS was not available for native execution. Windows ConPTY uses a shared `Arc<Mutex<Inner>>` and cloned pipe handles; its close/output ordering is not claimed verified without native Windows execution. No native macOS/Windows lifecycle result is claimed.

RISKS: `portable-pty::Child::wait` is intentionally not used because it can block indefinitely; kill followed by `try_wait` polling is bounded. A cleanup deadline that expires retains the session resources, so the caller should treat that error as fatal cleanup failure and preserve the owning session while possible.

NEXT ACTION: adversarial and logic reviewers verify GAP-FIXED lifecycle markers and platform review schedules native macOS/Windows confirmation; manager then assigns S13 separately.
