---
name: migration-orchestration
description: Master playbook for the Migration Manager running the TUI to GPUI 1:1 parity migration in goal mode. Defines phases, state files, the resume protocol, supervision rhythm, and stuck-handling. Use at the start of the migration, after any context loss, and whenever deciding what to do next.
---

# Migration orchestration playbook

Read `AGENTS.md` first. This skill expands it into concrete procedures.

## 0. Resume protocol (run this first, every time)

1. `git status`, `git log --oneline -20`, current branch.
2. Read `migration/STATE.md` (dashboard), `migration/PARITY_MATRIX.md`, last 80 lines of `migration/journal.md`.
3. Read the current `migration/waves/WAVE-*.md`.
4. Run `python scripts/parity_gate.py --surface migration/oracle/surface.txt` (fast mode, no cargo).
5. Reconcile: are there subagents you launched that you have no result for? Their work may be in the tree.
   Check `git status` and `migration/reports/`. Re-launch if work is missing, never assume.
6. Continue the loop in `AGENTS.md` section 5.

If `migration/STATE.md` does not exist, you are at Phase 0.

## 1. State files (your external memory; keep them truthful)

| File | Purpose | Writer |
|---|---|---|
| `migration/STATE.md` | One-page dashboard: phase, wave, counts by status, in-flight agents, blockers, next 5 actions | manager (rewrite each loop) |
| `migration/journal.md` | Append-only log: timestamp, action, result, commit SHA | manager |
| `migration/DECISIONS.md` | ADR log `D-0001...` with context, options, decision, sign-offs | manager |
| `migration/PARITY_MATRIX.md` | One row per feature. Machine-parsed by the gate | manager (developers propose via reports) |
| `migration/inventory/*.md` | Explorer outputs per TUI area | explorers |
| `migration/oracle/**` | Captured TUI behavior: keymap, help text, CLI help, config defaults, tmux goldens, `surface.txt` | explorers |
| `migration/specs/F-xxx.md` | Per-feature behavior spec derived from oracle | manager approves, developer or explorer drafts |
| `migration/waves/WAVE-n.md` | Wave plan: features, write-sets, deps, agents, status | manager |
| `migration/reports/**` | Reviewer, advisor, auditor reports | those agents |
| `migration/ENRICHMENTS.md` | Desktop-only enhancements backlog `E-xxx` (kept separate from parity rows) | manager + ux-advisor |

Templates are in `migration/templates/`. Copy, do not edit templates.

## 2. Phase 0: Orientation

Checklist (do it yourself, it is small):
- [ ] Identify workspace layout: crates, binaries, features, edition, MSRV, toolchain file.
- [ ] Record project coordinates in `STATE.md`: TUI crate/bin name and path, GPUI app crate/bin name and path,
      how gpui is depended on (crates.io version vs git rev), UI deps (ratatui, crossterm, ...), CI files.
- [ ] Build and run both. Capture exact commands that work: `cargo build`, `cargo run -p ...`, `cargo test`.
- [ ] Tag the oracle: `git tag tui-oracle-baseline <commit>` and push the tag. Also store a built oracle
      binary path or a `git worktree add ../tui-oracle tui-oracle-baseline` for differential runs.
- [ ] Audit the existing GPUI shell via `architecture-advisor`: what exists (window, root view, actions,
      keymap, theme, list view?), what is a stub, what pattern it uses. Output: `reports/shell-audit.md`.
- [ ] Ensure CI runs on macOS and Windows (`.github/workflows/*.yml` or the forge equivalent). If missing,
      create via `integrator`. Without CI evidence you cannot reach the gate.
- [ ] Copy templates, create `STATE.md`, `journal.md`, `DECISIONS.md`, empty `PARITY_MATRIX.md`, `surface.txt`.
- [ ] Commit and push on the working branch. All migration commits go to the integration branch unless the
      repo has other conventions; feature work happens on feature branches or worktrees (see wave-planning).

## 3. Phase 1: Deep discovery (learn before building)

Use skill `tui-feature-discovery`. Launch **parallel** `tui-discovery-explorer` agents, one per slice
(slice list is in that skill). Also launch in parallel:
- `architecture-advisor`: shell audit plus a `reports/gpui-notes.md` listing the pinned gpui version, the
  idioms that version uses, test support, and component-library options (with Context7 lookups).
- `platform-advisor`: `reports/platform-risks.md` for the features discovered so far (re-run when inventory grows).

You are done with discovery when:
- Every directory and module of the TUI source is claimed by an inventory file (no orphan files).
- `surface.txt` lists every key binding, command, CLI flag, env var, config key, on-disk file, and user-facing
  message class, one per line, as `kind:name`.
- Two independent explorers disagree on nothing material (run a cross-check on the 3 biggest areas).
- The live TUI was driven for every area and goldens are stored in `migration/oracle/`.

