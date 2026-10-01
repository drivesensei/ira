# Decisions

Recommendations from every advisor report are answered in this log. No terminal-only behavior is waived or marked equivalent without the required three sign-offs.

## D-0001 Preserve and isolate the frozen TUI oracle
- Context: feature parity depends on the tagged TUI and existing GPUI shell is only a button demo.
- Options considered: port directly in the desktop main; preserve root TUI and extract UI-neutral behavior; change the TUI while extracting.
- Decision: preserve the TUI unchanged at `tui-oracle-baseline`; build an explicit UI-neutral core and GPUI adapter, with characterization coverage preceding behavior refactors.
- Rationale: architecture shell-audit R1/R2 ADOPTED; architecture-plan R1/R5 ADOPTED; platform-plan R1/R2 ADOPTED.
- Advisor: architecture-advisor `shell-audit.md`, `architecture-plan.md`; platform-advisor `platform-risks.md`, `platform-plan.md`.
- Adversarial: pending | Logic: pending
- Affects rows: F-001,F-002 and all feature rows.

## D-0002 Observable oracle-backed parity harness
- Context: existing root tests do not prove the desktop behavior matches the frozen TUI.
- Options considered: reuse core tests only; stable action/input traces compared against TUI captures and observable state/effects; manual-only checks.
- Decision: adopt stable traces and frozen oracle evidence, separating core, GPUI headless, and native OS-window checks; record source-only versus runtime evidence distinctly.
- Rationale: architecture-plan R3 ADOPTED; shell-audit R3 ADOPTED; platform-ci R2/R3 ADOPTED; UX keymodel keyboard test guidance ADOPTED.
- Advisor: architecture-advisor `architecture-plan.md`, `gpui-notes.md`; platform-advisor `platform-ci.md`; ux-advisor `ux-keymodel.md`.
- Adversarial: pending | Logic: pending
- Affects rows: F-002 and all parity rows.

## D-0003 Path identity and persistence contract before filesystem features
- Context: current TUI uses lossy strings in some paths while the migration charter requires native path types and byte-compatible persisted formats.
- Options considered: keep lossy String paths; use `PathBuf`/`OsString` internally with conversion only at display/persistence edges; change persisted formats.
- Decision: adopt native path values internally; specify stable entry identity and explicit encoding/round-trip policy before filesystem, transfer, bookmark or preview work; preserve TUI formats byte-for-byte unless a separately reviewed migration decision is added.
- Rationale: platform-risks R1/R3/R5/R6 ADOPTED; platform-plan R2/R7/R8 ADOPTED; architecture-plan R5/R6 ADOPTED.
- Advisor: platform-advisor `platform-risks.md`, `platform-plan.md`; architecture-advisor `architecture-plan.md`.
- Adversarial: pending | Logic: pending
- Affects rows: F-004,F-005,F-007,F-014 and filesystem/config/fileops/platform/preview rows.

## D-0004 GPUI 0.2.2 and desktop test contract
- Context: desktop has its own lockfile and pins GPUI 0.2.2; current shell has no tests.
- Options considered: follow current upstream docs/components; verify APIs and test support against the pinned source; upgrade GPUI during feature work.
- Decision: keep GPUI 0.2.2 while bootstrapping; confirm every signature against `desktop/Cargo.lock` and its local source; use its test-support APIs after recording exact dependency changes in the desktop lockfile. Revisit upgrades only as a separate decision.
- Rationale: architecture-plan R4 ADOPTED; `gpui-notes.md` exact-source audit ADOPTED.
- Advisor: architecture-advisor `gpui-notes.md`, `architecture-plan.md`.
- Adversarial: pending | Logic: pending
- Affects rows: F-001,F-002 and GPUI-facing rows.

