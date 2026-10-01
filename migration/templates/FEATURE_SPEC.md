# Spec F-xxx: <feature>

Matrix row: F-xxx | Inventory: INV-... | Wave: n | Deps: F-...

## Oracle behavior (cite evidence for every clause)
Numbered clauses; each is one testable statement. Include exact strings and defaults.
1. [S1] When ... then ... (src/ops/x.rs:LINE; captures/fileops/x.txt)
2. [S2] ...

## Triggers and surface
Keys (mode/context), commands, CLI flags, config keys, files touched. Exact `kind:name` items.

## State and effects
State read/written. Effects (spawn job, write file, toast, quit). Ordering.

## Errors and edge cases
Each error: trigger, message (exact), resulting state.

## Config influence and persistence
Keys, defaults, parse rules, file format and compatibility notes.

## Platform notes
macOS / Windows differences, terminal-only aspects and proposed desktop mapping.

## GPUI/desktop surface (parity floor only)
Where it appears, key context, focus behavior, mouse parity items, aliases (additions only).

## Write-set and dependencies
Globs the developer may modify; contracts consumed.

## Test plan
Clause -> test name map (S1 -> tests/fNNN.rs::name). Attack catalogue items that apply.

## Open questions
