# Wave 2: Input, operations, platform adapters, and desktop UX floor
Status: planned
Entry criteria: Wave 1 row dependencies verified or explicitly accepted as merged contracts; each feature has an oracle-backed spec and red tests.
Exit criteria: all action contexts, file operations, platform services, responsive layout, and UX floor rows pass their reviews; macOS and Windows CI is green.

| Row group | Batch | Write-set | Reads | Reviewers | Advisors | Status |
|---|---|---|---|---|---|---|
| F-032,F-033,F-034,F-151,F-035,F-036,F-037,F-038,F-039,F-040,F-041,F-042,F-043,F-044,F-124,F-125,F-126,F-127,F-128,F-129,F-134,F-135,F-136,F-137,F-138,F-139,F-140,F-141,F-143,F-144,F-145,F-149 | A | `crates/core/src/actions/**`; `desktop/src/keymap/**`; `desktop/src/inputs/**`; `tests/actions/**` | F-003,F-004,F-009,F-010,F-011 | adv, logic | ux, arch, platform | NOT_STARTED |
| F-045,F-046,F-047,F-048,F-049,F-050,F-051,F-052,F-053,F-054,F-055,F-056,F-057 | B (platform consumer) | `crates/core/src/ops/**`; `desktop/src/operations/**`; `tests/ops/**` | F-004,F-005,F-006,F-009,F-010 | adv, logic | platform, arch | NOT_STARTED |
| F-065,F-066,F-067,F-071,F-120 | C (after B; adapter barrier) | `crates/core/src/platform/**`; `desktop/src/platform/**`; `tests/platform/**` | F-004,F-011,F-014 | adv, logic | platform, arch | NOT_STARTED |
| F-093,F-094,F-095,F-096,F-097,F-098,F-099,F-100,F-101,F-114,F-116,F-117,F-118,F-119,F-121,F-123,F-130,F-131,F-132,F-133,F-147,F-148 | D | `desktop/src/ux/**`; `desktop/src/components/**`; `desktop/tests/ux/**`; `desktop/tests/rendering/**` | F-008,F-009,F-010,F-011,F-012 | adv, logic | ux, platform | NOT_STARTED |

Conflicts considered: B consumes F-014 service contracts and C starts only after B merges; Wave 3 Batch B starts only after Wave 2 C merges; A is a barrier because the action registry/key ownership is shared. D follows stable event/action contracts and avoids editing shared key registries. UX acceptance includes: mouse click, double-click, wheel, modified selection and context-action convergence; OS window close/minimize/restore/reopen/focus/quit; system light/dark, contrast, HiDPI and font fallback; text selection/undo/clipboard/IME; accessibility roles/focus traversal/contrast/icon labels; empty/no-match/loading/error/permission/removed-volume states. Native macOS and Windows evidence is required per OS-dependent row.

Terminal cell overlays/font probing (F-068..F-070) and Windows Terminal font settings (F-073) require row-level parity mapping and sign-off before implementation; do not port host-specific terminal geometry into GPUI.

## Held for terminal-host equivalence decisions

These rows remain in this wave for tracking but have no implementation batch until row-level user-intent mapping is signed by UX, logic, and adversarial reviewers.

| Row | Reason | Status |
|---|---|---|
| F-068 | Terminal image overlay cell geometry/composition maps to the native preview pane; TUI-host overlay mechanism is not portable | BLOCKED |
| F-069 | macOS terminal-host AppKit overlay lifecycle/geometry requires GPUI mapping decision | BLOCKED |
| F-070 | Windows terminal-host popup/font-cell metrics require GPUI mapping decision | BLOCKED |
| F-072 | TUI Nerd-font probing must map to native icon/font fallback, if needed | BLOCKED |
| F-073 | Windows Terminal/VS Code font-setting mutation has no direct desktop-app equivalent | BLOCKED |
