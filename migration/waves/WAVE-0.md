# Wave 0: Common ground and desktop interaction contracts
Status: in progress — F-002 redesign red-test gates
Entry criteria: Phase 2 plan review cleared; DECISIONS.md records advisor responses. F-001's spec may be drafted and reviewed after this entry condition; every row needs its own spec and red tests before its implementation batch starts.
Exit criteria: F-001..F-014 and F-150 pass feature review and tests; shell remains launchable; no TUI behavior changes; macOS and Windows migration CI green.

| Row | Batch | Developer branch | Write-set | Reads | Reviewers | Advisors | Status |
|---|---|---|---|---|---|---|---|
| F-001 | A (barrier) | migrate/F-001-core-boundary | root `Cargo.toml`/`Cargo.lock`; `desktop/Cargo.toml`/`desktop/Cargo.lock`; `crates/core/**`; minimal `desktop/src/main.rs` wiring; `tests/boundary/**` | root TUI and GPUI shell | adv, logic | arch, platform | NOT_STARTED |
| F-002 | B (barrier) | migrate/F-002-review-fixes | `tools/ira-parity/**`; `migration/oracle/traces/**`; `migration/reports/F-002/**` | F-001 contracts; S12-S14 in D-0014 | adv, logic | arch, platform | RED TESTS (S13/S14 in progress); implementation gated |
| F-013 | C | migrate/F-013-perf-baseline | `migration/oracle/perf.md`; `tests/perf/**` | frozen TUI; F-001,F-002 | adv, logic | arch, platform | NOT_STARTED |
| F-150 | C | migrate/F-150-ci | `.github/workflows/migration-ci.yml`; CI docs/scripts | F-001,F-002 | adv, logic | platform, arch | NOT_STARTED |
| F-004 | C (barrier) | migrate/F-004-state-model | `crates/core/src/state/**` | F-001,F-002 | adv, logic | arch, ux | NOT_STARTED |
| F-005 | D | migrate/F-005-filesystem | `crates/core/src/fs/**` | F-001,F-004 | adv, logic | arch, platform | NOT_STARTED |
| F-007 | D | migrate/F-007-config | `crates/core/src/config/**`; `crates/core/src/persistence/**` | F-001,F-004 | adv, logic | arch, platform | NOT_STARTED |
| F-014 | D | migrate/F-014-platform | `crates/core/src/platform/contracts/**`; `desktop/src/platform/contracts/**` | F-001,F-004 | adv, logic | platform, arch | NOT_STARTED |
| F-003 | D | migrate/F-003-actions | `crates/core/src/actions/**`; `desktop/src/keymap/**` | F-001,F-002,F-004 | adv, logic | ux, arch | NOT_STARTED |
| F-006 | E | migrate/F-006-jobs | `crates/core/src/jobs/**`; `desktop/src/jobs/**` | F-001,F-004,F-005 | adv, logic | arch, platform | NOT_STARTED |
| F-008 | E | migrate/F-008-theme | `desktop/src/theme/**`; `crates/core/src/theme/**` | F-001,F-007 | adv, logic | ux, arch | NOT_STARTED |
| F-011 | E | migrate/F-011-event-adapter | `desktop/src/events/**`; `desktop/src/focus/**` | F-001,F-003 | adv, logic | platform, ux | NOT_STARTED |
| F-009 | F (barrier) | migrate/F-009-file-list | `desktop/src/file_list/**`; `desktop/src/panes/**` | F-003,F-004,F-005,F-008,F-011,F-013 | adv, logic | ux, arch | NOT_STARTED |
| F-010 | G | migrate/F-010-overlays | `desktop/src/overlays/**`; `desktop/src/inputs/**` | F-003,F-008,F-011 | adv, logic | ux, arch | NOT_STARTED |
| F-012 | H | migrate/F-012-responsive | `desktop/src/layout/**`; `tests/layout/**` | F-009,F-010,F-011 | adv, logic | ux, platform | NOT_STARTED |

Conflicts considered: F-002 owns only scenario/harness paths; F-013 owns the distinct performance baseline and fixtures. F-004 waits for F-002. Batches D and E have separate core contracts/modules; central manifests and registries are integrator-only. F-009 waits for the event/focus contract and performance baseline. F-010 waits for the native event/input adapter. Platform feature rows must use the F-014 service contracts, not share its contract files.

Barrier steps: F-001 workspace/core boundary; F-002 oracle harness (owned PTY lifecycle, typed captured-observation bundle, OS-specific paste contract); F-004 state schema; F-009 list/pane contract. F-002 implementation does not start until real helper-child tests and S13/S14 red tests are committed. Every other foundation row requires its own spec and adversarial red tests before its batch begins.
Pre-wired slots: stable action IDs, typed path/entry identity, typed errors/effects, state composition, platform service traits, async job handles, per-feature key contexts and registration table.
