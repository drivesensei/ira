# Advisory: migration plan by architecture-advisor round 1

Scope reviewed: `migration/PARITY_MATRIX.md`, `migration/STATE.md`, all nine `migration/inventory/*.md` slices, `migration/reports/gpui-notes.md`, `migration/reports/shell-audit.md`, and existing UX/platform advisory reports.

Summary verdict: **PROCEED WITH CHANGES**. The shell audit and UX/platform direction support a GPUI adapter over a UI-independent behavioral core. The current matrix is not yet an implementable dependency/wave plan: most dependencies are blank, nearly all rows are placed in wave 1, and several surface assignments visibly do not describe their owning feature. Resolve the ranked items below before briefing feature developers.

## Recommendations (ranked)

### R1 [must] Establish an explicit foundation DAG and separate foundation contracts from parity features

Why: `PARITY_MATRIX.md` defines F-001..F-014 as foundation in wave 0, but all remaining 86 rows are wave 1 and have `Deps: -`; this contradicts `wave-planning/SKILL.md`'s topological layers and makes the dependency graph impossible to audit. The shell is just a one-view click demo (`shell-audit.md`, `desktop/src/main.rs`) with no shared state, actions, fs, or UI tests. Build and land the minimal core/app boundary and parity harness contracts first, then establish contracts for app state, paths/entry identity, action dispatch, filesystem/effects/jobs, persistence, and UI composition before features. F-003/F-004/F-005/F-006/F-007 currently imply some dependencies but need exact edges and sequencing; F-008 should follow the compatible config contract, while F-009/F-010 depend on action/state/theme and must precede UI feature composition. F-011 and F-014 overlap on lifecycle, text input, and native dialogs and need distinct contracts/ownership. Assign each foundation item an owner, write-set, acceptance tests, and exit criteria in `waves/`.

Cost/risk of ignoring: developers will invent incompatible state/action/effect contracts, collide in shared files, and push feature behavior into the view layer; integration churn will grow and parity cannot be replayed uniformly.

Alternatives considered: build a feature slice directly in `desktop/src/main.rs`; rejected because the audited shell has no established feature architecture and that would make later separation a risky rewrite.

### R2 [must] Repair semantic surface ownership before using matrix coverage as evidence

Why: several assignments appear unrelated to their row by inspection: F-027 (version early exit) owns `mode:goto-path` and `key:normal:plus`; F-028 (terminal diagnostic) owns image/path environment keys; F-052 (copy) owns symlink classification; F-055 (delete confirmation) owns `key:goto-path:backspace`; F-073 (terminal font settings) owns `IRA_THUMBNAIL_CACHE_DIR`; F-015/F-016/F-017 split config/session/theme fields inconsistently; F-069/F-070 are native preview overlays rather than desktop file previews. Broad rendering rows also combine many unrelated states. Since the parity gate checks only whether a surface string appears in any row, syntactic coverage can pass while semantic coverage is false. Reassign each item by tracing it to its inventory trigger/behavior and the relevant source; split rows where one surface spans distinct features. Add an ownership review/check or an annotated mapping so every item has a meaningful owner.

Cost/risk of ignoring: false-positive surface coverage can make the completion gate pass while CLI, config, platform, and UI behavior remains unimplemented or tested under the wrong feature.

Alternatives considered: leave initial draft assignments and rely on feature implementers to notice; rejected because coverage is a mandatory gate and ownership drives review scope.

### R3 [must] Specify parity harness as observable traces with independent TUI oracle evidence

Why: F-002 currently has no contract details and the GPUI notes describe possible test-support facilities but the desktop has no test-support dependency or tests (`gpui-notes.md`, `shell-audit.md`). The oracle is frozen at `tui-oracle-baseline`; the architecture should replay stable actions/inputs and compare observable outcomes (active pane/path, ordered rows, selection, messages, persisted bytes, filesystem effects, exit behavior). Keep TUI execution/capture tooling separate from the new core; do not claim equivalence merely by testing the same newly extracted implementation twice. Baseline PTY captures and existing TUI tests are useful evidence but have unequal coverage; label source-only and runtime evidence explicitly. Keep GPUI headless wiring tests distinct from OS-window smoke checks.

