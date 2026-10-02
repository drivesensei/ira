# F-002 lifecycle implementation report

STATUS: DONE for the assigned S12 lifecycle follow-up; reviewer verification remains pending.
ROW(S): F-002 S12; ADV-29/30, ADV-59/60, LOG-07..11 marked GAP-FIXED.
CHANGED: `tools/ira-parity` PTY session teardown, real runner error composition, lifecycle tests.
COMMITS: follow-up commit recorded in Git history; prior lifecycle implementation `d945cd9`.
TESTS: all applicable lifecycle and F-002 Linux suites listed below pass.
GAPS: S13/S14 markers remain open and unchanged; ADV-40 remains open for native macOS/Windows evidence.
FINDINGS: pinned Unix `Child::kill()` blocks for a 200ms grace. The adapter instead uses `clone_killer()` for immediate HUP, polls the owned child, escalates to SIGKILL halfway through one shutdown-entry child deadline, and keeps polling/output drain within that deadline. Writer/slave are dropped before child poll/signal. Reader messages are drained through `Closed` even after its thread signals completion. The runner mismatch test confirms the primary mismatch and fixture-cleanup failure are both reported.
RISKS: Native Linux execution only. macOS and Windows clone-killer, PTY reader unblock, and handle-close ordering remain unverified. A failed reap/join retains owned process/thread/fixture state; no live reader JoinHandle is detached.
NEXT ACTION: adversarial and logic reviewers verify fixed markers; manager schedules native macOS/Windows runs before closing platform-specific evidence.

## Evidence

- Prior red baseline: `f002_lifecycle_session --no-run` failed because the extracted lifecycle API was absent; `migration/reports/F-002/lifecycle-red-1.md` records the red result.
- Reviewer red results before this follow-up: `adv-lifecycle-review-1.md` records 5 passed / 4 failed in the lifecycle adversarial suite; `logic-lifecycle-review-1.md` records the child-drain/input-order failures and a 210ms bounded-kill probe against a 150ms ceiling. Those exact test names now pass below.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_lifecycle_logic -- --test-threads=1`: 7 passed, including complete 1 MiB drain, input-close ordering, baseline-backed mismatch plus cleanup injection, and 20ms + 20ms bounded-kill probe (<150ms).
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_lifecycle_adversarial -- --test-threads=1`: 9 passed, including queued output/failure drain, reader timeout ownership, repeated cleanup, fixture retention, and bounded termination.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_lifecycle_session -- --test-threads=1`: 6 passed.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_lifecycle_child -- --test-threads=1`: 4 passed.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_contract -- --test-threads=1`: 29 passed, 11 pre-existing ignored with GAP reasons.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_adversarial -- --test-threads=1`: 6 passed.
- `cargo fmt --manifest-path tools/ira-parity/Cargo.toml -- --check`: passed.
- `cargo clippy --manifest-path tools/ira-parity/Cargo.toml --locked --lib -- -D warnings`: passed.
- Scoped clippy with `-D warnings` for `f002_lifecycle_logic`, `f002_lifecycle_adversarial`, `f002_lifecycle_session`, `f002_lifecycle_child`, and `f002_contract`: passed.
- `python3 scripts/scope_check.py custom --allow 'tools/ira-parity/**' --allow 'migration/reports/F-002/dev-lifecycle-1.md'`: passed before report refresh; rerun on final diff.
- Intentional later-slice limitations: full `--all-targets` remains unavailable while the S13 capture-bundle contract is compile-red; S14 paste framing is not part of this task and remains open.
