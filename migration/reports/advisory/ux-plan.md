# Advisory: UX plan review by ux-advisor round 2

Scope reviewed: `migration/PARITY_MATRIX.md` (F-001..F-113), `migration/waves/` (no wave plan yet), `migration/reports/advisory/ux-keymodel.md`, `ux-layout.md`, `ux-enrichment-plan.md`, `migration/ENRICHMENTS.md`, UX-relevant inventories (`entry-input`, `rendering-messages`, `platform-helpers`, `preview-external`, `config-state`, `state-search`, `files-ops`, `filesystem`), and `desktop-ux-enrichment`, `parity-matrix`, and `wave-planning` skills.

Summary verdict: RETHINK before implementation. The matrix is a useful discovery index, but it is not yet a behavior-sized parity ledger or an executable wave plan. Current broad rows and arbitrary surface assignments obscure required desktop UX work and cannot support the required reviews/evidence.

## Recommendations (ranked)

R1 [must] Split broad behavior rows before specs or development. The row rule is roughly 3–15 tests and explicitly separates distinct code paths, errors, modes, and platform behavior (`.cursor/skills/parity-matrix/SKILL.md`, Row granularity). F-032 combines dozens of normal keys, search, paste, and ignored events; F-033 combines navigation, open-default, prompts, and bookmark/common-folder shortcuts; F-034–043 likewise mix unrelated actions and contexts. Split at least by mode/context and behavior families, retaining distinct rows for open-vs-navigate, help dismissal, each overlay's close/focus semantics, copy-board actions versus board-focus handling, and editor save/exit. Conversely, do not use a whole inventory entry as a single row if its own `INV-` describes multiple independent behaviors. Cost of ignoring: a review can miss regressions while a mixed row appears green.

R2 [must] Add explicit UX-floor rows in area `ux-floor`, as required by `desktop-ux-enrichment` §2. None of F-001..F-113 clearly owns click/double-click/wheel/modified-click/context-menu convergence; native window lifecycle and geometry persistence; system light/dark, high contrast, HiDPI and font fallback; IME/clipboard/selection/undo; accessibility focus traversal and icon labels; or empty/loading/error/permission/offline states. Add behavior-sized rows with oracle-related rows in Deps and evidence requirements per platform. Include minimum/responsive window behavior as a dedicated row (mandatory parity-matrix row), and async responsiveness/progress/cancel rows linked to F-056 and filesystem/preview work. Do not put these as optional E-xxx enrichments: the skill makes them completion requirements.

R3 [must] Replace speculative surface mappings with semantic ownership. Several assignments contradict the referenced feature or the inventory: F-027 (`--version`) owns `key:normal:plus`, goto mode and unrelated open-detached exit; F-028 terminal diagnostic owns environment probes rather than its CLI/exit surface; F-031 owns only `cli:--check-terminal` while describing unknown args/no-TTY; F-033 includes rename prompt keys; F-034 includes create-entry and search keys plus copy-board; F-035 combines confirmation, creation, rename and eject; F-055 permanent-delete confirmation gets goto-path backspace; F-075 text preview owns dirty-indicator; F-076 editor entry owns dirty-indicator; F-078 video owns generic preview decode-error; F-081 exclusions owns preview-off; F-083 open-default owns unrelated preview UI; F-092 drag/drop absence owns ignored terminal mouse. Reassign every item by tracing its exact inventory/source reference; create an explicit `surface -> row` audit (allowing an item in multiple rows only when truly cross-cutting). The gate's simple coverage check proves presence only, not correct ownership.

R4 [must] Make key parity verifiable at context granularity. F-032's monolithic list cannot prove TUI dispatch priority or prevent OS shortcuts from stealing IRA bindings. Preserve the exact ordered policy in `ux-keymodel.md`: preview editor, global Ctrl handling, prompt modes, transient dialogs, search, Copy Board focus, normal pane. Split key/action rows by dispatch context and add a matrix-visible OS alias row/table for Cmd+Q/Alt+F4 and any chosen F2/Delete aliases. Record conflicts and explicit non-bindings: Ctrl+C remains IRA quit; `c` is copy-to-other-pane; `]` copies current folder path; do not map Cmd+C to file clipboard, Cmd+V to file paste, Ctrl+W/Cmd+W to tab/window close, or introduce count/multikey behavior. Required keyboard-only routes must accompany mouse/menu rows; menus must invoke the same action IDs and show IRA key plus additive alias.

