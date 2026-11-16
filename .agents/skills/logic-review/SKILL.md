---
name: logic-review
description: Protocol for the logic reviewer, who proves behavioral equivalence between the TUI source and the new core/GPUI code by structured reading, state-machine comparison, and trace derivation. Produces an equivalence table and GAP notes in test files. Use when acting as logic-reviewer or briefing one.
---

# Logic review: prove equivalence, do not sample it

The adversarial reviewer asks "can I break it?". You ask "is it **the same**?". You work by reading both codebases and
deriving traces, then checking them by running tests. You may block verification.

Write scope: test files and `migration/reports/**`. No production edits.

## Procedure

1. **Locate the oracle logic**: from the spec and matrix `TUI refs`, read the TUI functions fully, including helpers,
   constants, config defaults, and error handling. Follow calls until you hit the terminal I/O boundary.
2. **Extract the semantic model** in a few lines each: inputs, state read, state written, outputs/effects, ordering,
   error paths, early returns, default values, and magic numbers.
3. **Locate the new logic**: core functions and GPUI handlers for the same feature.
4. **Build the equivalence table** (one row per decision point in the oracle):

```
| # | Oracle (path:line) | Condition / step | New (path:line) | Equivalent? | Notes |
|---|---|---|---|---|---|
| 1 | src/ops/rename.rs:104 | empty new name -> Err(EmptyName) | core/ops/rename.rs:58 | yes | |
| 2 | src/ops/rename.rs:111 | name contains '/' -> Err(InvalidName) | core/ops/rename.rs:63 | NO | new code allows '/' on Windows |
```
5. **Cover every branch**: if/else arms, match arms, loop boundaries (0, 1, many, max), integer arithmetic (overflow,
   rounding, off-by-one), sort comparators (stability, tie-breakers, case folding, locale), string handling
   (truncate, width, normalization), iteration order of maps and directory listings, default values.
6. **State machine check**: for modal or sequence-driven features, enumerate (state, input) -> (state', effects) for
   the oracle and the new code. Differences are findings. Include "unhandled input in this state" behavior.
7. **Derive traces** that distinguish any suspected difference, and add them as tests (differential or characterization).
   Run them. A divergence that you cannot demonstrate with a test is a hypothesis; mark `confidence=low` in the note.
8. **Check effect ordering and atomicity**: what happens first, what is persisted when, what is visible if an error
   occurs midway, and whether messages appear in the same order.
9. **Check configuration influence**: every config key that influences this feature in the oracle must influence it
   identically in the new code, with the same default, parse rules, and error behavior.
10. **Check data compatibility**: serialization field names, order, whitespace, newline conventions, versioning,
    unknown-field handling, encoding.
11. **Check terminal-to-desktop mapping validity** for `EQUIVALENT_VERIFIED` candidates: state precisely which user
    intent the terminal behavior served and show the desktop behavior serves the same intent in all branches.
12. **Verify fixes**: for each `GAP-FIXED(` of yours, re-derive the affected rows of the table, run the test, then flip
    to `GAP-RESOLVED(` or reopen.

## Red flags that merit a closer look

- Re-implementation instead of reuse when the oracle logic could have been moved into core intact.
- "Simplified" error handling (collapsing distinct errors, changing messages).
- Different sort or filter semantics hiding behind similar names.
- Iterating a `HashMap` where the oracle used ordered data, or vice versa.
- Behavior depending on terminal size, which must be mapped to window size consciously.
- Platform `cfg` forks that make the same operation behave differently across OS without oracle justification.
- Constants copied wrongly (timeouts, limits, thresholds, default sizes).
- Implicit behaviors: automatic refresh after operations, cursor movement after delete/paste, selection clearing rules,
  message persistence duration, history updates, config writes on exit.

## Report (`migration/reports/<row>/logic-<n>.md`)

```
# Logic report: F-xxx round n
Commit reviewed: <sha>
Oracle files read: <list>
Equivalence table: <inline or linked>
Divergences: <ids, sev, test names>
Verified fixes: <ids>
Verdict: EQUIVALENT | DIVERGENT (n open)
Confidence: high|medium|low and why
```

Verdict EQUIVALENT is a strong claim. Give it only when every row of the table is "yes" and each distinguishing trace
you derived passes.
