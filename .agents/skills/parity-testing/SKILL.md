---
name: parity-testing
description: Test strategy and conventions for proving 1:1 parity. Characterization tests against the TUI oracle, differential traces, fixtures, test file placement, and the GAP note convention that reviewers use to record issues and coverage gaps directly in test files. Use for developers, reviewers, and the manager.
---

# Parity testing and the GAP note convention

## 1. Test layers

1. **Characterization tests** (written first, against the TUI baseline): pin the oracle's behavior before any refactor.
   Location: core crate `tests/characterization/*.rs`. They run against the baseline code or against captured goldens.
2. **Core behavior tests**: new core logic equals oracle behavior for every spec clause and edge case.
3. **Differential traces**: the same input trace is replayed against the oracle and the new app; observable state is
   compared. Trace files: `migration/oracle/traces/<area>/<name>.trace` (format defined in the parity crate).
4. **GPUI integration tests**: key bindings in contexts, focus, overlays, scrolling, action dispatch.
5. **Filesystem effect tests**: real temp directories (never the real home), per-test isolation, cleanup.
6. **Persistence round trips**: files written by the TUI are read by the new app and vice versa. Fixtures in
   `migration/oracle/fixtures/persisted/`.
7. **Platform tests**: Windows and macOS specific behavior behind `cfg` with CI evidence.
8. **Performance guards**: coarse budgets that fail on regressions (generous thresholds, deterministic fixtures).

Every spec clause maps to at least one test. Every matrix row's Evidence column names the tests.

## 2. File placement (so reviewers never touch production code)

- Integration tests: `crates/<crate>/tests/*.rs` (feature-scoped files: `tests/f017_rename.rs`).
- Unit tests: sibling file `foo/tests.rs` declared as `#[cfg(test)] mod tests;` in `foo/mod.rs`. Do not write inline
  `mod tests { ... }` bodies in production files.
- Shared helpers: `crates/<crate>/tests/common/mod.rs` or the parity crate.
- Reviewers may write only: `**/tests/**`, `**/tests.rs`, `**/*_tests.rs`, `migration/reports/**`.
  Check with `python scripts/scope_check.py reviewer`.

## 3. GAP notes: the issue and gap ledger inside test files

Reviewers (and developers who discover a gap) record every issue in the relevant test file using this exact,
grep-able format. The gate script reads it.

```rust
// GAP(G-F017-ADV-01) sev=high kind=behavior-divergence feature=F-017
//   what:     Renaming onto an existing name silently overwrites the target.
//   tui-ref:  src/ops/rename.rs:118 (fn rename_entry returns Err(Exists))
//   oracle:   migration/oracle/captures/fileops/rename_collision.txt
//   repro:    fixture a.txt + b.txt; rename a.txt -> b.txt
//   expected: error "Destination already exists", both files unchanged
//   actual:   b.txt replaced
//   cover:    gap_f017_adv_01_rename_collision_must_not_overwrite
#[test]
#[ignore = "GAP G-F017-ADV-01"]
fn gap_f017_adv_01_rename_collision_must_not_overwrite() {
    // real assertions whenever possible
}
```

Fields: `sev` in `blocker|high|medium|low`. `kind` in `behavior-divergence|missing-feature|edge-case|race|platform|
perf|ux-regression|test-gap|spec-gap|data-compat|security`.

### IDs (no central counter, so parallel reviewers never collide)

`G-<FEATURE>-<ROLE>-<NN>` where FEATURE is `F017` or `FND` for foundation, ROLE is `ADV` (adversarial), `LOG`
(logic), `DEV` (developer-found), `AUD` (audit), `SWP` (wave sweep), and NN is a two-digit counter per
(feature, role) that you increment yourself by grepping the tree.

### Lifecycle

| Marker | Meaning | Who sets it |
|---|---|---|
| `GAP(id)` | open issue or uncovered gap | reviewer/auditor/developer who found it |
| `GAP-FIXED(id)` | developer claims fixed; test passes; awaiting verification | developer only |
| `GAP-RESOLVED(id)` | reviewer verified the fix by running the test and re-attacking | the reviewer who raised it (or another reviewer of the same role) |
| `GAP-ACCEPTED(id) decision=D-xxxx` | explicitly accepted deviation (severity low only, or with full sign-off) | manager, with decision record |

Rules:
- **Never delete a note.** History is the point. Resolved notes stay.
- When fixed, the developer removes the `#[ignore = "GAP ..."]` attribute (the test must now run and pass),
  rewrites the header marker to `GAP-FIXED(id)` and appends `fixed-by: <commit sha or description>`.
- The verifying reviewer changes the marker to `GAP-RESOLVED(id)` and adds `verified-by: <role> <date>`.
- If the fix is inadequate, the reviewer reverts the marker to `GAP(id)` and adds `reopened: <reason>`.
- A test-gap note without a test is allowed only when the test needs infrastructure that does not exist yet; then
  `cover:` names the missing infrastructure and the note stays open until it is built and the test written.
- The gate fails on: any `GAP(`, any `GAP-FIXED(`, and any `#[ignore` whose reason does not start with `env:`
  (use `#[ignore = "env: needs display"]` for legitimately environment-bound tests).

### Prefer executable gaps

Order of preference for a finding:
1. A **failing test** that asserts the correct behavior (add `#[ignore = "GAP id"]` only if keeping CI green on the
   shared branch matters; on a feature branch let it fail).
2. A test skeleton with exact setup and assertions described in comments.
3. A comment-only GAP note with repro and expected behavior.

### Non-Rust artifacts

Snapshot or trace files can carry the marker in a header comment line starting with `#` using the same format.

## 4. Fixtures

- `migration/oracle/fixtures/make_fixture.py` builds a deterministic tree: nested dirs, unicode names (NFC and NFD),
  spaces, leading dots, leading dashes, very long names, symlinks (file, dir, broken, loop), empty files, large files,
  read-only items, hard links, zero-byte dirs, case-colliding names where the FS allows. Windows-safe variant guards
  unsupported items and records what was skipped.
- Tests copy the fixture to a temp dir per test. Tests never rely on global state or execution order.
- Timestamps are set explicitly when sort order depends on them.

## 5. Differential trace format (minimal contract; the parity crate may extend it)

```
# F-017 rename collision
fixture: basic
size: 120x40
keys: r | a b c <Enter>
expect.cwd: .
expect.cursor: b.txt
expect.message: Destination already exists
expect.fs: unchanged
```
The oracle runner executes keys in the TUI through the pty harness and fills `expect.*` from observation (golden mode);
the new app is checked against those goldens. Goldens are reviewed by humans-in-the-loop equivalents: the logic-reviewer
inspects every new golden for plausibility before it is committed.

## 6. Quality rules for tests

- A test must be able to fail: show the red run in the developer report for new behavior.
- No `sleep`-based sync; use channels, polling with timeouts, or deterministic executors.
- No assertions on incidental formatting unless the oracle defines it exactly.
- Each test name states the behavior. Avoid `test1`.
- Flaky tests are bugs; quarantine is forbidden.
- Mutation-style sanity check (adversarial-reviewer): flip a condition in the feature code locally (not committed) and
  confirm a test fails. Record "mutation checks performed" in the report.
