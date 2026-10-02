# Adversarial report: F-002 (mode B) round 1

Commit reviewed: `88dcf9c38185c3a10ee1938050c9dd095e456a2b`

Attempts made:
- Read the complete production source, contract tests, F-002 spec, reviewer protocol, and manager charter §7.
- Ran `TMPDIR=/dev/shm CARGO_TARGET_DIR=/dev/shm/ira-parity-review-target cargo test --manifest-path tools/ira-parity/Cargo.toml -- --test-threads=1`: 27 passed, 13 ignored. This included live Linux PTY replays of tagged-baseline startup/input/resize traces.
- Ran the same suite with `-- --ignored --test-threads=1`: 13 passed. This included exact tag/SHA, detached checkout, lockfile build, digest/metadata cache validation, wrong tag, and error cleanup.
- Replayed `all_event_types.toml` against the live frozen TUI on Linux; ordering/readiness/observations passed.
- Added a real baseline success/cache-hit/drop probe. It checks cached executable survives checkout removal and both successful and cache-hit worktrees are removed.
- Confirmed Windows-only coverage is cfg-gated and was not run here; no macOS/Windows native behavior is claimed.
- Attempted an external protected-root side-effect probe; it passed (override rejected before directory creation), so no issue filed.

Findings (open GAPs in `tools/ira-parity/tests/f002_adversarial.rs` and `f002_contract.rs`):
- `G-F002-ADV-44`, high, `expected_observation_content_is_compared`: `ScenarioRunner::run_with_target` ignores `ExpectedObservation.expect` and reports a match for mismatched screen contents.
- `G-F002-ADV-45`, medium, `incompatible_trace_platform_is_rejected_before_start`: declared trace profiles are not checked against `RunOptions.platform`; a macOS-only trace runs under Linux.
- `G-F002-ADV-46`, high, `readiness_observation_kind_is_enforced`: runner marks ready and proceeds without enforcing the declared readiness observation kind; a filesystem readiness assertion can be bypassed by terminal readiness.
- `G-F002-ADV-49`, medium, `normalizer_discards_osc_title_sequences`: OSC title control bytes/payload leak into normalized screen text (`\x1b]0;host-title\x07...` becomes `ost-title\x07...`).
- `G-F002-ADV-38`, medium, `golden_metadata_requires_oracle_sha_scenario_fixture_dimensions_events_os_and_date`: staged `metadata.toml` serializes `os_profile = "unknown"` and `capture_date = "unknown"` instead of the required platform/date. The revised test reads and checks actual serialized values.

Verified fixes / passing probes:
- All 13 ignored baseline/cache/cleanup tests passed; extra real success + cache-hit cleanup probe passed.
- Existing supported-event live replay and unsupported repeat/release rejection passed on Linux.
- Existing normalization, byte comparison, mismatch diagnostic, staging, ordinary-run, environment isolation, malformed trace, and tagged resize replay tests passed.
- `G-F002-ADV-31` was reopened in the contract test: it now declares filesystem readiness with pending input and fails because the runner accepts terminal readiness without observing the declared kind. The prior readiness/deadline assertions called `ScriptedTarget` methods returning hardcoded `true`.
- `G-F002-ADV-38` was reopened in the contract test: it now reads staged `metadata.toml` and verifies fields/values; this exposes current `unknown` OS/date serialization.
- `G-F002-ADV-27` stays open for unproven timeout/test-failure exit modes. The real baseline probe verifies successful and cache-hit cleanup, while previous “timeout”/“test-failure” labels exercise the same injected early-error branch.

Mutation checks:
- Bypassed executable digest comparison locally; `cache_hit_verifies_metadata_and_executable_digest` failed as intended. Restored source.
- Disabled volatile-root replacement locally; normalization test failed as intended. Restored source.
- Changed returned readiness flag locally; readiness contract test failed as intended. Restored source.
- `git diff` confirms no production-source changes.

Reopened: `G-F002-ADV-31`, `G-F002-ADV-38`; `G-F002-ADV-27` stays open for unexercised failure modes.

Verdict: FINDINGS (5 open GAPs, including 31 and 38; 27 remains open for untested cleanup modes).

Enrichment candidates: none.