R5 [must] Build dependencies and wave barriers before launching developers. Every current row has `Deps: -` and `Wave: 1` except F-111..113 at wave 5; `migration/waves/` contains only `.gitkeep`. This contradicts common-ground-first planning. Use the current UX reports to require foundational action/keymap dispatcher, focus/overlay host, state/pane/list, text input, theme/status/feedback, async job/progress services, and OS adapters before dependent UI features. Give each row explicit dependencies and non-overlapping write sets in WAVE-0/1 plans. Keep TUI-like layout and defaults as the parity profile; do not enable a default sidebar, tabs, or altered split state. No feature work should start until each wave has batch conflict analysis, reviewer/advisor assignments and exit criteria.

R6 [should] Separate parity from enrichment and encode gating. `migration/ENRICHMENTS.md` contains proposed E-001..E-011 but placeholder `F-0xx` dependencies; matrix rows currently mix absence statements such as open-with/drag-and-drop absence into `preview` parity. Preserve absence as a source-verified contract only where the absence itself guards against inventing a TUI command, while put desktop-only menus, breadcrumbs, palettes, toolbars, richer preview controls, DnD and other extras in the enrichment ledger. Resolve every E dependency to real row IDs and keep it blocked until all related parity rows are verified. Include tests that parity behavior stays unchanged with enrichment paths available.

R7 [should] Add concrete evidence/acceptance notes to UX-floor rows. Mouse needs action-convergence tests for click, double click, wheel and modified selection; native runners must capture window, theme, dialogs, accessibility tree, IME/clipboard behavior; keyboard routing needs per-context simulated-key tests plus focus restoration; async UI needs non-blocking/progress/cancel/hide semantics; empty/error states need exact messages and zero-match behavior. `ux-layout.md` notes Linux captures do not establish macOS/Windows UX; keep platform artifacts as evidence gates.

## Risks not covered

- The matrix's `Area` values `config`/`state` hide distinct responsibilities, making it harder to assign relevant UX reviewers and desktop integration ownership.
- F-111..F-113 contract rows at wave 5 do not replace mandatory CLI, persisted-file, help, startup/shutdown, errors, and performance rows; some are currently represented only by broad or unrelated rows.
- Inventory confidence is uneven; rows must retain source/runtime evidence and not infer desktop outcomes from terminal-only rendering.

## Questions for the manager

- None; the plan can be corrected from the current inventories and source references.

# Advisory: UX plan follow-up by ux-advisor round 2

Scope reviewed: revised `migration/PARITY_MATRIX.md` (F-001..F-133), `migration/surface-ownership.tsv`, `migration/DECISIONS.md` (D-0001..D-0008), `migration/waves/GRAPH.md`, `WAVE-0.md`..`WAVE-3.md`, `migration/ENRICHMENTS.md`, and round-one UX reports.

Summary verdict: **STOP — F-001 may not start yet.** The revision materially improves the plan: it adds dedicated UX-floor rows, adds concrete ownership and dependency data, and splits several formerly broad input/dialog rows. However, a remaining row-granularity violation and unresolved enrichment placeholder remain, and D-0008 plus WAVE-0 entry criteria explicitly require plan review and specs/red tests before implementation. F-001 has no spec or red tests (`migration/specs/` is empty except `.gitkeep`).

## Residual recommendations

