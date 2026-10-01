# Advisory: UX key model by ux-advisor round 1

Scope reviewed: `migration/inventory/entry-input.md`, `config-state.md`, `state-search.md`, `preview-external.md`, `files-ops.md`, `rendering-messages.md`; `src/handler.rs`, `src/ui/mod.rs`; `desktop/src/main.rs`; `migration/reports/gpui-notes.md`; GPUI engineering sections 3 and 6. The shell is a one-window click demo, not an interaction foundation.

Summary verdict: PROCEED WITH CHANGES. Build one core-owned input/mode state machine and route GPUI actions through it. Do not let native menu accelerators preempt an existing IRA binding in the same context.

## Ranked recommendations

R1 [must] Preserve the exact dispatch priority in `src/handler.rs:19-226` as an explicit ordered mode/context policy. Effective order: preview editor (Ctrl+C global quit; Ctrl+S save; Esc return; Tab changes pane and discards edits; otherwise editor owns keys); global Ctrl handler (Ctrl+C quit, Ctrl+A selection unless rename, Ctrl+- eject; other Ctrl swallowed); rename; go-to-path; create-new; help overlay (next key dismisses); error (next key dismisses); deletion progress (next key hides, job continues); multi-info (next key closes, walk continues); info (`x` cancels walk, `r` restarts, otherwise closes); confirmation (`o`, y/Enter, n/Esc); search (Esc cancel, Enter commit, text/backspace, arrows; Right enters folder); Copy Board focus (only its listed keys, rest swallowed); normal pane. Evidence: INV-INPUT-005..012, `src/handler.rs`; help's priority over background notice is intentional. Model explicit states and parent/child focus; overlays trap focus and restore prior pane/board focus. Escape must retain each mode's observed meaning, not universally close the top widget.

R2 [must] Keep `q`, Ctrl+C, `*`, navigation (`arrows`, z/x, Left/Right), pane Tab, file/action keys, and mode-specific meanings as first-class bindings. No count prefix or multi-key sequence exists (INV-INPUT-002); do not add a sequence timeout or reinterpret digits. GPUI 0.2.2 supports `actions!`, `KeyBinding::new`, `App::bind_keys`, `FocusHandle`, and headless `TestAppContext`/`VisualTestContext`/`simulate_keystrokes` per local locked source recorded in `migration/reports/gpui-notes.md`. Use dedicated contexts (`FilePane`, `Search`, `Rename`, `PathPrompt`, `CreatePrompt`, `Confirm`, `Info`, `CopyBoard`, `TextEditor`, `Help`) but keep the core dispatcher authoritative for precedence and global Ctrl behavior.

R3 [must] Avoid aliases that steal a TUI key or alter an operation. Suggested additional aliases only where semantics are unambiguous: F2→rename (same selected entry and validation as Enter); Delete→delete (native label, existing TUI key; on macOS Backspace remains delete); Escape/Enter/arrows in prompts; Cmd+Q on macOS and Alt+F4 on Windows as OS-level quit; Ctrl+Q as optional quit alias only if no conflict in all contexts. Do **not** bind Cmd+C to OS clipboard-copy: IRA `c` copies to the other pane and clipboard is current-folder-only via `]` (INV-INPUT-003/004, INV-PE-014). Do not bind Cmd+V to paste files (TUI has no such behavior); prompt text paste uses native text input and preserves TUI prompt semantics. Do not assign Ctrl+W to close tab/window: tabs are absent (INV-STATE-001). Menu labels must show the actual IRA shortcut plus alias where present.

R4 [should] Centralize action metadata so menus, tooltips, help and command palette read the same stable action label and key hints. Preserve `*` help overlay as an any-key-to-dismiss overlay (INV-INPUT-013); an additive palette may be opened by Cmd+Shift+P / Ctrl+Shift+P without changing `*` or adding a TUI command mode. Exclude text-edit focus from global shortcuts except the already-global Ctrl+C behavior; Ctrl+S is save only in editor.

## Conflict table

| Candidate OS/native shortcut | TUI or platform collision | Recommendation |
|---|---|---|
| Ctrl+C | Quits in every mode (INV-INPUT-005..011) | Preserve as IRA behavior, including modal/editor. Clipboard copy must use a menu/explicit separate action only if it does not capture Ctrl+C. |
| Cmd+C / Ctrl+C | OS conventional clipboard copy | Do not map to clipboard; keep Ctrl+C quit. On macOS Cmd+C may be added for clipboard only in a text field if GPUI/native input owns it, but do not route to pane action. Verify per platform. |
| Cmd+Q / Alt+F4 | OS quit | OS lifecycle quit alias; equivalent intent to q. Confirm unsaved edit behavior remains baseline (TUI quits without prompt per handler). |
| Ctrl+W | Conventional close tab/window; no TUI tab stack | Leave unbound. |
| Cmd+V / Ctrl+V | OS paste; TUI supports bracketed paste into active prompt/editor | Native text insertion only while text input owns focus; never paste filesystem items by default. Verify prompt paste normalization separately. |
| Delete / macOS Backspace | Delete with confirmation; macOS Backspace is explicitly included | Preserve existing platform condition; Delete menu item and confirmation y/Enter, n/Esc unchanged. |
| Enter / F2 | Enter opens rename prompt; no F2 binding | F2 additive alias; must invoke identical rename flow, not commit directly. |
| Cmd+S / Ctrl+S | Ctrl+S saves only in preview editor; other Ctrl combos swallowed | Optional platform save alias only in text-editor context; no global save action. |
| Cmd+W | No tab support; could close app/window | Leave unbound until close semantics and multi-window parity are characterized. |

## UX floor and verification

Keyboard tests: replay oracle traces for each binding in each mode; assert swallowed keys do not reach the underlying pane, Escape unwinds the right state, modal close restores focus, and pending job continues after hiding progress. GPUI 0.2.2 test support is available but absent from the current desktop manifest; add it in dev configuration as architecture decides. Keep semantic behavior tests in core, UI tests for focus/action routing. Mouse/context menu paths must invoke the same action IDs; test mouse and keyboard outcomes converge. Native OS accelerators, IME, clipboard, accessibility tree, and window lifecycle need actual macOS and Windows CI/manual artifacts; headless tests alone do not establish them.

Unverified: dynamic captures are still missing for destructive confirmation/progress, most shortcuts, paste/IME, Copy Board mode, dialog focus behavior and native OS mappings. Inventory confidence is source-high for dispatch order, not cross-platform runtime.

Risks not covered: GPUI key-event normalization and native menu precedence can differ by platform; validate exact GPUI 0.2.2 APIs from locked source and real macOS/Windows runners. No Context7 connector was available; GPUI notes explicitly rely on local pinned crate source.