## D-0005 Platform adapters and native integration
- Context: paths, permissions, process launch, clipboard, drives and file operations differ between macOS, Windows and Linux.
- Options considered: reuse terminal commands in the GPUI app; introduce typed platform services with platform-specific tests; defer cross-platform semantics.
- Decision: adopt a platform service boundary and test capability-specific behaviors; desktop clipboard/open/reveal/chooser use native adapters. Preserve Windows drive labels. Terminal image overlays, cell geometry, and font-probing mechanisms are not GPUI implementation requirements; user-visible image preview is preserved in a native pane. Rows F-068..F-070, F-072..F-073 and Linux-only F-088 remain BLOCKED until row-specific evidence and UX+logic+adversarial sign-off; no terminal-host implementation is scheduled.
- Rationale: platform-plan R3-R8 ADOPTED; platform-risks R2/R4/R7 ADOPTED; architecture-plan R6 ADOPTED.
- Advisor: platform-advisor `platform-risks.md`, `platform-ci.md`, `platform-plan.md`; architecture-advisor `architecture-plan.md`.
- Adversarial: pending | Logic: pending
- Affects rows: F-011,F-014,F-065..F-073,F-083..F-088,F-114..F-122.

## D-0006 Keyboard dispatch and desktop UX floor
- Context: modal priority, editor ownership, Ctrl behavior, and per-mode Escape semantics are part of IRA behavior; desktop mouse/window/accessibility support is mandatory floor.
- Options considered: GPUI-local per-view shortcuts; one ordered core dispatch policy plus GPUI event/focus adapter; standard desktop shortcuts may override IRA bindings.
- Decision: preserve the ordered mode dispatch from `ux-keymodel.md`; file actions use stable IDs across keyboard, mouse and menu paths; do not assign Cmd+C to copy, Cmd+V to file paste, Ctrl/Cmd+W to tab close, counts or multikey sequences. Add explicit UX-floor rows for mouse, lifecycle/reopen, native text/IME, theme/scaling, accessibility, dialogs, feedback and keyboard reachability. Adopt TUI-like default layout/state.
- Rationale: ux-keymodel R1-R4 ADOPTED; ux-layout R1-R4 ADOPTED; ux-plan R1-R5/R7 ADOPTED; platform-plan R5 ADOPTED.
- Advisor: ux-advisor `ux-keymodel.md`, `ux-layout.md`, `ux-plan.md`; platform-advisor `platform-plan.md`.
- Adversarial: pending | Logic: pending
- Affects rows: F-003,F-009..F-012,F-032..F-044,F-114..F-151.

## D-0007 Migration CI and enrichment gate
- Context: current desktop artifact workflow is not parity CI; desktop enhancements must not alter baseline parity.
- Options considered: reuse artifact workflow as test proof; add migration CI for root/TUI/core/desktop on Linux, macOS and Windows and keep artifact distribution separate; implement desktop extras alongside parity.
- Decision: adopt required locked fmt/clippy/test/build CI for supported targets and separate native GUI smoke evidence from compile jobs. Keep enrichments conditional on relevant parity rows being verified; resolve every E-row dependency to concrete parity IDs before any enrichment work.
- Rationale: platform-ci R1-R5 ADOPTED; ux-enrichment R1/R2/R4 ADOPTED; ux-plan R6 ADOPTED.
- Advisor: platform-advisor `platform-ci.md`, `platform-plan.md`; ux-advisor `ux-enrichment-plan.md`, `ux-plan.md`.
- Adversarial: pending | Logic: pending
- Affects rows: F-001,F-002,F-114..F-123,F-150 and all enrichment rows.

