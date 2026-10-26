---
name: gpui-engineering
description: Engineering guide for building the file manager on GPUI (Zed's Rust UI framework) when porting from a ratatui/crossterm-style TUI. Maps TUI concepts to GPUI concepts, prescribes core/UI separation, actions and key contexts, virtualized lists, async work, text input, theming, and headless testing. Use for developers, architecture-advisor, and manager when making design decisions.
---

# GPUI engineering guide

**Accuracy warning.** GPUI is under active development and its API changes between revisions (entity types, how
windows and contexts are passed, builder method names). The names below are the concepts; **confirm every signature
against the pinned `gpui` version** (see `Cargo.lock`, the gpui source in `~/.cargo/registry/src/*/gpui-*` or the git
checkout, its `examples/` directory, and Context7 docs). When the existing desktop shell already uses an idiom,
copy that idiom. Record the pinned version and discovered idioms in `migration/reports/gpui-notes.md`.

## 1. TUI to GPUI concept map

| TUI concept (ratatui/crossterm style) | GPUI equivalent |
|---|---|
| `main` loop: poll events, update, draw | `Application::new().run(...)`; GPUI owns the loop; you react to events and call `cx.notify()` |
| `App` struct with all state | Split: UI-agnostic **core state** (plain Rust, no gpui) wrapped by an entity (`Entity<AppState>`, older: `Model<T>`) |
| `draw(frame)` with widgets | `impl Render for View`, returning `div()` element trees styled with flex/tailwind-like methods |
| `KeyEvent` match in `handle_key` | `actions!` types + `KeyBinding`s in key contexts, handled via `.on_action(cx.listener(...))` |
| Modes (normal/insert/visual/command) | Key contexts (`.key_context("FileList")`, `"Prompt"`, `"Visual"`) and/or an explicit mode enum in core state mirrored to the context |
| Multi-key sequences (`g g`, `d d`) | Multi-keystroke `KeyBinding`s (verify support and timeout behavior in the pinned version) or a core-level sequence matcher that the harness can also drive |
| Count prefixes (`5j`) | Core-level pending-count state; GPUI binding feeds digits to core |
| Terminal resize event | Window bounds observation; responsive layout via flex and min sizes |
| Scrolling list state | `uniform_list` (fixed row height) or `list` with `ListState` (variable height); keep scroll-to-cursor logic in view, cursor index in core |
| Popups / dialogs | Overlay layer in the root view (modal host) with focus trapping and `Escape` handling |
| Status/message line | Status bar view plus toast queue driven by core messages |
| Color palette / ratatui styles | Theme token struct (colors, spacing, fonts), light and dark; no color literals in views |
| `std::process::Command` suspend-to-shell for editor | Spawn external process without suspending UI; reflect completion via async task |
| OSC 52 / external clipboard commands | GPUI clipboard API (`cx.write_to_clipboard` / `read_from_clipboard`; verify names) plus keep internal yank register semantics from TUI |
| Tick-based redraw | Event-driven notifications; timers via `cx.spawn` with `cx.background_executor().timer(...)` only if TUI behavior depends on time |
| Terminal-only features (mouse reporting toggles, alternate screen) | N/A; mark per `parity-matrix` rules |

## 2. Required architecture

```
crates/
  <name>-core/        pure logic: state model, actions as data, fs service traits, ops, config, formats, errors
                      no gpui, no ratatui, no crossterm. Fully unit-testable. Deterministic.
  <name>-gpui/        GPUI views, action wiring, theme, platform adapters, overlays, components
  <name>-parity/      dev-only: scenario runner + trace format + oracle comparison tools
  <name>-tui/         the original TUI (frozen oracle, builds from tag tui-oracle-baseline). Do not modify.
```
(Names are placeholders; follow the existing workspace naming. If the existing shell has a different structure,
the architecture-advisor proposes the minimal path to this separation and records it as a decision.)

Principles:
- **Core owns behavior; views own presentation.** If a rule can be expressed without gpui, it belongs in core and has
  unit tests there. The parity harness can then replay traces against the core and compare to the oracle.
- **Actions as data**: `enum Command` (or one gpui action type per command mapped to core commands) with a stable
  name, e.g. `file_list::MoveDown`, `ops::Rename`. Names are recorded in the matrix Surface column.
- **Effects, not side effects**: core returns `Effect`s (`SpawnJob`, `WriteFile`, `OpenExternal`, `Toast`, `Quit`);
  the GPUI layer executes them. Tests assert on effects without touching the OS.
- **Filesystem behind a trait** with a real implementation and an in-memory or tempdir fake for tests.
- **Errors**: typed error enums with user-facing messages identical to the TUI where the oracle shows them.
- **State changes only via entity updates** and always followed by `cx.notify()`; avoid interior mutability hacks.
- **Subscriptions** (`cx.subscribe`, `cx.observe`) tied to the lifetime of the view; store `Subscription` handles.

## 3. Actions, key bindings, focus

- Define commands with the `actions!` macro in a feature-local module; bind with `cx.bind_keys([...])` in the feature's
  `register(cx)`. Central registry collects bindings and a test asserts **no duplicate (context, keystroke)** pairs.
- Reproduce the TUI keymap exactly, including mode-specific meaning of the same key. Add desktop aliases (platform
  `cmd-` on macOS, `ctrl-` shortcuts on Windows, arrow keys, `delete`, `f2`) as **additional** bindings, never
  replacing the TUI ones. Keep an alias table in `migration/oracle/keymap-aliases.md`.
- Focus: each focusable region has a `FocusHandle`; the root decides which context is active. Overlays capture focus
  and restore it on close. Test focus restoration explicitly.
- Text input: GPUI has no built-in text field. Decide with architecture-advisor: build a small input component
  (cursor, selection, IME, clipboard, undo) or adopt a maintained component crate compatible with the pinned gpui
  (`gpui-component` or Zed's `ui`/`editor` pieces; check license, maintenance, and version). Document in DECISIONS.
  IME and non-Latin input must work; the TUI may not handle it, but desktop users expect it.

## 4. Lists and large directories

- Never render all rows. Use `uniform_list`/`list` virtualization.
- The cursor and selection model is in core and indexes into the **sorted, filtered view model**; the view only maps
  indexes to rows. Selection is by stable entry identity (path plus file id where available), not by row index, so it
  survives refresh and re-sort exactly as the TUI does (verify the TUI rule in the oracle).
- Directory loading is incremental and cancellable. Show the same intermediate states the TUI shows (for example
  "loading", partial listings) only if the oracle shows them.
- Sorting and filtering run off the main thread for large directories but must produce deterministic orders identical
  to the TUI (same comparators, same tie-breakers, same locale/case rules).

## 5. Async and jobs

- Use `cx.spawn` for UI-coupled async work and `cx.background_executor().spawn` for CPU or blocking IO.
- File operations run as jobs in core's job runner: progress events, cancellation tokens, per-item errors, final
  summary. The GPUI layer subscribes and renders. Jobs must be testable without GPUI.
- File watching (`notify` crate or whatever the TUI uses): debounce rules identical to TUI refresh behavior;
  platform differences handled in the fs service.
- Never call blocking `std::fs` on the main thread except for trivially small, bounded reads that the TUI also did
  synchronously AND that the platform-advisor approves.

## 6. Theming and components

- Theme tokens struct loaded from the TUI's theme/config where the TUI had themes (mapping documented), plus light/dark.
- Build reusable components once: `Button`, `IconButton`, `TextInput`, `Modal`, `Menu`/`ContextMenu`, `Toast`,
  `Tooltip`, `Splitter`, `Tabs`, `Breadcrumbs`, `StatusBar`, `KeyHint`. Keep them in a `components` module owned by
  the integrator or a dedicated foundation developer. Features compose them; they do not re-invent them.
- Fonts: bundle or pick a monospace and a UI font with cross-platform availability; fallback chains per OS.
- Icons: a single icon abstraction; SVG assets embedded with `rust-embed` or `include_bytes!` (follow existing shell).

## 7. Testing GPUI

- Behavior tests in **core** first (fast, deterministic).
- GPUI integration tests using gpui's test support (`#[gpui::test]`, `TestAppContext`, `VisualTestContext`, simulated
  keystrokes) require the `test-support` feature of gpui in dev-dependencies; confirm the exact setup in the pinned
  version's tests/examples. Use them for: key bindings in contexts, focus transitions, overlay open/close, list
  scrolling to cursor, action dispatch to core.
- Parity harness: a trace is a list of `(input, expected observable state)`. The same trace is replayed against the
  oracle (TUI, via pty capture or via core if the logic was characterized) and against the new app (via simulated
  keystrokes in GPUI tests and/or core). Compare observable state: cwd, listing order, cursor, selection, messages,
  filesystem result, persisted files.
- Visual checks: screenshots through the `computerUse` subagent if a display is available; otherwise rely on
  structure assertions. Never `VERIFIED` a purely visual row without a captured artifact.

## 8. Performance and quality

- Budgets measured from the TUI (`oracle/perf.md`): time to first listing, time to open a 100k-entry directory,
  key-to-cursor latency, memory.
- `cargo fmt`, `cargo clippy --workspace --all-targets -D warnings`, no `unwrap()`/`expect()` on fallible fs or
  platform calls in non-test code, no `todo!()`.
- Log with `tracing` or whatever the TUI used; keep log file locations compatible.
- Avoid global statics for app state. Avoid `Arc<Mutex<_>>` soup; prefer entities and channels.

## 9. Pitfalls checklist

- Key context mismatch: binding exists but never fires because the focused element lacks the context.
- Forgetting `cx.notify()` after state change.
- Dropped `Subscription` or `Task` (work silently stops). Store handles.
- Re-render storms from notifying on every fs event. Debounce.
- Modal focus leaks: key presses reaching the list under a dialog.
- Selection stored as indexes and corrupted by re-sort.
- macOS-only assumptions about `cmd`; Windows path separators and drive switching in "go to path".
- Blocking the main thread inside action handlers.
- Window size not persisted when the TUI persisted layout state.
