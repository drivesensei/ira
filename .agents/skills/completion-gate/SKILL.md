---
name: completion-gate
description: Final procedure to decide whether the TUI to GPUI migration is complete. Runs the gate script, the blind parity audit (twice, with fresh auditors), a final integrated adversarial sweep, platform CI verification, and writes the final report. Use when the matrix looks fully verified, or when the manager believes the work is finished.
---

# Completion gate

Believing you are done is not evidence. Evidence is: scripts green, blind audits clean twice, CI green on both OSes.

## Step 1: Mechanical gate

```bash
python scripts/parity_gate.py --with-cargo --surface migration/oracle/surface.txt
python scripts/parity_gate.py --summary
```
Must exit 0. Fix every violation: each becomes a matrix row or GAP note and goes back through the gauntlet.
Violations the script reports: non-final matrix statuses, missing evidence, open `GAP(`, `GAP-FIXED(` awaiting
verification, bad `#[ignore]`, `todo!()`/`unimplemented!()`, uncovered `surface.txt` items, bad waiver records,
fmt/clippy/test failures.

## Step 2: CI evidence

- Ensure the integration branch head has CI runs on macOS and Windows (and Linux if applicable) that are **green**
  and include the full test suite and clippy. Record run ids/links in `FINAL_REPORT.md`.
- If tests are `#[ignore = "env: ..."]`, confirm a CI job executes them.

## Step 3: Blind parity audit (fresh `parity-auditor` agents)

Why blind: the matrix reflects what we believed existed. An auditor that has not seen it can find what we missed.

Launch **three auditors in parallel** (fresh agents, no resume), each with a different lens, each **forbidden to read
`migration/PARITY_MATRIX.md`, `migration/inventory/**`, `migration/specs/**`, or prior reports**:

1. **Source-surface auditor**: derives, from the TUI source at `tui-oracle-baseline`, a list of every user-visible
   capability, key binding, command, flag, env var, config key, persisted file, message, and exit behavior. Writes
   `reports/audit/<round>/source-surface.md`. Then checks each item in the GPUI app by running it or by reading its
   tests; marks FOUND / MISSING / DIFFERENT with evidence.
2. **Runtime-walkthrough auditor**: uses the TUI (tmux/pty) and the GPUI app (computerUse subagent or headless harness)
   side by side on the same fixtures; executes broad exploratory scenarios per feature area; reports any behavioral
   difference. Writes `reports/audit/<round>/walkthrough.md`.
3. **Persistence and CLI auditor**: round-trips every persisted file between TUI and GPUI app, exercises every CLI
   flag and exit/stdout contract, config edge cases (missing/corrupt), first run and upgrade. Writes
   `reports/audit/<round>/persistence-cli.md`.

Each auditor returns: items checked, MISSING/DIFFERENT list with severity and repro, and an explicit statement
`ZERO UNMATCHED` or `UNMATCHED: n`.

Manager then diffs: every MISSING/DIFFERENT becomes a matrix row (`F-0nn`, origin = audit round) or a GAP note, and
the loop resumes. After fixes, run a **new** audit round with **new** auditor instances.

**A clean round** = all three report `ZERO UNMATCHED`. **Two consecutive clean rounds are required.**

## Step 4: Integrated adversarial sweep

One `adversarial-reviewer` (mode C, final sweep) plus one `logic-reviewer` on cross-cutting logic (key engine,
state, jobs, persistence). Zero open GAP notes after their pass. Include long randomized scenarios against core
invariants and a soak test of the GUI (open dirs, run ops, resize, switch tabs) for crashes, leaks, and hangs.

## Step 5: Advisor drift check

All three advisors review `DECISIONS.md` against the final tree: any decision not honored, any enrichment altering
parity defaults, any platform risk without a test. Resolve findings.

## Step 6: UX floor check

Verify every item of the UX floor in `desktop-ux-enrichment` section 2 has a VERIFIED row or recorded waiver.
Screenshots (computerUse) of main states on macOS and Windows runners or local machines if available, saved under
`migration/reports/final/screenshots/`.

## Step 7: Final report

Copy `migration/templates/FINAL_REPORT.md` to `migration/FINAL_REPORT.md` and fill it with real numbers and links:
rows by status, waivers and equivalents with decision ids, enrichments delivered, CI run ids, audit rounds, known
limitations (must be empty of parity issues), how to run, how to run the oracle, commit SHA of completion.

## Step 8: Re-run gate and finish

Run Step 1 again on the final commit. Commit, push. Only then stop. State in your final message: gate output summary,
audit round ids, CI links, and the FINAL_REPORT path. Do not delete the TUI.

## If anything regresses mid-way

Reopen the affected rows (`REGRESSED`), cancel the clean-round counter, and restart from Step 1.
