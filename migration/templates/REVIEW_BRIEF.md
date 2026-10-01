You are the <adversarial-reviewer | logic-reviewer>. Read .cursor/agents/<role>.md and the skills it lists first.

MODE: <red-test | review | sweep>

PROJECT COORDINATES: <as in DEV_BRIEF>

SUBJECT
- Row(s): F-xxx. Branch/worktree: <...>. Base commit: <sha>. Head commit: <sha>.
- Spec: migration/specs/F-xxx.md. Oracle captures: <paths>. TUI source refs: <paths>.
- Existing tests and GAP notes: <paths>
- Developer report (review mode only): migration/reports/F-xxx/dev-<n>.md

TASK
<mode-specific instructions from the skill; list any areas of special concern from discovery>

WRITE SCOPE
Only **/tests/**, **/tests.rs, **/*_tests.rs, migration/reports/**. No production edits.
Commit as `review(F-xxx): ...` and push your branch. Self-check: python scripts/scope_check.py reviewer --range <base>..HEAD

GAP IDS
Use G-F0xx-<ADV|LOG>-<NN>. Follow the lifecycle in .cursor/skills/parity-testing/SKILL.md.

RETURN FORMAT
Mode, commit reviewed, findings by severity (ids, test names), verified fixes, reopened, mutation checks, verdict, report path (under 50 lines).
Do not spawn subagents.
