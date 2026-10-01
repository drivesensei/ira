---
name: subagent-delegation
description: How the Migration Manager briefs, launches, parallelizes, and verifies subagents. Includes the brief template per role, the report contract, parallel launch patterns, write-set enforcement, and result verification. Use every time a subagent is launched or returns.
---

# Subagent delegation

Subagents start with **no context**: no user message, no history, no matrix. The brief is their whole world.
Under-specified briefs are the top cause of rework. Spend the tokens.

## 1. Launch mechanics

- Select the custom agent by its name (`subagent_type`: `gpui-developer`, `adversarial-reviewer`, ...). If a custom
  type is not recognized by the platform, use `generalPurpose` and start the prompt with
  `You are the <role>. First read .cursor/agents/<role>.md and follow it exactly.`
- Parallel launch: **one message, multiple Task calls**.
- Developers: `run_in_background: true`. Reviewers/advisors: foreground when you need the result to decide the next
  step, background when you have other work.
- Never launch two agents whose write-sets overlap. Never launch a developer and a reviewer on the same working tree
  while the developer is still writing (reviewers work on the developer's committed branch or worktree).
- Use `resume` to send follow-ups to a finished agent that holds useful context (for example, send GAP-fix requests
  to the same developer). Start fresh when context is polluted or you want independent eyes.
- Model choice: reviewers and auditors on a different, strongest-available model than developers when possible.
- Do not use cloud/remote subagent mode unless the repo is set up for it; local worktrees are the default.

## 2. Brief template (copy `migration/templates/DEV_BRIEF.md` or `REVIEW_BRIEF.md`)

Every brief contains, in this order:

1. **Role and first reads**: `You are the <role>. Read .cursor/agents/<role>.md and the skills it lists.`
2. **Project coordinates**: TUI path/crate/bin, GPUI app path/crate/bin, core crate, gpui version and source, build and
   test commands that work, oracle worktree path or binary, target OS notes.
3. **Objective**: matrix row id(s) and one-sentence outcome.
4. **Inputs**: exact paths: spec, inventory ids, oracle captures, related reports, decisions that bind this work.
5. **Write-set**: globs you may modify. Everything else is read-only. Include the test locations.
6. **Do-not-touch list**: central registries, other features, TUI baseline.
7. **Definition of done**: concrete, checkable bullets (tests that must exist, commands that must pass, GAP notes
   that must be closed).
8. **Constraints**: invariants from `AGENTS.md` section 7 that matter here, plus feature-specific traps found in
   discovery.
9. **Return format**: what to put in the final message (see section 4), with a line limit.
10. **Escalation**: "If blocked or if the spec seems wrong or incomplete, stop and report; do not improvise
    behavior. Do not spawn subagents."

## 3. Briefs per role: must-haves

**gpui-developer**: spec path; write-set; branch name and worktree path; tests-first instruction; list of existing
red tests and GAP notes to turn green; instruction to flip `GAP(` to `GAP-FIXED(` only after the test passes, never
delete notes; instruction to put unit tests in `tests.rs` siblings; commands to run before returning.

**adversarial-reviewer** (two modes):
- *Red-test mode* (before implementation): spec, oracle captures, attack catalogue pointers, instruction to write
  failing tests (or ignored tests with GAP notes) covering every spec clause plus attack ideas. No production code.
- *Review mode* (after implementation): branch/diff, instruction to run the app and tests, attempt to break,
  write GAP notes with repros, verify fixes (flip `GAP-FIXED(` to `GAP-RESOLVED(` or reopen).

**logic-reviewer**: the TUI source refs and the new code paths; instruction to produce an equivalence table; GAP notes
for each divergence; verdict EQUIVALENT / DIVERGENT.

**advisors**: the specific question or plan to review, the files to read, desired output (ranked recommendations with
rationale, risks, and a clear recommend/avoid list). Advisors never edit code.

**tui-discovery-explorer**: slice name, files claimed, fixture instructions, inventory format, surface contribution,
oracle capture rules, return format.

**integrator**: exact merge/wiring task, branches to merge, shared files it owns, CI changes, conflict policy.

**parity-auditor**: blind-audit brief **without** the matrix, inventories, or prior reports. Only: TUI baseline
location, how to run it, the GPUI app location and how to run it, and the audit procedure from `completion-gate`.

## 4. Report contract (what every subagent returns; ask for it explicitly)

```
STATUS: DONE | BLOCKED | PARTIAL
ROW(S): F-xxx
CHANGED: <file list or git diff --stat>
COMMITS: <shas>
TESTS: <names added/changed>, command run, pass/fail counts
GAPS: opened <ids>, fixed <ids>, resolved <ids>, still open <ids>
FINDINGS (top 5, one line each)
RISKS / OPEN QUESTIONS
NEXT RECOMMENDED ACTION
```
Keep it under 60 lines. Long detail goes in `migration/reports/<row>/<role>-<n>.md` and the final message links it.

## 5. Verify, do not trust

When a subagent returns, **you** verify before changing a status:
1. `git diff --stat <base>..<branch>`: do changed files stay in the write-set?
   `python scripts/scope_check.py <role> --range <base>..HEAD [--allow GLOB ...]`
2. Run the build and the feature's tests yourself (or via integrator). Quote the pass counts in the journal.
3. Open at least the test file diff and skim the production diff for scope creep, `todo!()`, `#[ignore]`, `unwrap()` on
   fallible fs/platform operations, and `cfg(unix)` without Windows handling.
4. Run gate fast mode to see GAP counts.
5. Only then update the matrix. Record SHAs.

## 6. Handling bad returns

| Symptom | Response |
|---|---|
| No tests or tests that cannot fail | reject, relaunch with tests-first and require showing a red run |
| Claims parity without oracle evidence | reject; point to the capture files and require comparison |
| Silent behavior change ("better UX") | revert that part; log under ENRICHMENTS if it is truly an enhancement |
| Out-of-scope edits | revert those files, relaunch with explicit write-set |
| Deleted or edited GAP notes | restore from git, relaunch; note in journal |
| Reviewer edited production code | revert it and relaunch the reviewer with stricter scope; run scope_check |
| Report too vague | resume the same agent asking for specifics (file, line, command output) |

## 7. Parallel patterns

**Pipeline** (default): while developers D1..D4 work, reviewers prepare red tests for D5..D8; when D1 returns,
launch its two reviewers while D2..D4 still run, and start D5 into the freed slot.

**Fan-out discovery**: N explorers over disjoint slices in one message; then cross-check explorers blind.

**Triple advisory**: architecture, ux, platform advisors in one message on the same plan; reconcile conflicts yourself.

**Tournament** (only for high-risk or stuck features): two developers on separate worktrees implement the same
feature; reviewers pick the one with fewer findings. Use sparingly; it doubles cost.
