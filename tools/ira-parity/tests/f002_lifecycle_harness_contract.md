# F-002 real PTY lifecycle red-test harness contract

This contract is intentionally documentation-only until the implementation
extracts a PTY session API. At `5ae8bf3`, `run_oracle_with_baseline` creates the
fixture, PTY pair, child, and reader inline; `PtyChildLifecycle` is private and
owns only the child and reader. A test built on `ScriptedTarget` cannot reach
these OS resources. Guessing an API here would either test a fabricated seam
or keep testing a mock, so the open GAPs remain open.

When the session extraction lands, replace the lifecycle-related mock evidence
with integration tests that instantiate the same session type used by the TUI
adapter. The helper child must be launched through the session's real PTY and
must have deterministic modes selected by arguments/environment (no sleeps for
synchronization). The session should expose observable results or test hooks
for child status, reader completion, handle closure, fixture removal, and
ordered cleanup events; tests must not infer success from the helper's own
claims.

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

The existing tests `deadline_terminates_reaps_child_and_closes_pty` and
`timeout_drains_and_joins_readers_before_fixture_removal` compile against
`ScriptedTarget` and remain useful only for orchestration behavior. They were
not relabeled as real-child evidence. The helper-child cases above are blocked
on the extracted production session API and helper fixture infrastructure;
there is no honest red runtime result against that absent API yet.
