# Adversarial report: F-002 (mode B) round 1

Initial commit reviewed: `88dcf9c38185c3a10ee1938050c9dd095e456a2b`
Remediation reviewed: `62db2ceba7e7d67b7db60cf280954853d297f9b3`

Attempts made:
- Read the complete production source, contract tests, F-002 spec, reviewer protocol, and manager charter §7.
- Ran `TMPDIR=/dev/shm CARGO_TARGET_DIR=/dev/shm/ira-parity-review-target cargo test --manifest-path tools/ira-parity/Cargo.toml -- --test-threads=1`: 27 passed, 13 ignored. This included live Linux PTY replays of tagged-baseline startup/input/resize traces.
- Ran the same suite with `-- --ignored --test-threads=1`: 13 passed. This included exact tag/SHA, detached checkout, lockfile build, digest/metadata cache validation, wrong tag, and error cleanup.
- Replayed `all_event_types.toml` against the live frozen TUI on Linux; ordering/readiness/observations passed.
- Added a real baseline success/cache-hit/drop probe. It checks cached executable survives checkout removal and both successful and cache-hit worktrees are removed.
- Confirmed Windows-only coverage is cfg-gated and was not run here; no macOS/Windows native behavior is claimed.
- Attempted an external protected-root side-effect probe; it passed (override rejected before directory creation), so no issue filed.
- Against remediation `62db2ce`, reran ADV-44/45/46/49: all four pass. Ran the revised G31 and G38 contract tests individually: both pass with meaningful observed bytes and serialized metadata assertions.
- Re-ran G27/G29/G30 individually with `--ignored`: all pass as scripted tests, but the implementations remain test doubles and do not exercise real timeout/build/test-failure child or reader cleanup.
- Disk-backed real baseline success/cache-hit cleanup probe passed. The first attempt with `/dev/shm` failed at link time with a bus error due to constrained tmpfs; rerun passed using the report's disk-backed target.
- Tested Git ref collision in a temporary repository and against the exact remediation crate: tag `oracle` points to commit A while `refs/oracle` points to B. Git's unqualified `rev-parse oracle^{commit}` selects B and `BaselineResolver::resolve()` incorrectly rejects the valid tag SHA A.

Resolved by remediation and reviewer verification:
- `G-F002-ADV-44`: mismatching expected observation content now fails comparison; adversarial test passes.
- `G-F002-ADV-45`: incompatible platform is rejected before target start; adversarial test passes.
- `G-F002-ADV-46` and `G-F002-ADV-31`: declared readiness data gates event delivery; contract and adversarial tests pass. G31 inspects the captured filesystem bytes rather than trusting target booleans.
- `G-F002-ADV-49`: OSC title sequence is removed; adversarial test passes.
- `G-F002-ADV-38`: staged metadata now includes actual profile/date and all tested values; revised serialized-metadata test passes.

Open finding:
- `G-F002-ADV-50`, medium, `baseline_resolution_is_qualified_to_tag_namespace`: `BaselineResolver::resolve` validates `refs/tags/<tag>` syntax but runs unqualified `rev-parse <tag>^{commit}`. A valid direct ref `refs/<tag>` shadows the tag and causes a false SHA rejection. Repro test is in `tools/ira-parity/tests/f002_adversarial.rs`; it fails against the exact remediation crate. Resolve via the explicit `refs/tags/<tag>^{commit}` refspec.

Verified fixes / passing probes:
- All 13 ignored baseline/cache/cleanup tests passed; extra real success + cache-hit cleanup probe passed.
- Existing supported-event live replay and unsupported repeat/release rejection passed on Linux.
- Existing normalization, byte comparison, mismatch diagnostic, staging, ordinary-run, environment isolation, malformed trace, and tagged resize replay tests passed.
- `G-F002-ADV-27` stays open for unproven timeout/test-failure exit modes. The real baseline probe verifies successful and cache-hit cleanup; the scripted labels exercise the same injected early-error branch.
- `G-F002-ADV-29` and `G-F002-ADV-30` stay open: current ignored tests use `ScriptedTarget` booleans rather than an actual child/PTY and reader cleanup path.

Mutation checks:
- Bypassed executable digest comparison locally; `cache_hit_verifies_metadata_and_executable_digest` failed as intended. Restored source.
- Disabled volatile-root replacement locally; normalization test failed as intended. Restored source.
- Changed returned readiness flag locally; readiness contract test failed as intended. Restored source.
- `git diff` confirms no production-source changes.

Resolved by review: `G-F002-ADV-31`, `G-F002-ADV-38`, `G-F002-ADV-44`, `G-F002-ADV-45`, `G-F002-ADV-46`, `G-F002-ADV-49`.
Open: `G-F002-ADV-27`, `G-F002-ADV-29`, `G-F002-ADV-30`, `G-F002-ADV-50`.

Verdict: FINDINGS (4 open GAPs: 27, 29, 30, 50).

Enrichment candidates: none.