R1 [must] Split F-124 before approving implementation. Its single row covers bookmark navigation/management, theme cycle, hidden-file toggle, sort, split toggle, and pane switch. These are separate actions/state changes and separate verification groups under the matrix rule to split distinct code paths. Assign separate rows to the actions, keeping `key:normal:*` ownership with the behavior it invokes. Recheck related broad rows such as F-033 (navigation plus bookmark/common-folder destinations) and F-128 (selection toggle plus select-all/invert aliases) in the same granularity pass. The ownership TSV is structurally consistent today (I checked all 418 surface items: every TSV owner exists and its row contains the item), but that check only verifies declared ownership; it does not make a multi-action row behavior-sized.

R2 [must] Complete the enrichment dependency gate before Wave 0 approval. `migration/ENRICHMENTS.md` still has only E-001 with `Parity rows it could affect = F-0xx`. Replace the placeholder with concrete IDs from navigation/path/pane/list rows and document that E-001 cannot start until all those rows are verified. Confirm every planned enrichment has a concrete row list and that no enrichment work enters WAVE-0..3. D-0007 already adopts this requirement, so the artifact needs to match the decision.

R3 [must] Add F-001's implementation spec and red tests, then satisfy WAVE-0's own entry condition. Current WAVE-0 entry criteria require the Phase 2 review to be complete and foundation specs/red tests approved; D-0008 blocks all feature briefs until the matrix, ownership, UX floor and advisor review are reconciled. The matrix lists F-001 as NOT_STARTED with no owner/evidence; `migration/specs/` contains no F-001 spec or tests. Define the crate boundary, preserved root TUI/oracle build path, desktop launch/lifecycle contract, and smoke/contract tests first. In particular, reserve desktop window lifecycle/reopen behavior for F-115/F-011 without allowing F-001's broad `desktop/**` write-set to implement or regress it implicitly. Then obtain the required plan-review reconciliation before starting the barrier.

R4 [should] Make UX-floor acceptance explicit in the assigned specs and wave exits. F-117's label does not enumerate OS light/dark, contrast, HiDPI and font fallback; F-114 should enumerate click, double-click, wheel, modified multi-select and context action convergence; F-118 should name IME, clipboard, selection and undo; F-119 should name visible focus, dialog traversal, contrast and icon labels; F-121 should state exact parity behavior for empty/no-match, loading, errors, permission denial and removed/offline volumes. Wave exit must require native macOS and Windows evidence for platform-dependent rows, not just CI compilation. These rows are present, but their current names and generic `desktop UX floor` references do not constitute acceptance criteria.

R5 [should] Narrow wave ownership around shared platform files. WAVE-2 Batch B (operations) and Batch C (platform) both include `crates/core/src/platform/**` and `desktop/src/platform/**`; the prose says sequential unless contracts are frozen, but the table assigns both to the same wave without an explicit serial batch relationship. WAVE-3 Batch B also overlaps those globs. Make this a named barrier/serial order or split the write-set by service module before scheduling. This matters to F-120/F-122 and cannot be resolved by surface ownership alone.

## Checks that passed

- The revised ledger now has separate UX-floor rows F-114..F-123, and dialog dismissal/progress/selection paths were split into F-130..F-132.
- The keyboard ledger now gives distinct rows for search activation, Copy Board activation, delete, selection aliases, eject, errors, progress, and multi-selection info. Keep the exact dispatch precedence and do not let native accelerators preempt IRA bindings as D-0006 records.
- All 418 surface lines are present in the ownership TSV, and every declared owner row contains that item. Continue to review semantic attribution where related input/rendering surfaces are reused.
- WAVE-0 identifies common-ground barriers and WAVE-1..3 defer UX-floor and preview work until their contracts; this is directionally sound once conflicts and entry criteria above are resolved.

## F-001 start decision

**Do not start F-001 now.** It is the Wave-0 barrier, but its entry criteria are not met: plan review is still open, F-124 remains oversized, enrichment dependencies still contain a placeholder, and no foundation spec/red tests exist. Once R1–R3 are addressed and the Phase 2 review is reconciled in `DECISIONS.md`, F-001 may start as the serial boundary/bootstrap task, with its write-set constrained to the approved spec and the WAVE-0 integration contracts.

# Advisory: UX plan final follow-up by ux-advisor round 3

