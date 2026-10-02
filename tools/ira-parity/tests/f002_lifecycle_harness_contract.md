# F-002 real PTY lifecycle red-test harness contract

`f002_lifecycle_child.rs` is a real helper-child fixture launched through
`portable-pty`; `f002_lifecycle_session.rs` is the compile-time red contract for
the session API that must replace the inline lifecycle. At `5ae8bf3`,
`run_oracle_with_baseline` creates the fixture, PTY pair, child, and reader
inline; `PtyChildLifecycle` is private and owns only the child and reader. The
session contract currently fails at `use ira_parity::lifecycle` because that
module does not yet exist.

The API shape proposed by the executable contract is:

```rust
PtySession::spawn(CommandBuilder, TempDir, PtySize, PtySessionConfig)
session.wait_for_output(marker, deadline)
session.wait_for_child_exit_and_drain(deadline)
session.shutdown() -> ShutdownReport
session.shutdown_with_primary_error(error) -> CombinedError
```

`ShutdownReport` exposes whether the OS child was reaped, the reader joined,
and PTY handles closed before fixture removal. `CombinedError` retains primary
and cleanup errors plus ordered `CleanupEvent`s. Deterministic failure injectors
in `PtySessionConfig` let tests cause a reader error and fixture-removal error.
This is a minimal reviewer-proposed test seam; the developer can adapt names,
but the tests must continue to instantiate the same session implementation as
the TUI adapter. The helper is the `helper_child_entry` case in
`f002_lifecycle_child.rs`, selected by environment mode (no sleeps).

Required cases mapped to the existing open notes:

| Existing GAP | Required test assertion |
|---|---|
| G-F002-ADV-27 | Baseline checkout/fixture cleanup runs on real session success, cache hit, startup/read failure, scenario timeout, and assertion failure; primary plus cleanup diagnostics both survive. |
| G-F002-ADV-29 | A live helper is terminated on deadline, its OS process is waited/reaped, and all cleanup completes within the configured bounded deadline. |
| G-F002-ADV-30 | Helper exits while output reader is active; reader/PTY failure is surfaced; output is drained and reader joined; master/slave/child handles close before fixture removal. Assert event order and prove the fixture can be removed on Windows after ConPTY handles close. |

Run the real-child cases on Linux, macOS, and Windows MSVC. Keep platform
specific assertions where required, but use one cross-platform helper protocol
and no platform-only shell command. Verify wall-clock upper bounds with generous
deadlines and synchronization channels/pipes rather than `sleep`.

## Current status

The direct helper tests prove the OS child/PTY fixture works, but they do not
prove the future session cleanup path. The session contract is an expected
compile failure until the extracted production API exists. Reader failure,
cleanup ordering, fixture removal order, and error composition are asserted in
that contract and cannot run yet. The existing tests
`deadline_terminates_reaps_child_and_closes_pty` and
`timeout_drains_and_joins_readers_before_fixture_removal` still exercise only
`ScriptedTarget`; they do not close any GAP.