Cost/risk of ignoring: tests may prove only internal consistency, not parity; terminal event priority, messages, persistence timing, and side effects can drift unnoticed.

Alternatives considered: rely on unit tests plus manual visual checks; rejected as insufficient for non-visual behavior and byte-compatibility.

### R4 [must] Treat `gpui 0.2.2` and the separate desktop lockfile as hard implementation constraints

Why: `desktop/Cargo.toml` and `desktop/Cargo.lock` independently pin crates.io GPUI 0.2.2; `gpui-notes.md` confirms relevant APIs against that local source and explicitly notes Context7 was unavailable. Use the audited idioms (`Application::on_reopen`, `App::open_window`, `Entity`/`Context`, `cx.listener`, `cx.notify`) and exact 0.2.2 test-support signatures/examples. Recheck every API and component dependency against the locked source, not current GPUI docs or a contemporary `gpui-kit` release. Make desktop tests/build workflow cover the desktop manifest and keep dependency changes in its lockfile; do not accidentally assume root `Cargo.lock` controls GPUI.

Cost/risk of ignoring: stale signatures, mismatched transitive GPUI snapshots, and platform-only compile failures can derail implementation late.

Alternatives considered: adopt current Zed UI/component crates immediately; rejected because compatibility and dependency/license closure are unverified in `gpui-notes.md`.

### R5 [should] Freeze shared state and effect contracts before feature parallelism

Why: The inventories show broad coupling: panes, cursor/selection, sorting/filter, preview mode, bookmarks, jobs, dialog priority, and session serialization are read across slices (e.g. `config-state.md` INV-STATE-001..006 and `entry-input.md` INV-INPUT-001..013). UX keymodel R1 requires exact ordered dispatch, while platform reports require native path types, platform adapters, and async operations. Define core-owned `PathBuf`/`OsString` identities, stable entry identity, per-pane composition, explicit mode/focus and action dispatch order, typed messages/errors, `Effect`/job boundaries, and persisted-format compatibility tests as contracts. Avoid feature developers adding fields to one shared app struct; use composed per-feature state and an integrator-owned contract change barrier. Preserve root TUI untouched as oracle.

Cost/risk of ignoring: cursor/selection corruption on refresh/sort, key dispatch drift, text input interception, and lossy path serialization may be duplicated in multiple features.

Alternatives considered: let each UI feature own its own state and translate ad hoc; rejected for state read/write dependencies and ordered modal semantics in the inventories.

### R6 [should] Schedule filesystem scale and platform behavior as foundation adapters, not late polish

Why: `gpui-engineering` requires virtualized lists and cancellable background work; inventories include large-directory streaming, symlink/special files, recursive sizing, cross-volume transfer, external open/reveal, and platform drives. The platform reports call out path fidelity, macOS permissions, Windows ACL/locking, collisions, and native launch behavior. Current F-005/F-006 are rightly foundation candidates, but F-013 performance baselines and F-014 platform integration must specify measurable contracts and realistic CI/manual evidence. Keep blocking I/O off the GPUI thread and don't let a POSIX-shaped path/type model become a prerequisite that later must be broken.

Cost/risk of ignoring: UI freezes and platform correctness work gets deferred until it is entangled with every feature.

Alternatives considered: implement platform behavior opportunistically per feature; rejected for shared path/error/process semantics.

### R7 [should] Split broad matrix rows and order features by dependencies, not inventory numbering

Why: Rows such as F-032 (event loop and paste routing), F-034 (file/view actions plus help/error modes), F-035 (delete/copy/move/selection/eject bindings), F-093..F-101 (multiple visual components/messages), and F-011 combine independently testable behavior with different dependencies and likely write-sets. Conversely, config/session rows F-015..F-020 omit obvious dependencies on config loader, state model, and theme foundation. Derive edges from API, state, UI composition, and semantics per `wave-planning/SKILL.md`; then group only when a single implementation/review boundary truly owns the whole behavior. Use wave 0 for contracts and shared components, then topological feature layers, and only parallelize disjoint declared write-sets with explicit collision checks.