Scope reviewed: latest `migration/PARITY_MATRIX.md` (F-001..F-150), `migration/surface-ownership.tsv`, `migration/DECISIONS.md`, `migration/ENRICHMENTS.md`, `migration/waves/GRAPH.md`, and WAVE-0..3.

Verdict: **PROCEED WITH CONDITIONS from the UX perspective.** The requested plan changes address the prior UX blockers: F-124 is now one action; F-134..F-144 provide individual action rows; copy and move are distinct; editor save is separate; F-149 tracks native quit aliases; UX-floor labels now call out concrete behaviors; E-001..E-011 list concrete parity dependencies. I verified mechanically that all 418 oracle surface items have an ownership row and the declared owner row contains each item, every one of the 150 matrix rows appears in a wave, no unknown wave row IDs appear, and the enrichment table has no `F-0xx`/TBD placeholder.

## Residual UX recommendations

R12 [must] Split F-034 into separate quit and selected-entry-info rows before implementation specs are approved. Its title combines `q`/Ctrl+C quit with `?` info, which are different actions, effects, and verification traces. This is the remaining clear behavior-row granularity issue visible in the input matrix and follows the parity-matrix rule to split genuinely different key paths. Update both ownership entries for `key:normal:q`, `key:global:ctrl-c`, `key:normal:?`/`question-mark` and any dependent E-rows/F-149 dependency to the resulting row IDs.

R13 [should] In F-032's spec, explicitly scope `ui:event-mouse-ignored` to TUI event characterization; the GPUI mouse behaviors are the additive F-114 contract. Do not interpret this surface item as requiring the desktop app to ignore mouse input. Test F-114's listed mouse gestures through the same action IDs and compare resulting state to keyboard traces.

R14 [should] Specify native quit alias routing in F-149: Cmd+Q (macOS) and Alt+F4 (Windows) must reach the same application quit lifecycle as IRA `q`, while Ctrl+C keeps its TUI semantics and no new unsaved-edit prompt is introduced without a decision. Keep the menu/shortcut discoverable but ensure native menu accelerators do not preempt mode-owned keys.

The UX-floor coverage is now adequate at matrix level: mouse (F-114), lifecycle/reopen (F-115), geometry (F-116), theme/display (F-117), native text/IME (F-118), accessibility (F-119), chooser and OS integration (F-120/F-122), feedback (F-121), keyboard-only/menu convergence (F-123). WAVE-2 now records native macOS/Windows evidence and concrete acceptance dimensions. Specs must retain those checks and distinguish headless tests from OS-native evidence.

## F-001 preparation decision

**UX review permits F-001 spec and red-test preparation, but does not clear the overall Phase 2 gate or authorize production implementation.** Keep that preparation limited to a proposed boundary contract and checks; do not alter code/manifests until WAVE-0 entry criteria are formally cleared. The current `WAVE-0.md` explicitly gates even spec drafting on “Phase 2 plan review cleared,” and the parallel architecture/platform follow-up reports still identify Wave-0 dependency/batch contradictions and unresolved terminal-only rows. The manager must reconcile those cross-advisor blockers and record the responses in `DECISIONS.md` before opening F-001 implementation. Once that overall gate is cleared, F-001 is a suitable serial first barrier with the UX constraints from D-0006 and F-115 lifecycle ownership preserved.

# Advisory: F-001 spec pre-review by ux-advisor

Scope reviewed: `migration/specs/F-001.md` only.

Verdict: **APPROVE.** The spec keeps F-001 to the package boundary/build contract, explicitly preserves the existing GPUI window and button interaction (S2 and “GPUI/desktop surface”), and says lifecycle remains with its owning row and that F-001 does not claim F-115 lifecycle parity (S6 and the GPUI surface section). Its non-goals exclude lifecycle redesign and UX features; the only desktop wiring allowed is the minimum needed for the boundary. This protects the current startup/reopen path without pulling F-115 implementation into F-001.

No UX changes are requested. The spec correctly leaves executable red boundary checks as a prerequisite for implementation; their absence does not block this UX pre-review.
