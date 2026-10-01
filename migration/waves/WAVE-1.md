# Wave 1: Core data, startup, and CLI contracts
Status: planned
Entry criteria: Wave 0 contracts merged; F-001..F-014 reviewed; plan review re-run approves row ownership and conflicts; each active row has a spec and red tests.
Exit criteria: CLI/startup, persisted config/state, base listing and pane state pass oracle-backed tests on Linux/macOS/Windows; no feature row is marked VERIFIED without the full review gate.

| Row group | Batch | Write-set | Reads | Reviewers | Advisors | Status |
|---|---|---|---|---|---|---|
| F-027,F-028,F-029,F-030,F-031,F-111 | A | `crates/core/src/entry/**`; `desktop/src/entry/**`; `tests/entry/**` | F-001,F-002,F-007,F-011 | adv, logic | arch, platform | NOT_STARTED |
| F-015,F-016,F-017,F-018,F-019,F-020,F-142 | B | `crates/core/src/config/**`; `crates/core/src/persistence/**`; `tests/config/**` | F-001,F-004,F-007,F-008 | adv, logic | arch, platform | NOT_STARTED |
| F-021,F-022,F-023,F-024,F-025,F-026,F-104,F-102,F-103,F-105,F-106,F-107,F-108,F-109,F-110 | C | `crates/core/src/state/**`; `tests/state/**` | F-003,F-004,F-005,F-009,F-010 | adv, logic | arch, ux | NOT_STARTED |
| F-058,F-059,F-060,F-061,F-062,F-063,F-064 | D | `crates/core/src/fs/**`; `tests/fs/**` | F-004,F-005,F-006,F-009 | adv, logic | arch, platform | NOT_STARTED |
| F-112,F-113 | E | `migration/reports/**`; `tests/contracts/**` | F-002 | adv, logic | arch, platform | NOT_STARTED |
| F-115 | F | `desktop/src/window/**`; `desktop/tests/lifecycle/**` | F-001,F-011 | adv, logic | platform, ux | NOT_STARTED |

Conflicts considered: all writes are disjoint by crate module and test directory. Batch C waits for shared state contracts in Wave 0. Batch E is a UI barrier and begins after data/input contracts exist. F-102..110 overlap F-021..026 state semantics; keep the state owner single and resolve duplicate inventory evidence inside those specs rather than parallelizing them.

Barrier steps: no feature work before Wave 0. The integrator owns workspace manifests, root/desktop registries, test harness registration, and CI wiring. Any central-file write is serialized through the integrator.