Cost/risk of ignoring: oversized rows hide partial completion and reviewers cannot isolate gaps; arbitrary wave placement schedules dependent features too early.

Alternatives considered: retain one row per inventory header; acceptable only for inventory bookkeeping, not when behavior has distinct acceptance tests or owners.

## Risks not covered by any recommendation

- No desktop behavior has yet been integrated with the TUI library; the baseline tag is the only behavioral authority.
- UX floor (mouse, lifecycle/reopen, theme, native text/IME, accessibility, OS integration) is additive but cross-cuts parity and needs explicit matrix ownership; UX reports correctly say these are not E-xxx enrichments.
- Root-wide fmt/clippy currently has pre-existing findings according to `STATE.md`; the gate needs a decision that preserves the oracle while making migration-owned checks meaningful.
- Visual/window-manager behavior cannot be established by headless GPUI tests alone; CI/manual evidence must distinguish those claims.

## Questions for the manager

- None. The plan is actionable after ranked items are resolved and recorded in `DECISIONS.md`.

---

# Follow-up: revised plan review

Scope reviewed: current `migration/PARITY_MATRIX.md`, `migration/surface-ownership.tsv`, `migration/DECISIONS.md`, and `migration/waves/GRAPH.md`, `WAVE-0.md` through `WAVE-3.md`.

Verdict: **STOP — do not start F-001 yet.** The revised work is materially better: all 133 matrix rows appear in exactly one wave, and all 418 items in `oracle/surface.txt` occur once in the ownership map and in their listed matrix owner's Surface cell. The foundation chain is clearer and the decisions capture the major architecture constraints. Two plan-level blockers remain, including the explicit Wave 0 entry criteria for F-001.

## Residual recommendations

### R8 [must] Satisfy the Wave 0 entry criteria with an F-001 spec and red tests before implementation

Why: `WAVE-0.md` says foundation specs and red tests must be approved before the wave begins, and the charter requires test-first work. No F-001 spec or F-001 test plan/artifact is present under `migration/` or `tests/`; the matrix row only says “Core crate and TUI/GPUI boundary.” Define the smallest contract: which crate owns UI-neutral types/behavior, what the desktop may depend on, how the existing root TUI remains independent/frozen, which manifests and lockfiles may change, and build/test acceptance on the root and desktop manifests. Add the boundary/build checks as initially failing tests or scripted checks before changing workspace architecture. Once those are approved, F-001 is appropriately first as a serial barrier.

Cost/risk of ignoring: the first structural change can silently couple the TUI to GPUI, alter the oracle package, or strand either app without a reproducible build.

### R9 [must] Resolve the F-002/F-004 dependency contradiction and Wave 0 batch overlap

Why: `waves/GRAPH.md` has F-002 → F-004, but `WAVE-0.md` places F-002 and F-004 in the same parallel Batch B. The wave table's “F-001 contracts” read-set for F-004 also conflicts with the graph edge. F-002's declared write-set `migration/oracle/**` overlaps F-013's `migration/oracle/perf.md`, although both are placed in Batch B and the conflict statement says those write-sets are disjoint. Either remove the F-002 → F-004 dependency with rationale, or move F-004 after the harness contract; narrow F-002's write-set (or serialize the conflicting files) before claiming the batches are parallel-safe.

Cost/risk of ignoring: implementation starts against contradictory prerequisite contracts or concurrent changes to shared oracle artifacts.

### R10 [should] Tighten F-001's write-set and list explicit non-goals

Why: Wave 0 assigns F-001 `Cargo workspace/manifests; crates/core/**; desktop/** shell wiring`. That is broad for a “boundary” task and `desktop/**` overlaps all subsequent desktop foundation write-sets. Its barrier placement can protect sequencing, but the task brief should restrict it to the crate skeleton, package wiring, and minimum launch adaptation; reserve feature modules and shared action/state/fs contracts for their own rows. Explicitly forbid edits to root `src/**` and behavior changes to the frozen oracle, and say whether root `Cargo.lock`, desktop `Cargo.lock`, and workspace metadata are in scope.

