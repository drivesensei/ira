# Adversarial report: F-002 real PTY lifecycle red-test preparation

Mode: A (red-test preparation)
Commit reviewed: `5ae8bf38346e7ff20f636f9928eebcf29bc97ab6`
Branch: `review/F-002-lifecycle-red`

## Attempts and findings

- Read F-002 S7/S8/S12, D-0013, and architecture redesign R1/R2. Inspected `run_oracle_with_baseline` and `PtyChildLifecycle` in `tools/ira-parity/src/runner.rs`, plus ADV-27/29/30 in `tests/f002_contract.rs`.
- Confirmed `run_oracle_with_baseline` currently creates the fixture, PTY pair, child, and reader inline. Its private lifecycle struct owns the child and reader only; master/slave handles and the fixture are outside it. The existing ADV-29/30 tests call `ScriptedTarget`, so their successful boolean assertions are not evidence about an OS child, PTY handles, reader join, or fixture-removal ordering.
- Added a test-harness contract at `tools/ira-parity/tests/f002_lifecycle_harness_contract.md`, mapped to the existing open G-F002-ADV-27/29/30 notes. Clarified in the existing test source that these scripted probes remain orchestration-only; no GAP was fixed or resolved.
- A direct real-helper-child integration test cannot honestly be compiled against this base: the extracted PTY session API and helper-child fixture do not exist. I did not invent a production API or add a fabricated mock. The report specifies the behaviors the new shared session tests must assert once the extraction lands: success; live-child timeout; exit while reader active; reader/PTY failure; bounded cleanup; process wait/reap; reader drain/join; handles closed before fixture removal; cleanup order; and combined primary/cleanup errors.
- Tried the existing `tagged_oracle_trace_replays_on_linux_macos_windows` test as a live-oracle smoke. It failed before starting the oracle because the temp baseline worktree could not be created: `Disk quota exceeded`; cleanup then reported the incomplete temp path was not a worktree. This is an environment failure, not a behavior result.

## Commands and results

- `cargo test --manifest-path tools/ira-parity/Cargo.toml --test f002_contract --no-run` — passed; test binary compiled.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --test f002_contract deadline_terminates_reaps_child_and_closes_pty -- --ignored --exact` — passed (1), scripted orchestration only.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --test f002_contract timeout_drains_and_joins_readers_before_fixture_removal -- --ignored --exact` — passed (1), scripted orchestration only.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --test f002_contract tagged_oracle_trace_replays_on_linux_macos_windows -- --exact` — failed before oracle launch on temp-worktree disk quota.

## Coverage and status

No real-child lifecycle test is currently runnable because the production session seam and helper fixture are absent. The harness contract lays out all S12 cases and the testable evidence required; G-F002-ADV-27, G-F002-ADV-29, and G-F002-ADV-30 remain open. The Linux live-oracle attempt is blocked by disk quota; macOS and Windows MSVC runs were not available in this environment. No mutation checks were applicable because no production implementation was changed.

Verdict: FINDINGS — lifecycle evidence is not yet possible against this implementation base.
