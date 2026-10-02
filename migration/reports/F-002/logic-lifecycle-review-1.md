# Logic review F-002: S12 lifecycle, round 1

Commit reviewed: `d945cd9bafa1a2dd2f825c8b6342d3119231a14c`, base `dfd78bf`.
Verdict: **DIVERGENT**, five open LOG findings (all high severity), four reproduced by real PTY tests and one awaiting deterministic runner injection. Confidence: high for LOG-07/08/09/11, low for LOG-10 until injected execution. No production changes.

## Authority and source reading

Read frozen `tui-oracle-baseline` (`1cad4ce43cc72d52d4cc4eef920e0da22cb69568`) `src/main.rs` startup/event loop/exit, `src/event.rs` fully, and `src/tui.rs` fully. These three files have no diff against the tag. Read current manager-worktree `migration/specs/F-002.md` S8/S12 (the review branch's older spec ends at S11), AGENTS section 7, developer report, and full lifecycle/runner diff. Read pinned portable-pty 0.9.0 `lib.rs` child traits/implementations, `unix.rs` reader/writer/master/slave, and Windows `mod.rs`, `conpty.rs`, `psuedocon.rs` lifecycle code from the Cargo registry.

The oracle probes before starting its event thread, initializes raw/alternate-screen/mouse/paste handling, draws, then redraws and consumes events. Only key presses are accepted; repeat/release are ignored. Normal quit persists state, cleans graphics, restores the terminal, and returns. Its output and persistence may occur immediately before exit. S12 describes harness resource management, so the direct equivalence authority for cleanup order is the approved S12 contract, with the frozen program supplying real observed output. Killing a still-interactive oracle after a completed observation is different from invoking normal quit; persistence tests must explicitly deliver the oracle's quit input before asserting exit effects.

## State and outcome equivalence table

`L` means tools/ira-parity/src/lifecycle.rs; `R` means src/runner.rs in that crate. References name the reviewed commit, before this review's tests.

| # | Oracle / contract | Condition and ordered effect | New source | Result / evidence |
|---|---|---|---|---|
| 1 | S12 startup | PTY allocation or reader clone fails before child exists: release fixture/handles | L147-149 | RAII matches by reading; clone/open error injection absent. |
| 2 | S12 startup | Child spawn fails: no running child; remove fixture | L149 | Passed `failed_spawn_removes_fixture_and_repeated_shutdown_is_safe` using nonexistent executable. |
| 3 | S12 startup | Child exists, then reader-thread creation fails | L149-153 | `thread::spawn` can panic before session ownership is constructed; bounded unwind cleanup not demonstrated. Existing broad lifecycle GAPs remain; requires fallible spawn injection coverage. |
| 4 | S12 phase 1 | Stop input and drop writer/slave before child termination | L378-445 | NO, LOG-09: first kill/reap, then all PTY handles close. Real Unix HUP handler never receives pending input-close bytes before forced death. |
| 5 | S12 phase 2 | Child already exited: observe and reap | L379-383 | `try_wait` reaps; normal exit session test passes. |
| 6 | S8/S12 deadlines | Child still alive: termination included in absolute cleanup bound | L385-414 | NO, LOG-11: Unix portable-pty `Child::kill` performs four 50 ms sleeps before the configured deadline is created. 20+20 ms configuration took 210.980603 ms. |
| 7 | S12 phase 2 | Poll/kill failures: preserve reason, retain fixture unless cleanup proven | L407-434,486-509 | Structurally guarded; native API failure injection absent. Windows WinChild::kill itself discards TerminateProcess errors; polling must prove exit. |
| 8 | main.rs71-89; S12 output | Child exits while reader remains active: preserve exact final output | L294-323 | NO, LOG-07: accepting a message followed by is_finished does not empty the queued channel; final marker missing after 1 MiB producer. |
| 9 | S12 phase 3 | Continue servicing output while PTY closes | L443-458 | Background producer is active but channel consumption may stop early; LOG-08. Unix cloned reader remains independent of the owned master FD. Closing master alone is not a cancellation guarantee if a descendant retains slave. |
| 10 | S12 phase 4 | Reader finished: join AND drain all queued messages/errors | L448-483 | NO, LOG-08: join success does not imply receiver drained; shutdown retained only 69,394 bytes in recorded 1 MiB run. |
| 11 | S12 phase 4 | Reader timeout/panic: error, retain fixture and owned handles | L460-509,529-559 | Guards match by reading. Existing tests cover injected reader error, not stalled descendant or panic. Native Windows/macOS evidence outstanding. |
| 12 | S12 reader failure | Failure previously consumed by next_output/wait_for_output preserved with primary | L218-237,329-346,482-483 | Existing reader-injection session test passes. Failure queued behind bytes can be skipped by row 10; drain path also returns Ok after accept(Failed) unless later shutdown consumes recorded error. |
| 13 | S8 readiness | Readiness timeout or PTY read error before input => shutdown with primary | R457-488, finish_failed_session | All paths route to bounded session cleanup; zero input delivered. Native tagged Linux replay passed; pending cleanup issues apply. |
| 14 | S8 event application | Deadline/encode/write/resize error after startup => shutdown with primary | R492-514 | Error return routing matches by reading. Blocking write_all itself has no scenario deadline; explicit write/resize failure injection missing. Unsupported encodings preflight before startup. |
| 15 | S8 observation | Observation API/read failure => preserve primary plus cleanup | R526-559 | session_try routes returned errors through finish_failed_session. Filesystem reads themselves have no enforced deadline. |
| 16 | S8/S12 mismatch | Final comparison is false AND cleanup fails => both diagnostics retained | R552-574 | NO by reading; LOG-10. shutdown map_err returns before mismatch diagnostics. Runner lacks a session/config injection seam for deterministic executable proof. |
| 17 | S12 composition | Explicit primary plus known cleanup error | L512-524 | Existing injected fixture-removal and reader-error tests pass. No change to previously resolved LOG markers. |
| 18 | S12 phase 5 | Only remove fixture after child reap, reader join, and handle closure | L486-509 | Correct guard, existing removal-order/retention tests pass. Claimed all-output drain is nevertheless false per rows 8/10. |
| 19 | S12 removal | Removal failure: return error and retain fixture for diagnosis | L494-504 | Injected failure passes; actual removal failure remains owned and Drop retention prevents automatic removal. |
| 20 | S12 repeated shutdown | Repeat after successful cleanup has no live work | L378,476,499 | Passing new repeated-shutdown test; absent fixture can still produce a removal-attempt event, but no actual removal runs. |
| 21 | S12 Drop | Unattempted session => try shutdown; failed attempt => retain ownership without detaching JoinHandle | L529-559 | Reading confirms global retention rather than dropping live handles; process-lifetime resource retention requires treating error as fatal. Successful explicit shutdown makes Drop inert. |
| 22 | S12 platform | ConPTY final close/read/descendant ordering proven natively | portable-pty win/conpty.rs; L355-359 | UNVERIFIED on Windows/macOS. Shared Inner owns ClosePseudoConsole and pipe; synchronous drop is outside reader deadline. No native-equivalence claim. |

Rows marked structurally matching are code-reading observations, not standalone verification of every fault path. The outstanding lifecycle reviewer findings and F-150 native evidence remain required. LOG-07/08 concern final messages, regardless of whether early screen assertions happened to match. The original flood test's threshold of 200 KiB for a 256 KiB producer permits truncation and does not establish byte-exact drain.

## Findings and executable evidence

New notes reside in `tools/ira-parity/tests/f002_lifecycle_logic.rs`:

- **LOG-07 high:** `gap_log_07_child_exit_drain_keeps_all_queued_output`; missing final marker, returned 999,442 bytes for a 1,048,576-byte payload plus marker/harness output.
- **LOG-08 high:** `gap_log_08_shutdown_drains_queued_reader_messages`; shutdown reported success with only 69,394 output bytes retained and no final marker.
- **LOG-09 high:** `gap_log_09_stop_input_precedes_child_reap`; Unix child HUP handler requires writer-drop line/EOF bytes, but is force-killed before they arrive.
- **LOG-10 high test gap / low confidence pending injection:** final mismatch diagnostics lost when shutdown fails; missing runner session-factory/config fault injection prevents the combined execution test. Exact control-flow reproduction is in the GAP note.
- **LOG-11 high:** `gap_log_11_cleanup_deadline_includes_termination`; 20 ms child + 20 ms reader budget exceeds the generous 150 ms test ceiling because portable-pty kill runs its 200 ms grace first.

Tests do not sleep to synchronize. Producers publish a completion file only after flushing output, parent polls with a deadline, and all child resources are shut down before assertions fail. Unix-specific signal behavior tests do not claim Windows coverage.

Environment for all commands:
`TMPDIR=/home/vlad/.cache/ira-parity-f002-fix-tmp CARGO_TARGET_DIR=/home/vlad/.cache/ira-parity-f002-fix-target`

- `cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_lifecycle_logic -- --test-threads=1`: 2 pass / 3 fail before adding LOG-11 (LOG-07/08/09). A prior run independently reproduced LOG-07/08.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_lifecycle_logic gap_log_11 -- --test-threads=1`: 1 fail, elapsed 210.980603 ms.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_lifecycle_session --test f002_lifecycle_child -- --test-threads=1`: 6 + 4 pass.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_contract tagged_oracle_trace_replays_on_linux_macos_windows -- --exact --test-threads=1`: 1 pass on Linux against the frozen oracle (6.40 s).
- Final combined `f002_lifecycle_logic` run: 2 pass / 4 fail (LOG-07/08/09/11), zero ignored; LOG-07 returned 778,256 bytes, LOG-08 retained 73,745 bytes, LOG-11 elapsed 211.123838 ms.
- Reviewer-file rustfmt, `git diff --check`, and `python3 scripts/scope_check.py reviewer --range d945cd9..HEAD`: pass (2 files).

No LOG fixes were claimed by the developer in this slice. LOG-01..04 remain resolved, LOG-05/06 remain open outside this slice. No ADV markers changed or closed.