Cost/risk of ignoring: F-001 becomes an umbrella implementation that absorbs later rows and makes review, ownership, and matrix evidence ambiguous.

### R11 [should] Clarify treatment of terminal-only overlay rows before their waves

Why: F-068..F-070 still describe terminal cell geometry/native overlay behavior, while D-0005 says these mechanisms are not GPUI requirements and the charter requires UX + logic + adversarial sign-off for terminal-only equivalence. Wave 2 correctly says to obtain row-level mapping/sign-off before implementation, but the rows currently remain ordinary `NOT_STARTED` parity rows without an explicit disposition. Add row-specific equivalent behavior or an approved decision/sign-off path before those rows are scheduled; do not infer surface coverage means a GPUI overlay is required.

Cost/risk of ignoring: either terminal-specific implementation leaks into the desktop architecture or parity is silently waived without required evidence.

Risks not covered: the specific assignments of all 418 items are structurally consistent between TSV and matrix, but this check does not independently prove every owner is semantically correct against its inventory; such review remains required at feature-spec time. The central surface file and ownership map currently agree on 418 items.

---

# Final follow-up: latest wave revision

Scope reviewed: latest `PARITY_MATRIX.md`, `surface-ownership.tsv`, `DECISIONS.md`, `waves/GRAPH.md`, and `waves/WAVE-0.md` through `WAVE-3.md`.

Final verdict: **PROCEED WITH CHANGES for F-001 spec and red-test preparation; do not begin F-001 implementation yet.** The reviewed revision resolves the principal sequencing and overlap findings: F-004 now follows F-002, F-013 has separate paths from the harness, Wave 0 is explicitly ordered, F-001 has a narrower desktop write-set, and the F-001 spec/review/red-test prerequisite is stated. I checked 150 matrix rows: every row appears in exactly one of WAVE-0..3. The ownership ledger and surface file both contain 418 entries, with exact set equality, and every ledger owner matches the corresponding matrix Surface cell.

## Remaining recommendations

### R12 [must before F-001 implementation] Approve and land the F-001 spec, review, and red boundary checks

F-001 is correctly placed as the first serial barrier, and `WAVE-0.md` now explicitly allows its spec to be drafted after plan review while requiring a row-specific spec and red tests before implementation. Preparation may begin. The actual F-001 contract must constrain changes to the core crate/package wiring and minimal desktop launch adaptation; preserve the TUI at `tui-oracle-baseline`, keep UI-neutral code free of GPUI/Ratatui/Crossterm coupling, specify the separate root and desktop lockfile handling, and require both existing app manifests to remain buildable. Review that concrete spec and land executable boundary/build checks before assigning F-001 implementation.

Cost/risk of ignoring: a broad “core boundary” change can accidentally rewrite TUI behavior or entangle UI dependencies before there is an enforceable boundary test.

### R13 [must before Wave 0 batch B] Resolve F-013's remaining dependency disagreement

The matrix lists F-013 dependencies as `F-001,F-002`, but `WAVE-0.md` puts F-013 in Batch B alongside F-002 and lists its reads as only the TUI baseline and F-001; `GRAPH.md` likewise shows only F-001 → F-013. This is a real dependency-vs-parallel-batch mismatch even though the paths are now disjoint. Either remove F-002 from F-013's Deps if no harness contract is actually needed, or move F-013 to a later batch after F-002. It does not block F-001 spec preparation or F-001 implementation by itself, but it must be corrected before scheduling Batch B.

Cost/risk of ignoring: the performance baseline either starts before its declared prerequisite or unnecessarily prevents safe parallelism.

### R14 [should before implementation] Refresh stale plan status text

`PARITY_MATRIX.md` still describes the matrix as an “initial manager draft” whose waves/dependencies “will be refined,” while `DECISIONS.md` D-0008 says the current matrix and WAVE-0 are working drafts only. Reconcile those statements with the new review status before treating the Phase 2 plan review as cleared. Keep any remaining row-level semantic questions, including held terminal-host behavior rows, gated at their feature specs as already stated in Wave 2/3.

