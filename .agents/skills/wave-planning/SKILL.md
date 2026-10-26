---
name: wave-planning
description: Algorithm for identifying common-ground (foundation) features to build first and for detecting which remaining features can be built in parallel without conflicts. Covers dependency DAG, fan-in scoring, write-set disjointness, pre-wired slots, contract-first stubs, and wave plan format. Use during analysis and whenever a new wave is scheduled.
---

# Wave planning: common ground first, then maximum safe parallelism

## 1. Inputs

- `PARITY_MATRIX.md` rows (with Deps filled by best knowledge)
- Inventories, specs, architecture-advisor notes on the GPUI shell
- Current source layout of the GPUI app and core crate

## 2. Step A: Build the dependency graph

For each row ask four questions; each "yes" is an edge `row -> prerequisite`:
1. **API dependency**: does it call a type, trait, action, or service another feature defines?
2. **State dependency**: does it read or write a state field another feature owns (selection, cursor, cwd, clipboard,
   job queue, tab list)?
3. **UI composition**: does it render inside or attach to a view another feature provides (file list, modal host,
   status bar, preview pane)?
4. **Behavioral dependency**: does its correct behavior assume another feature's semantics (paste needs yank; undo needs
   every mutating operation to log)?

Record edges in the Deps column and in `waves/GRAPH.md` (list or mermaid).

## 3. Step B: Identify common ground (build first)

Compute for each node: `fan_in` = number of rows that depend on it directly or transitively.
**Common ground** = any row where `fan_in >= 3` OR it is on a path to more than a third of all rows OR it is a
cross-cutting capability. In a TUI file manager this is nearly always:

| Foundation piece | Why it is common ground |
|---|---|
| Workspace split: core crate (UI-agnostic) + GPUI app crate + oracle tooling | everything imports it |
| Parity harness: scenario runner that feeds the same action trace to the core and compares with the oracle | every feature's tests use it |
| Action registry + keymap engine (names, key contexts, multi-key sequences, counts, modes) | every feature is an action |
| App state model (tabs/panes, cwd, entries, cursor, selection, sort, filter, history) | everything reads it |
| Filesystem service (listing, metadata, watcher, error taxonomy, platform differences) | listing and all operations |
| Async job runner (background tasks, progress, cancel, results to UI thread) | all long operations |
| Config + persisted state loader (formats byte-compatible) | many features read config |
| Theme and design tokens | all views |
| File list view (virtualized, keyboard and mouse hooks, focus) | most UI hangs here |
| Overlay system: modal host, prompt/input widget, confirmation dialog, toast/status line, command palette | rename, delete, search, errors |
| Text input widget (GPUI has none built in; decide build vs adopt) | prompts, search, rename, command mode |
| Error and message model | every failure path |
| Pre-wired slots for all planned features (see section 5) | keeps parallel developers conflict-free |

The manager still derives the real list from the matrix; the table is a prior, not a substitute.

Foundation ordering: sort by dependencies; foundation pieces with disjoint write-sets can also run in parallel.
Typical split: F-0xx workspace and harness first (serial, integrator), then in parallel: state model, fs service,
job runner, theme, keymap engine, text input. Then file list view and overlays.

## 4. Step C: Layer the remaining rows

Topologically sort the non-foundation rows into layers `L1, L2, ...` where a row's layer is 1 + max layer of its deps
(foundation counts as layer 0).

## 5. Step D: Detect parallelism inside a layer

Two rows can run concurrently **only if** all hold:

1. **Disjoint write-sets**: the files and modules each will create or modify do not intersect. Declare write-sets as
   globs in the wave plan (`crates/app/src/features/rename/**`, `crates/core/src/ops/rename.rs`).
2. **No shared mutable state edit**: neither changes the schema of a state struct the other also changes. If one
   needs a new field in shared state, schedule a tiny contract change first (see below).
3. **No key binding collision**: their key bindings in the same context do not overlap, or the overlap is already
   declared in a keymap table owned by the integrator.
4. **No semantic coupling**: neither needs the other's runtime behavior to pass its own tests. If it does, add a
   dependency edge instead.
5. **Test fixture independence**: tests do not mutate the same global fixtures (use per-test temp dirs).

Compute a **conflict graph** over the layer's rows (edge when any rule fails) and choose batches by greedy graph
coloring: each color class is a batch that can run in parallel. Order batches by (fan_in desc, risk desc).
Cap concurrency at 5 developers by default.

### Techniques to turn conflicts into parallelism

- **Pre-wired slots**: in Wave 0 the integrator creates, for EVERY planned feature, a module file with a stub
  `register(cx)` and a line in the central registry (`features/mod.rs`, `actions.rs`, keymap table, `Cargo.toml`
  deps that will be needed). Developers then touch only their own module and never the central files.
- **Contract-first stubs**: when feature B needs a type or action from A, the integrator lands a minimal contract
  (trait/struct/action names, no behavior) first. A and B then proceed in parallel against the contract.
- **State by composition**: new per-feature state lives in its own struct held by the app state, instead of
  adding fields to a shared struct.
- **Keymap table ownership**: key bindings are declared in the feature module but collected by a registry that
  detects duplicates at test time (a foundation test fails on any collision).
- **Worktrees**: each developer works in its own git worktree and branch with its own `CARGO_TARGET_DIR`.
  The integrator merges serially.

### Rows that are never parallel with others in the same area

Cross-cutting rewrites: key engine changes, state schema migrations, theme token renames, persisted-format changes.
Run them alone in a barrier step.

## 6. Wave plan format (`migration/waves/WAVE-n.md`)

```markdown
# Wave n: <theme>
Status: planned | running | sweeping | done
Entry criteria: <rows VERIFIED or merged>
Exit criteria: all rows VERIFIED, wave sweep clean, CI green macOS+Windows

| Row | Batch | Developer branch | Write-set | Reads | Reviewers | Advisors | Status |
|---|---|---|---|---|---|---|---|
| F-017 | A | migrate/F-017-rename | crates/app/src/features/rename/**, crates/core/src/ops/rename.rs | state, fs service | adv, logic | arch, platform | IN_DEV |

Batches run in parallel within the batch; batch B starts when A frees capacity (rolling), not after A completes.
Conflicts considered: <pairs and how resolved>
Barrier steps: <any serial work>
```

## 7. Re-planning

Re-run this analysis when: a new row is added, a write-set was violated, a merge conflict occurred, an advisor
changes an architecture decision, or two consecutive features in a batch took 3 review cycles (the plan is probably
too coupled). Record the re-plan in `journal.md`.

## 8. Quality checks on the plan

- Every row is in exactly one wave.
- Every foundation piece has an owner and write-set.
- No wave-n row depends on a wave-(n+1) row.
- Every parallel batch passes the five rules above; record the evidence in the conflicts line.
- Advisors reviewed the plan (reports exist) and decisions are logged.
