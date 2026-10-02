# Adversarial report: F-150 (mode A) round 1

Commit reviewed: `082e85eedb6145f97525ca3da3e5884f7e3e1712` (`main`, F-150 approved CI contract)

Attempts made:

- Inspected the approved F-150 spec, platform review, architecture review, reviewer protocol, parity-testing convention, and AGENTS.md §7.
- Confirmed `.github/workflows/migration-ci.yml` is absent; only the existing desktop packaging and release workflows are present.
- Ran `python3 -m unittest discover -s tests/ci -p 'test_*.py' -v`: **9 tests, 9 failures**, each reporting the missing migration workflow. This is the expected red baseline.
- The contract checks use Python's standard library and workflow text assertions; no external YAML parser dependency is needed.
- Runtime oracle/file-operation attacks are not applicable to this CI-only feature. No app, TUI, production source, packaging workflow, or release workflow was changed.

Findings:

| ID | Severity | Test | Missing contract |
|---|---|---|---|
| `G-F150-ADV-01` | high | `test_explicit_three_os_runners_and_runner_image_evidence` | Explicit `ubuntu-24.04`, `macos-15`, `windows-2022` jobs and `ImageOS`/`ImageVersion` evidence per job. |
| `G-F150-ADV-02` | high | `test_each_independent_package_runs_locked_build_test_clippy_and_fmt` | Explicit locked build, all-target test, all-target clippy (`-D warnings`), and manifest-scoped fmt check for root, desktop, and parity manifests. |
| `G-F150-ADV-03` | high | `test_selects_the_named_live_replay_without_blanket_ignored` | Select the exact named live trace on each runner, avoid blanket `--ignored`, and report one pass, scenario, baseline SHA, cleanup. |
| `G-F150-ADV-04` | high | `test_fetches_and_verifies_the_immutable_oracle_baseline` | Fetch and verify/log the frozen tag and exact peeled baseline SHA. |
| `G-F150-ADV-05` | high | `test_audits_the_pinned_full_parity_lockfile_closure` | Pinned cargo-deny and policy/inventory over the standalone parity lockfile, including target profiles and unknown-license denial. |
| `G-F150-ADV-06` | high | `test_ci_is_read_only_and_never_publishes_release_assets` | Read-only repository permission and no release creation or release-asset publication. |
| `G-F150-ADV-07` | high | `test_native_gui_smoke_is_real_or_explicitly_unresolved_experiment` | Native visible-window/close/deadline smoke, or a clearly unresolved hosted-GUI experiment gate. |
| `G-F150-ADV-08` | medium | `test_triggers_pin_toolchain_and_cache_keys_cover_lockfiles` | PR, integration push, manual dispatch; exact Rust pin/version output; cache dimensions and clean checkout. |
| `G-F150-ADV-09` | medium | `test_failures_are_not_suppressed_and_evidence_has_finite_retention` | No suppressed failures/hidden skips; bounded timeouts and finite evidence retention. |

Mutation checks: Not applicable before workflow implementation; these tests assert the required workflow contract directly and all currently fail on workflow absence.

Verified fixes: None (red-test preparation).

Reopened: None.

Verdict: **FINDINGS: 9 open (7 high, 2 medium)**.

Enrichment candidates: None.