Cost/risk of ignoring: implementers cannot tell whether the matrix is the reviewed source of truth or still provisional.

F-001 spec/red-test preparation may begin once the manager records the Phase 2 plan review as cleared. F-001 implementation begins only after that row's own spec is approved and its red boundary checks are in place. No production/planning files were edited in this review.

---

# F-001 specification pre-review

Scope reviewed: `migration/specs/F-001.md`, F-001 in `migration/PARITY_MATRIX.md` and `waves/WAVE-0.md`, D-0010 in `migration/DECISIONS.md`, current root and desktop manifests, `desktop/src/main.rs`, and the pinned-version notes in `migration/reports/gpui-notes.md`.

Verdict: **PROCEED WITH CHANGES — approve both applications depending on `ira-core` from the first F-001 implementation commit.** The structural boundary is appropriately narrow: preserve separate packages/lockfiles, avoid a workspace, keep root `src/**` untouched, and leave feature behavior to later rows. This review approves the dependency-direction choice and the architecture of the spec; it does not waive D-0010's separate UX/platform pre-reviews or adversarial red-test prerequisite.

## Findings

### A1 [must] Make both manifests consume `ira-core` from the outset

Adopt the spec's preferred option: add a direct local path dependency on `crates/core` from root `ira` and `desktop/ira-desktop` in the same F-001 change. Keep the crate API empty/minimal initially. This demonstrates the intended shared dependency direction and proves that the crate builds in each package graph without requiring either UI host to import the other's types. Update root `Cargo.lock` and `desktop/Cargo.lock` independently. Do not create a workspace or share a lockfile; this agrees with the repo's current separate manifests and GPUI pin. A root package dependency alone does not change TUI behavior, while postponing it leaves the central two-host boundary unproven.

### A2 [should] Tighten the boundary check assertions to inspect both dependency graphs

The listed S1–S6 checks are testable. Ensure S3/S4 assert that each manifest resolves the same local `ira-core` package and that its direct dependency list excludes GPUI, Ratatui, and Crossterm; assert that root and desktop remain separate lockfile roots and that no workspace was introduced. S5 should run the exact locked build/test commands independently; a desktop native-window launch remains a manual/OS-runner smoke check, not a headless package-graph claim. Preserve the existing app's `Application::new().run`, window-open/reopen registration, listener, and notification idioms in any necessary minimal shell wiring.

### A3 [should] Keep GPUI out of F-001's new API and defer test-support enablement

The spec correctly preserves GPUI 0.2.2 and makes no workspace or upgrade change. F-001 need not add GPUI `test-support`: its acceptance is package-boundary/build behavior, and `desktop/Cargo.toml` currently pins GPUI 0.2.2 independently. If the minimal shell wiring needs a GPUI API edit, verify the exact signature from that locked source, as `gpui-notes.md` requires; do not infer APIs from current upstream examples. Add GPUI test support with the UI-wiring test row that actually needs it.

## Readiness

The spec is sufficiently concrete for the adversarial reviewer to prepare and land failing checks under `tests/boundary/`; it clearly forbids production/TUI edits outside its bounded write-set. **F-001 implementation remains blocked** until architecture/platform/UX pre-reviews are recorded on this exact spec, the red checks and GAP notes exist, and the manager approves the reviewed spec. No source or test files were changed in this review.

---

# F-002 specification pre-review

Scope reviewed: `migration/specs/F-002.md`, matrix row F-002, D-0002/D-0004/D-0009/D-0010, `.cursor/skills/parity-testing/SKILL.md`, `scripts/parity_gate.py`, existing root tests and F-001 boundary tests, `migration/oracle/tools/render_ansi_capture.py`, oracle captures, and GPUI architecture notes.

