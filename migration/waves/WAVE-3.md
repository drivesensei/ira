# Wave 3: Preview and external integrations
Status: planned
Entry criteria: filesystem, action, async-job, platform, and view contracts are verified; each row has an oracle-backed spec and red tests.
Exit criteria: supported previews and OS integrations preserve TUI outcomes on macOS and Windows, with native smoke evidence and all reviewer gaps resolved.

| Row group | Batch | Write-set | Reads | Reviewers | Advisors | Status |
|---|---|---|---|---|---|---|
| F-074,F-075,F-076,F-077,F-078,F-079,F-080,F-081,F-082,F-146 | A | `crates/core/src/preview/**`; `desktop/src/preview/**`; `tests/preview/**` | F-005,F-006,F-009,F-010,F-014 | adv, logic | arch, ux, platform | NOT_STARTED |
| F-083,F-084,F-085,F-086,F-087,F-089,F-090,F-091,F-092,F-122 | B | `crates/core/src/platform/**`; `desktop/src/platform/**`; `desktop/tests/integrations/**` | F-003,F-014,F-114,F-120 | adv, logic | platform, ux | NOT_STARTED |

Conflicts considered: B overlaps the Wave 2 platform adapter write-set; implement sequentially after its contracts or move into a later adapter batch. A may run only after the preview view contract is frozen. Rows documenting an absent terminal feature need a source-backed no-op/absence spec; they are not permission to add behavior.

## Held for platform equivalence decision

| Row | Reason | Status |
|---|---|---|
| F-088 | TUI Linux mount/eject action has no characterized macOS/Windows desktop equivalent; retain until explicit sign-off | BLOCKED |
