# Migration state (rewrite every loop)

Updated: <timestamp>   Branch: <integration branch>   HEAD: <sha>
Phase: <0..6>   Current wave: <n>   Gate: <PASS|FAIL (n violations)>

## Project coordinates
- TUI crate/bin/path:
- GPUI app crate/bin/path:
- Core crate:
- gpui dependency (version or git rev):
- Build/test commands that work:
- Oracle baseline tag: tui-oracle-baseline -> <sha>; oracle worktree/binary:
- CI: <workflow files>, last green runs: <ids>

## Counts (from `python scripts/parity_gate.py --summary`)
rows=<n> final=<n> (<pct>%)  | NOT_STARTED <n> | IN_DEV <n> | IN_REVIEW <n> | FIXING <n> | VERIFIED <n> | EQUIVALENT <n> | WAIVED <n>
Open GAPs: blocker <n> high <n> medium <n> low <n>   Awaiting verification: <n>
Blind audit clean rounds: <0|1|2>

## In flight
| Agent | Role | Row(s) | Branch/worktree | Launched | Expect |
|---|---|---|---|---|---|

## Blockers
- <none>

## Next 5 actions
1.