Verdict: **PROCEED WITH CHANGES; F-002 red-test preparation should wait until these concrete choices are adopted into the spec and the platform-advisor confirms the PTY dependency/configuration.** The draft has good isolation, fail-closed, human-reviewed-golden, and capability-based observation goals. Its location, input/target boundary, baseline artifact and dependency ownership are still open, and the current proposed write-set disagrees with the parity-testing skill's trace location and tests-in-the-owning-crate convention.

## Ranked recommendations

### R1 [must] Put the harness in a standalone developer tool crate and give it its own manifest ownership

Recommended home: `tools/ira-parity/` as an independent Cargo package (not a member of a root/desktop workspace). Put unit/integration tests under `tools/ira-parity/tests/**`, the versioned input traces under `migration/oracle/traces/<area>/*.trace` as required by `parity-testing/SKILL.md`, and scenario-specific expected observations in those traces or a clearly paired `migration/oracle/goldens/**` directory. Preserve `migration/oracle/captures/**` as existing checked discovery evidence; don't retrofit/overwrite them as generated goldens. The tool crate owns `tools/ira-parity/Cargo.toml` and its own `Cargo.lock`. No root `Cargo.toml`/`Cargo.lock`, desktop manifest/lock, root `tests/*.rs`, shared scripts, or workflow changes are required for F-002; F-150 owns CI wiring. Invoke explicitly with `cargo test --manifest-path tools/ira-parity/Cargo.toml --locked` / `cargo run --manifest-path ...`; do not add a workspace or root integration target just to register it.

Why: root and desktop remain independent Cargo packages and lockfiles (D-0004); F-002 needs PTY/trace dependencies that should not leak into either shipped app. The current draft says `tests/parity/**`, `migration/oracle/scenarios/**`, and “manifest/package only if” while `parity-testing` specifies a parity crate and `migration/oracle/traces/**`. Decide the package now and narrow the F-002 matrix/WAVE-0 write-set to the standalone tool, traces/goldens, and their tests.

### R2 [must] Define a stable event and observation adapter contract, independent of GPUI and F-003 actions

Keep the trace protocol terminal-semantic rather than action-named: a versioned ordered sequence of key events (`code`, modifiers, press/repeat/release kind), text/paste, and resize. Use a machine-readable TOML v1 document for `*.trace` with explicit fixture recipe, initial environment, dimensions, ordered events and observation assertions. Define a small `TraceTarget` boundary with lifecycle/setup, `apply(InputEvent)`, `observe(ObservationKind)`, and shutdown/exit reporting. Oracle target owns PTY/Crossterm encoding and terminal screen parsing; a later core target maps the same semantic events into a core-owned input boundary; an optional later GPUI target maps into 0.2.2 test support. The harness protocol must contain no GPUI types, Crossterm types, `F-003` action IDs, or view structs.

Use a neutral observation vocabulary for process status/stdout/stderr, terminal screen, domain snapshots (cwd/panes/ordered entries/cursor/selection/message), filesystem results, and persisted bytes. Each adapter declares/supplies supported observations; compare only observations asserted by a trace and supported by both selected targets, with a clear diagnostic for unsupported requests. Keep native OS-window screenshots/lifecycle smoke evidence separate under D-0002; it is not a deterministic F-002 screen-observation adapter. This resolves the current ambiguity in S3/S6, where terminal screen data and a non-UI core target are described as though they have the same observations.

The existing `render_ansi_capture.py` handles a small subset of CSI/OSC sequences for a final 16/256-color screen; it is not an interactive PTY or general terminal emulator. Do not treat its output as a complete normalizer. Test cursor, wide Unicode, erase, alternate-screen, title/OSC, and resize/input handling that the TUI actually emits, or choose a pinned VT parser and confirm the exact supported sequence subset from real captures.

### R3 [must] Make the baseline executable reproducible and identity-checked

