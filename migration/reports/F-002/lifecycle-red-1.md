# Adversarial report: F-002 real PTY lifecycle red-test preparation

Mode: A (red-test preparation)
Commit reviewed: `5ae8bf38346e7ff20f636f9928eebcf29bc97ab6`
Branch: `review/F-002-lifecycle-red`

## Attempts and findings

- Read F-002 S7/S8/S12, D-0013, and architecture redesign R1/R2. Inspected `run_oracle_with_baseline` and `PtyChildLifecycle` in `tools/ira-parity/src/runner.rs`, plus ADV-27/29/30 in `tests/f002_contract.rs`.
- Confirmed `run_oracle_with_baseline` currently creates the fixture, PTY pair, child, and reader inline. Its private lifecycle struct owns the child and reader only; master/slave handles and the fixture are outside it. The existing ADV-29/30 tests call `ScriptedTarget`, so their successful boolean assertions are not evidence about an OS child, PTY handles, reader join, or fixture-removal ordering.
- Added a test-harness contract at `tools/ira-parity/tests/f002_lifecycle_harness_contract.md`, mapped to the existing open G-F002-ADV-27/29/30 notes. Clarified in the existing test source that these scripted probes remain orchestration-only; no GAP was fixed or resolved.
- Added `tools/ira-parity/tests/f002_lifecycle_child.rs`. It launches this integration-test executable as a helper child through a real `portable-pty` and covers successful exit, a live child terminated/reaped by the parent within a bounded interval, and output draining while the helper exits. These exercise the helper protocol and OS PTY, not the current production lifecycle code.
- Added `tools/ira-parity/tests/f002_lifecycle_session.rs` with an explicit minimal `PtySession` API contract. It covers session success, deadline cleanup, child exit while output is read, injected reader failure, cleanup order, fixture removal, and combined primary/cleanup errors. This target currently fails to compile because `ira_parity::lifecycle` does not exist. No production API was added by the reviewer.
- Tried the existing `tagged_oracle_trace_replays_on_linux_macos_windows` test as a live-oracle smoke. It failed before starting the oracle because the temp baseline worktree could not be created: `Disk quota exceeded`; cleanup then reported the incomplete temp path was not a worktree. This is an environment failure, not a behavior result.

## Commands and results

- `cargo test --manifest-path tools/ira-parity/Cargo.toml --test f002_contract --no-run` — passed; test binary compiled.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --test f002_contract deadline_terminates_reaps_child_and_closes_pty -- --ignored --exact` — passed (1), scripted orchestration only.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --test f002_contract timeout_drains_and_joins_readers_before_fixture_removal -- --ignored --exact` — passed (1), scripted orchestration only.
- `cargo fmt --manifest-path tools/ira-parity/Cargo.toml -- --check` — initially reported formatting differences in the new tests; `cargo fmt --manifest-path tools/ira-parity/Cargo.toml` applied formatting.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --test f002_lifecycle_child -- --test-threads=1` — passed (4): helper entry, real PTY success, live child terminate/reap, and reader draining during helper exit.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --test f002_lifecycle_session --no-run` — expected red: `error[E0432]: unresolved import ira_parity::lifecycle` at `tests/f002_lifecycle_session.rs:6`.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --test f002_contract tagged_oracle_trace_replays_on_linux_macos_windows -- --exact` — failed before oracle launch on temp-worktree disk quota.

## Coverage and status

Real helper-child PTY tests are runnable and pass for success, termination/reap, and active-reader exit. The extracted session's cleanup behavior remains untested: its contract intentionally fails to compile on the absent module, and reader failure injection, handle/fixture ordering, and composed errors await the implementation. G-F002-ADV-27, G-F002-ADV-29, and G-F002-ADV-30 remain open. The Linux live-oracle attempt is blocked by disk quota; macOS and Windows MSVC runs were not available in this environment. No mutation checks were applicable because no production implementation was changed.

Verdict: FINDINGS — lifecycle evidence is not yet possible against this implementation base.
