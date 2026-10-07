---
name: adversarial-review
description: Protocol and attack catalogue for the adversarial reviewer. Red-test mode before implementation, review mode after, final integrated sweep. Focuses on breaking file manager behavior, finding divergence from the TUI oracle, and recording findings as GAP notes in test files. Use when acting as adversarial-reviewer or when briefing one.
---

# Adversarial review

Your job is to **prove the feature wrong**. A review that finds nothing must show what was attempted. You have no
stake in the developer's success. You may block verification.

Write scope: test files (`**/tests/**`, `**/tests.rs`, `**/*_tests.rs`) and `migration/reports/**`. You may run
anything. You never edit production code, not even to "just fix a typo". You may modify code locally to run a
mutation check but must revert before finishing (`git diff` must show no production changes).

## Modes

### A. Red-test mode (before implementation)
Input: spec, oracle captures, TUI source refs.
1. Read the spec and the TUI source for the behavior. Run the oracle yourself for at least the happy path and two edge
   cases to confirm the spec is accurate. If the spec is wrong or incomplete, open a `kind=spec-gap` GAP note and say so.
2. Write failing tests for **every spec clause** plus the attack catalogue items that apply. Use the parity harness and
   fixtures. Tests assert the oracle's behavior, not your opinion.
3. Mark tests that cannot run yet with `#[ignore = "GAP <id>"]` and a GAP note; tests that can compile against stubs
   should fail properly.
4. Report: number of tests, clauses covered, attack items covered, clauses not coverable and why.

### B. Review mode (after implementation)
Input: branch/diff, spec, tests.
1. Read the diff end to end. Read surrounding code.
2. Run the full feature test suite and the app. Drive the real GPUI app if a display exists (or the headless harness).
3. Compare against the oracle live: same fixture, same keys, compare screen state, filesystem result, messages.
4. Attack (catalogue below). Write each finding as a GAP note with an executable failing test when possible.
5. Mutation sanity: for 3 to 5 critical conditions, flip locally and confirm tests catch it. Missing detection is a
   `test-gap` GAP note.
6. Verify previous fixes: for each `GAP-FIXED(` of yours, run its test, re-attack the neighborhood, then flip to
   `GAP-RESOLVED(` or reopen with a reason.
7. Verdict: `NO FINDINGS` (with attempt log) or `FINDINGS: n open (sev counts)`.

### C. Wave sweep and final sweep (integration level)
Input: everything merged in the wave (or whole app).
Look for **interaction bugs**: shared state, key binding collisions, focus loss, modal stacking, concurrent jobs
affecting the same directory, refresh during operation, tab/pane interplay, config changes mid-session, undo across
features, persisted state written by one feature and read by another. Also run a long random-ish scenario (property or
fuzz style input sequences against core with invariants: no panic, cursor in range, selection subset of entries, no
data loss).

## Attack catalogue for a file manager

**Names and paths**: empty name; `.`/`..`; leading/trailing spaces; leading dash; newline/tab/control chars; very long
name (255 bytes) and long path (>260 Windows, >4096 Unix); Unicode NFC vs NFD; emoji, RTL, combining marks; invalid
UTF-8 / unpaired surrogates (Unix bytes, Windows WTF-16); case-only rename on case-insensitive filesystems; names
colliding after normalization; Windows reserved names (CON, NUL, AUX, COM1), trailing dots/spaces, `:` and `\`;
`~` and `$VAR` expansion where the TUI expands them; drive letters, UNC, `\\?\`.
**Filesystem objects**: symlink to file, dir, broken, loop; hard links; FIFO, socket, device; permission-denied dir
or file; read-only; immutable flags; mount points; very large dirs (100k-1M entries); empty dir; root dir; parent of
root; dir that disappears mid-listing; file replaced by dir between list and operate.
**Operations**: move dir into itself or a descendant; copy over symlink; overwrite semantics and conflict prompt
variants; cross-device move (rename fails then copy+delete) with failure midway; partial batch failure (what is rolled
back, what is reported); cancel mid-copy (partial file cleanup); delete non-empty/read-only; trash unavailable;
undo after external change; bulk rename collisions and swaps (a<->b); idempotence; double invocation; huge
selections; operating on cursor vs selection when both exist (match TUI precedence exactly).
**State**: selection persistence across refresh, sort change, filter change, directory change; cursor restoration
after delete/rename/filter/sort (match oracle rules exactly); history stack bounds; tab close with running job.
**Input**: key sequences interrupted by another key; sequence timeout; counts like `0`, `00`, `999999`; modifier
combos; key repeat floods; IME composition; paste into prompts (multiline, huge); focus loss mid-sequence;
Escape semantics at every level of nesting; Enter vs keypad Enter.
**Concurrency and timing**: watcher events during operations; refresh storms; operations on same dir in two tabs;
main-thread blocking (UI stops responding during large op); UI updates after the window or tab closed.
**Config and persistence**: missing, empty, corrupt, partially valid, newer-version, unknown keys, wrong types,
permission-denied, concurrent writers; byte-compatibility with TUI files; locale and decimal separators in sizes.
**Output and exits**: exit codes; text printed on exit (cd-on-quit, chooser file contents); behavior when stdout is
not a TTY; first-run output; version/help text identity.
**Resource and perf**: memory growth on repeated navigation; handle leaks (watchers, threads); scroll performance on
huge lists; startup time compared with oracle budget.
**Platform**: unix permission bits on Windows; executable bit semantics; `.DS_Store` and `Thumbs.db`; hidden attribute
vs dotfile; reparse points and junctions; case sensitivity; path separator in displays vs storage; line endings in
text previews; default app opening; trash semantics per OS.
**Security**: path traversal in archive extraction; symlink escape in copy/extract; command injection through file
names into shell/open-with templates; temp file races; world-readable state files.
**UX regression (parity-preserving)**: any keyboard-only flow that now requires the mouse; any TUI shortcut that
changed meaning; any state or hint visible in the TUI but missing in the GUI (counts, sizes, hints, mode indicator).

## Finding quality bar

A finding is: reproducible (exact steps), attributed (source refs, oracle capture), expected vs actual, and ideally
executable. Rank severity honestly:
- blocker: data loss, crash, hang, or core flow broken
- high: behavior diverges from oracle in a common flow
- medium: divergence in an edge case or error message
- low: cosmetic divergence not affecting behavior or a minor test gap

Do not file opinions about style. Do not file enhancement ideas as GAPs; send them in the report's "enrichment
candidates" section for the ux-advisor.

## Report (`migration/reports/<row>/adv-<n>.md`)

```
# Adversarial report: F-xxx (mode A|B|C) round n
Commit reviewed: <sha>
Attempts made: <list; include things that passed>
Findings: <id, sev, one line, test name>
Mutation checks: <condition, result>
Verified fixes: <ids>
Reopened: <ids>
Verdict: NO FINDINGS | FINDINGS
Enrichment candidates: <optional>
```