Use a detached temporary worktree at the frozen tag, and verify `git rev-parse tui-oracle-baseline^{commit}` equals the pinned SHA `1cad4ce43cc72d52d4cc4eef920e0da22cb69568` before building. Build the root `ira` binary once per job/target with the manifest and `Cargo.lock` from that worktree, `--locked`, and an isolated `CARGO_TARGET_DIR`; reuse that artifact for all scenarios in the run. Store it under an ignored cache such as `target/oracle/<commit>/<target>/` with a sidecar recording tag, commit, target triple, `rustc -Vv`, baseline lockfile digest, build profile and binary SHA-256. On reuse, verify every field and the executable hash; on missing/mismatch, rebuild from the verified worktree or fail with an actionable error. A CI cache is an optional speedup keyed by those same identity fields, never an authority; never fall back to current `target/debug/ira` or a working-tree build. This is compatible with F-001 adding `ira-core` to current manifests: the detached baseline still uses its original standalone manifest/lock at the tagged commit.

### R4 [must] Exercise live oracle traces on Windows through a pinned PTY/ConPTY backend

Recommend Rust `portable-pty` as the one backend API for Unix PTYs and Windows' native pseudo-console implementation, pinned to an exact reviewed release in `tools/ira-parity/Cargo.lock`. Its docs describe a cross-platform PTY API and note multiple Windows implementations ([portable-pty docs](https://docs.rs/portable-pty/latest/portable_pty/)); the exact release/configuration still needs Windows CI confirmation. F-150 should run at least one real baseline startup/input/exit trace on Linux, macOS, and `windows-latest`, and should include modifier keys, resize, cleanup after timeout, and actual terminal dimensions. Do not silently make Windows schema/golden-only: ConPTY is the selected live strategy, and any inability to run it is a visible failing platform prerequisite requiring a reviewed fallback/decision. Keep schemas and recorded evidence portable across targets but do not demand byte-identical terminal rendering across OSes when host/terminal capability output differs; trace assertions should explicitly limit the comparable fields or supply per-platform expectations.

### R5 [should] Bound F-002 to infrastructure and harness contract, not parity adapters for unfinished features

F-002 should deliver scenario parsing/validation, safe fixture/environment setup, baseline PTY runner, one neutral target interface, observation/diff plumbing, candidate golden staging, and tests including a fake target that proves a mismatch fails. It should not implement a core or GPUI target adapter, migrate existing captures, normalize every TUI surface, add feature behavior, or wire F-150 CI. A small successful TUI trace plus malformed-schema, isolation, mismatch, timeout-cleanup, and golden-staging tests are adequate F-002 acceptance. Core/desktop adapters and feature-specific trace suites arrive alongside the owned behavior rows. Preserve logic-reviewer approval of every promoted golden; runner-generated candidates are not approved truth.

Cost/risk of ignoring: F-002 becomes an unbounded project-wide test framework, couples trace schema to not-yet-defined app internals, and consumes feature-wave ownership before core/action contracts exist.

## Readiness and residual risk

Red-test preparation **may start after** the manager folds R1–R4 into F-002 and the platform advisor confirms the exact PTY release can run live on the supported Windows runner. At present the draft still asks these questions and leaves package ownership unsettled, so tests would encode an undecided architecture. Once updated, infrastructure tests can begin before implementation; the review does not require a new GPUI API or `test-support` dependency in F-002. No spec, production, test, or planning files were edited in this review.

---

# F-002 final specification re-review

Scope reviewed: revised `migration/specs/F-002.md`, D-0011, matrix rows F-002/F-150, WAVE-0 and `GRAPH.md`, plus the F-002 platform-advisor review.

Verdict: **PROCEED WITH CHANGES. Adversarial red-test preparation may begin now; implementation remains gated on reviewed executable red tests and the normal feature review.** The major architecture choices from the prior review are now explicit: standalone `tools/ira-parity`, TOML-based traces, `ScenarioTarget`, `portable-pty 0.9.0` with live Unix/ConPTY runs, and baseline resolution from the verified detached tag. D-0011 records those choices. F-002's matrix and Wave 0 write-set are narrowed to tool/traces; F-150 owns CI. The baseline SHA and cache identity are sufficiently fail-closed to proceed with test-first work.

## Remaining findings

### R15 [must before approving the final F-002 spec] Close the input/observation contract holes

