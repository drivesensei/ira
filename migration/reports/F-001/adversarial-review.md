# Adversarial report: F-001 (mode B) round 1

Commit reviewed: `b06c919e451e307a879d4a460596ef6f08d49d25` (implementation `296c8927b57e526b2144f431397d2343e3aeb414`)

Attempts made:
- Ran all seven tests in `tests/boundary/test_f001_boundary.py`, including locked root/desktop builds and root test suite: passed.
- Checked both local dependencies resolve to `crates/core`, lockfiles are distinct and include `ira-core`, and neither manifest declares a workspace: passed.
- Checked core dependency declarations and source imports for UI crates, the TUI source delta from `tui-oracle-baseline`, and desktop reopen/window/button source markers: passed. `desktop/src/main.rs` is unchanged in the reviewed diff.
- No oracle/UI interaction attempted: F-001 has no end-user behavior. No visual parity claim is made.

Findings:
- `G-F001-ADV-05`, low, `test-gap`: prior S3 checks could be bypassed by dependency rename or absolute crate path. Added executable regression test `test_s3_ui_neutrality_rejects_renamed_dependencies_and_absolute_imports`; verified below. No implementation divergence found.

Mutation checks:
- Added `ui = { package = "gpui", version = "0.2.2" }` temporarily to core manifest; regression test failed on forbidden package identity. Reverted.
- Added `use ::gpui::App;` temporarily to core source; regression test failed on import scan. Reverted.
- No other production mutation attempted; the two critical S3 bypass cases are directly exercised.

Verified fixes: `G-F001-ADV-03`, `G-F001-ADV-04` (tests S3/S4 pass; manifest and package graph inspected).
Resolved: `G-F001-ADV-05` (regression test catches both mutations).
Reopened: none.

Test command: `python3 -m unittest discover -s tests/boundary -p 'test_*.py' -v` — 7 passed.
Scope check: `python3 scripts/scope_check.py reviewer --range 9b08eb9..HEAD` with the brief's exact allowlist — failed because the range includes the reviewed implementation's expected `Cargo.toml`/lockfiles and `crates/core/**`. My review changes are only `tests/boundary/test_f001_boundary.py` and this report; temporary mutations were reverted.

Verdict: FINDINGS: 1 low test-coverage gap, now covered and resolved; no open implementation findings.
Enrichment candidates: none.
