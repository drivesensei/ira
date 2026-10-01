# Adversarial report: F-001 (mode A: red-test preparation)

Commit reviewed: `7ac63fd` (approved F-001 plan/spec baseline; working tree had additional manager-owned planning edits)
Attempts made:
- Read F-001 spec, D-0001/D-0004/D-0008/D-0009/D-0010, WAVE-0, root and desktop manifests, root TUI entrypoint, and the existing desktop shell.
- Checked the preserved TUI at runtime with `cargo run --locked -- --version`; observed `ira 0.1.21`.
- Confirmed root package identity/dependencies, desktop package identity and GPUI 0.2.2, existing `on_reopen`/window/button handlers, and locked builds.
- Checked the TUI source against `tui-oracle-baseline`; it matches. This is a structural boundary row with no new user-visible behavior, so an interactive TUI trace was not applicable.
- Ran root tests. The exact `TMPDIR="$PWD/target/tmp" cargo test --locked` command passed on direct rerun (218 unit + 2 drive + 52 operations + 6 sort = 278; doctests 0). One earlier parallel run had a transient `spawn_first_available_runs_a_candidate_in_the_given_folder` failure; isolated and subsequent runs passed. The boundary test uses single-thread mode for stable bundled verification.

Findings:
- `GAP(G-F001-ADV-03)` sev=high kind=missing-feature feature=F-001 — no `crates/core/Cargo.toml` or library exists; `test_s3_core_is_ui_neutral_and_hosts_are_isolated` fails at the missing manifest check.
- `GAP(G-F001-ADV-04)` sev=high kind=missing-feature feature=F-001 — neither host declares the required local `ira-core` dependency; `test_s4_local_core_and_independent_cargo_graphs` fails on the root manifest before lockfile/core package checks.

Executable checks: `tests/boundary/test_f001_boundary.py`, run with `python3 -m unittest discover -s tests/boundary -p 'test_*.py' -v`. Six test methods cover S1-S6. Final red result: S1, S2, S5, and S6 passed; S3 and S4 failed as intended (2 failures). The S5 check builds both manifests with `--locked` and runs the root suite serially; each completed successfully.

Attack checks attempted: missing package/path edge; prohibited host dependencies/imports; root-to-GUI and desktop-to-TUI import isolation; umbrella-workspace introduction; lockfile separation; shell reopen/button preservation; root TUI source drift. Checks that did not depend on the not-yet-created core passed. No mutation experiments were applicable before implementation.

Scope: only `tests/boundary/test_f001_boundary.py` and this report were authored. `git diff -- src desktop/src` was empty. `python scripts/scope_check.py reviewer --range 7ac63fd..HEAD` returned 1 because it enumerated 8,410 untracked/generated files, chiefly existing `desktop/target/` and `wasm-demo/target/` artifacts plus manager-owned untracked migration files. Those files were not modified or staged. The scope checker does not provide an exclude option for this situation.

Verdict: FINDINGS (2 open boundary gaps). No `#[ignore]` markers were added. No UI behavior divergence is claimed. The developer should create the minimal `ira-core` crate and add its local path dependency to both independent app manifests, then rerun this boundary suite; the logic and adversarial review gates still apply afterward.