The document still calls event representation “a pre-review question” under Triggers, and Open Question 2 still asks whether the schema/adapter contract is suitable. S1/S3 name TOML and `ScenarioTarget`, but do not define the actual logical event variants/fields or how a target reports supported observations. D-0002 requires state/effect comparisons; S3 only lists process, terminal screen, filesystem, and persisted state, leaving future core adapters with no shared cursor/pane/selection/message observation. During red-test preparation, specify at minimum an ordered tagged event enum (`Key { code, modifiers, kind }`, `Paste { text }`, `Resize { cols, rows }`) and neutral observation selectors for process, terminal screen, core state (cwd/panes/ordered entries/cursor/selection/message), filesystem, and persisted bytes. Define the adapter contract around setup/fixture, `apply(event)`, `observe(selector)`, and bounded finish/exit; adapters must report unsupported observation kinds explicitly. Keep action IDs, GPUI views, and terminal byte encodings out of the trace schema. Then delete the contradictory open-question text. The exact spelling can be set by the red-test owner, but the spec must freeze these semantics first.

Cost/risk of ignoring: trace files and later core/GPUI adapters will invent incompatible event and observation models, or omit state needed to prove parity.

### R16 [should] Align the trace filename and complete the exact build/cache cleanup recipe

Use `migration/oracle/traces/<area>/<name>.trace` with TOML syntax so the path conforms to `.cursor/skills/parity-testing/SKILL.md` while honoring D-0011's TOML decision; the current `.toml` suffix is a small protocol mismatch. Pin one build command/profile for the baseline artifact (for example the locked root `ira` binary for the host target in dev profile) and include profile/build flags in its cache identity or declare them fixed. Record enough sidecar metadata to verify tag, commit, OS/target, toolchain, baseline lock hash, profile, and executable digest. Add explicit detached-worktree cleanup on success/failure; S8 currently cleans per-scenario fixture/PTY state but not the baseline worktree itself. Finally, add `migration/reports/F-002/dev-1.md` to the WAVE-0 F-002 write-set or remove it from the spec write-set; the two currently differ.

Cost/risk of ignoring: the same cache key could refer to binaries from different build modes, a failed run can leave a worktree behind, and the developer/reviewer scopes disagree about the evidence report.

## Test-first readiness

Adversarial **test design and red-test preparation can start now** because package ownership, PTY strategy, supported live runners, baseline source, and F-002 scope are decided. The adversarial brief should own R15 as a contract-first test-design step and submit the event/observation shape for manager acceptance before committing schema-dependent red tests. No feature implementation may start from this report; F-002 implementation follows only after those tests are in place and the feature brief/write-set is accepted. The remaining platform task is to prove the pinned dependency closure/MSVC ConPTY replay in F-150 before F-002 is marked VERIFIED, as the platform review specifies. No code, spec, matrix, decision, or wave files were changed here.

---

# F-002 final contract acceptance

Reviewed the revised `migration/specs/F-002.md` against the F-002/F-150 matrix rows, D-0011, and Wave 0 ownership. **PROCEED: adversarial red-test and schema preparation may begin.** The v1 contract now defines ordered semantic input variants and fields, explicit target lifecycle, neutral process/screen/domain/filesystem/persisted observations, and fail-fast behavior when a requested observation is unsupported. This keeps the harness independent of GPUI, terminal encoding, and F-003 action IDs while allowing later core/desktop adapters to share traces.

The additional baseline build/profile and cache-sidecar identity, detached-worktree cleanup including Windows handles, and report/test write-set alignment close the residual operational and ownership recommendations from the prior review. F-002 remains isolated to its standalone tool, traces, and report; F-150 retains CI and cross-platform dependency/replay verification.

No architecture must-fix remains before red-test/schema preparation. F-002 implementation still follows the accepted spec, executable failing checks/GAP notes, and the manager's normal review gate. The `portable-pty` dependency closure and live MSVC ConPTY replay remain F-150 evidence required before F-002 can be marked verified. This is an architecture acceptance only, not implementation or parity sign-off. No production, test, spec, matrix, decision, or wave files were changed.