## D-0008 Analysis-plan revision required before implementation
- Context: the initial 113-row draft passes surface presence but not semantic ownership and bundles independent behaviors.
- Options considered: start developers from presence-only matrix; treat it as a discovery index, split behavior rows, audit each surface owner and re-review dependencies/waves before specs.
- Decision: ADOPT advisor direction: do not brief feature implementation against a row until the manager has corrected ownership, split behavior groups per the 3–15 test rule, completed the UX floor, and reconciled the plan reviews. The matrix now has 150 rows, a structural 418-item owner audit, behavior-sized input/render splits and WAVE-0..3 membership. Do not implement features until the final advisor re-review clears the plan. After that, F-001 may enter spec/pre-review/red-tests; implementation starts only after those gates pass.
- Rationale: architecture-plan R1/R2/R7; ux-plan R1-R5; platform-plan R1,R3-R5; later plan-review recommendations R1-R5 ADOPTED after matrix/wave corrections.
- Advisor: architecture-advisor `architecture-plan.md`; ux-advisor `ux-plan.md`; platform-advisor `platform-plan.md`.
- Adversarial: pending | Logic: pending
- Affects rows: all.

## D-0009 Baseline cleanliness and gate scope
- Context: the required final gate calls root-wide fmt/clippy while the frozen TUI currently has legacy findings; desktop has a separate manifest/lock and is not part of the root Cargo workspace.
- Options considered: lower the final quality bar; alter TUI code immediately; keep the oracle tag immutable while using characterization tests for any behavior-preserving cleanup, and ensure the gate/CI explicitly build, format, lint and test both manifests.
- Decision: ADAPTED from platform CI and architecture recommendations. Do not weaken final checks or alter behavior. Before Wave 0 exit, the integrator must make the gate cover the root TUI/core and separate GPUI manifest on all supported OSes. Existing TUI fmt/clippy findings must be resolved only through test-first, behavior-preserving changes against the frozen oracle, or the gate must use a narrowly justified tracked-baseline policy with a clean CI clone; do not include user-owned untracked examples in formatting scope. CI must include `cargo build --locked`, root tests, `cargo build --manifest-path desktop/Cargo.toml --locked`, desktop tests, scoped formatting/linting for both manifests, and native window smoke evidence.
- Rationale: platform-ci R1/R2/R5 ADAPTED; architecture-plan R8/R9 and shell-audit R3 ADOPTED.
- Advisor: architecture-advisor `shell-audit.md`, `architecture-plan.md`; platform-advisor `platform-ci.md`, `platform-plan.md`.
- Adversarial: pending | Logic: pending
- Affects rows: F-001,F-002,F-013,F-150 and all later rows.

## D-0010 Phase 2 plan review reconciliation and F-001 preparation
- Context: the final architecture/platform/UX re-reviews allowed F-001 spec and red-test preparation with a few explicit plan conditions.
- Recommendations and disposition: ADOPT architecture R12 (row-specific spec and red boundary checks before implementation) and architecture F-001 pre-review (add `ira-core` as a path dependency to both apps while retaining independent lockfiles and no umbrella workspace); ADOPT architecture R13 and platform R1 by placing F-013 after F-002 in Wave 0 Batch C and adding the graph edge; ADOPT platform R2 by replacing F-014 terminal-overlay TUI refs with platform service contracts; ADOPT platform F-001 pre-review by source-scanning core imports for forbidden host types and requiring F-150 locked builds for both manifests on all supported OSes; ADOPT UX R12 by splitting quit (F-034) from selected-entry info (new stable row F-151) and remapping the two question-mark surfaces; ADOPT architecture R14 by refreshing plan status text; ADAPT platform/UX proceed-with-conditions by retaining explicit specs and per-row evidence gates.
- Decision: Phase 2 plan review is cleared for F-001 specification and red-test preparation only. F-001 implementation remains blocked until its concrete spec receives architecture, platform, and UX pre-review and an adversarial reviewer commits executable failing boundary checks with GAP notes. The spec is approved after all three pre-reviews; boundary-test preparation is now authorized. No developer may start earlier.
- Advisor: architecture-advisor `architecture-plan.md` final follow-up; platform-advisor `platform-plan.md` round 4; ux-advisor `ux-plan.md` final review.
- Adversarial: pending | Logic: pending
- Affects rows: F-001,F-013,F-014,F-034,F-151,F-149.