## 4. Phase 2: Analysis

Use skills `parity-matrix` and `wave-planning`.
1. Convert inventories into matrix rows with IDs `F-001...`. One row = one independently verifiable behavior
   group. If a row cannot be verified by a handful of tests, split it.
2. Compute common ground, the DAG, and waves. Write `waves/WAVE-0.md` ... `WAVE-n.md`.
3. Send the plan to all three advisors **in parallel**; reconcile in `DECISIONS.md`.
4. Draft specs for Wave 0 and Wave 1 features (explorers can draft; you approve). Later waves are specced
   just-in-time, but no developer starts without a spec.

## 5. Phase 3 and 4: building

For each wave, follow the **feature gauntlet**:

```
G1 SPEC       specs/F-xxx.md exists and cites oracle evidence (source refs + captured behavior)
G2 PRE-REVIEW architecture/ux/platform advisors review design notes of the spec (parallel, short)
G3 RED TESTS  adversarial-reviewer writes failing tests + GAP notes from the spec BEFORE implementation
G4 IMPLEMENT  gpui-developer, test-first, inside its write-set. Returns report + list of GAPs it believes fixed
G5 REVIEW     logic-reviewer and adversarial-reviewer in parallel on the finished diff
G6 FIX        developer closes GAPs (flips GAP -> GAP-FIXED), never deletes a note
G7 VERIFY     the reviewer who raised each GAP verifies and flips GAP-FIXED -> GAP-RESOLVED, or reopens it
G8 MERGE      integrator merges; manager runs gate; matrix row -> VERIFIED with evidence column filled
```

Supervision rhythm (reviewers and advisors supervise while developers work, not only after):
- Launch developers with `run_in_background: true`. In the same message, launch the red-test adversarial
  reviewers for the **next** features in the pipeline.
- As soon as any developer returns, immediately launch its reviewers. Do not wait for the whole wave.
- At wave end: one **wave sweep**: adversarial-reviewer on cross-feature interactions (shared state, key
  binding collisions, focus, modal stacking, concurrent jobs) of everything merged in the wave.
- Review cycle cap: 3 per feature, then escalate (see `AGENTS.md` section 8).

Concurrency cap: default 5 developers + 3 reviewers in flight. Lower it if builds thrash (shared `target/`
dir contention: give each worktree its own `CARGO_TARGET_DIR` or use `sccache`).

## 6. Phase 5: Integration hardening

- Platform matrix: `platform-advisor` reviews every platform-conditional code path; CI results on both OSes read
  by you, not assumed.
- Performance: directories with 100k+ entries, deep trees, slow disks (simulate), rapid key repeat. Budgets are set
  from the TUI's own responsiveness measured in Phase 1 (record in `oracle/perf.md`).
- UX floor (skill `desktop-ux-enrichment`): required before the gate.
- Enrichment wave: from `ENRICHMENTS.md`, only for areas whose parity rows are all verified; each enrichment gets
  its own review but cannot alter parity behavior (reviewers verify the parity tests still pass).

## 7. Phase 6: Completion

Run skill `completion-gate`. Until it passes you are in Phase 5 or earlier.

## 8. Handling common situations

| Situation | Action |
|---|---|
| Subagent returns "done" with no tests | Reject. Re-launch with brief tightened: tests first, show failing then passing output |
| Subagent edits outside write-set | `git checkout` those paths, log it, relaunch with explicit write-set |
| Two features need the same shared file | Integrator pre-wires a slot, or serialize them. Never let both edit it |
| Build broken on integration branch | Stop launching new work. Integrator + the last merging developer fix first |
| New TUI behavior discovered mid-migration | Add matrix row immediately, mark NOT_STARTED, assign a wave. Discovery never closes completely |
| Reviewer and developer disagree | Evidence wins: run the TUI oracle, compare. If unclear, logic-reviewer rules with a trace |
| Advisors contradict each other | Record both in `DECISIONS.md`; prefer parity-preserving option; ask architecture-advisor to arbitrate |
| Context nearly full | Update `STATE.md` and `journal.md`, commit, push, and continue; files carry you through compaction |
| Flaky test | Treat as a bug in the test or code; quarantine is not allowed. Fix root cause |
| GPUI missing a widget (text input, menu, dialog) | architecture-advisor evaluates build vs adopt a component crate (license, maintenance, pinned gpui compatibility). Decision record required |
| You think you are done | You are not. Run the gate script and blind audits |

## 9. Commit and branch hygiene

- Small logical commits. Prefix: `foundation:`, `feat(F-017):`, `review(F-017):`, `fix(F-017):`, `plan:`, `state:`.
- Push after every merge and every state update. Never force push. Never rewrite published history.
- One feature per branch: `migrate/F-017-rename`. Integrator merges into the integration branch.
