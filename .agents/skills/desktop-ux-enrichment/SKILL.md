---
name: desktop-ux-enrichment
description: Rules for making the GPUI desktop UI richer than the TUI without breaking 1:1 parity. Defines the required UX floor, an enrichment catalogue, the parity-first ordering, and how enrichments are tracked and reviewed. Use for ux-advisor, developers, and the manager.
---

# Desktop UX: richer, never different

The TUI is the behavioral contract. The desktop can add presentation and interaction channels. It cannot remove,
rename, reorder, or change the meaning of any TUI behavior.

## 1. Parity floor (must hold for every feature)

- Every TUI key binding works, in the same mode/context, with the same result.
- Every TUI mode is visible (mode indicator) and behaves the same way, including `Escape` unwinding.
- Every TUI status, hint, count, size total, selection summary, and message is shown somewhere equivalent.
- All TUI flows are completable **keyboard-only**.
- Same defaults: sort, hidden files, view layout, preview toggle state, confirmation prompts.
- Same on-disk state and config semantics.

## 2. UX floor (required before the Completion Gate; part of Wave 0 and hardening)

Desktop table stakes, implemented as foundation or hardening rows (they get matrix rows with area `ux-floor`):
- Mouse: click selects/moves cursor, double click opens, wheel scroll, shift/ctrl(cmd) click multi-select with the
  same semantics as keyboard selection, middle/right click context menu with the same actions as keys.
- Window: native chrome, resizable with sane minimum, remembers size/position/maximized state, multi-window if the TUI
  supports multiple instances.
- Theming: light and dark following OS, high contrast tolerance, HiDPI crispness, font fallback.
- Text input: IME, selection, clipboard, undo in all prompts.
- Native OS integration: file open dialogs where the TUI asked for a path, reveal in Finder/Explorer, open with
  default app, trash semantics, drag out/in at least as copy of path or file (see enrichments for full DnD).
- Accessibility basics: focus visible, tab/arrow navigation in dialogs, readable contrast, labels for icon-only
  controls.
- Responsiveness: no UI freeze during long operations; progress and cancel everywhere the TUI had them.
- Feedback states: loading, empty, error, permission denied, offline/removed volume.

## 3. Enrichment catalogue (optional, ordered by value/risk; ux-advisor selects and bounds)

Tier 1 (low risk, high value): breadcrumbs with click navigation; command palette listing all actions with their key
bindings; key hint overlay (which-key style) for multi-key sequences; context menus; toolbar with common actions;
rich status bar; file icons by type; sortable column headers (same sort semantics as keys); inline rename (same
validation as TUI rename); toasts with undo where the TUI had undo.
Tier 2: tabs with drag reorder; dual pane / split view toggle (if the TUI is single pane, off by default); preview pane
with images, markdown, syntax highlighting, hex, thumbnails; sidebar with places/bookmarks (backed by the TUI bookmark
store); progress center for jobs; settings UI editing the same config file; search UI with filters.
Tier 3: drag and drop between panes and to/from the OS; quick look; batch rename editor UI; archive browsing;
multi-window; themes marketplace; animations (subtle, disabled by reduced-motion).

## 4. Rules for enrichments

1. Parity first: an enrichment for area X starts only after all matrix rows of area X are `VERIFIED`.
2. Tracked separately in `migration/ENRICHMENTS.md` as `E-001...` with: description, area, parity rows it touches,
   rationale, risk, status. Never mixed into parity rows.
3. Default behavior unchanged. If an enrichment changes default presentation (for example, showing a sidebar), it
   needs ux-advisor approval and a setting to restore TUI-like layout. Parity tests run in TUI-like layout.
4. Every enrichment ships with: tests, keyboard accessibility, and a statement of which parity rows it could affect.
   The adversarial-reviewer re-runs those rows' tests and attacks interactions.
5. Time-box: enrichment must never delay parity. If the parity matrix has any row not VERIFIED, enrichment work in
   unrelated areas pauses unless capacity is idle.
6. No enrichment may add network access, telemetry, or auto-update without a recorded decision. Default off.
7. Enrichment cannot introduce new on-disk state without versioning and a compatibility note (the TUI must still be
   able to read its own files; extra state goes in separate files).

## 5. Look and feel guidance

- Dense, calm, productivity-grade UI: consistent spacing scale, clear typographic hierarchy, restrained color.
- Monospace for file names/sizes columns where alignment matters; proportional UI font elsewhere.
- Every action is discoverable with its key shown in menus and tooltips (`KeyHint` component).
- Focus ring and selection colors distinct and accessible; cursor row vs selected rows clearly differentiated
  (the TUI distinguishes them; the GUI must too).
- Respect platform conventions as aliases only: macOS `cmd-` equivalents, Windows menu mnemonics, but TUI bindings
  stay first-class.
- Motion: minimal; no animation on core navigation paths that would delay feedback versus the TUI.

## 6. ux-advisor deliverables

- `reports/advisory/ux-keymodel.md`: how modes, counts, sequences, and overlays map to key contexts, including a
  conflict table with OS shortcuts (for example `ctrl-w`, `cmd-q`, `alt-f4`).
- `reports/advisory/ux-layout.md`: window layout proposal preserving TUI information density.
- `reports/advisory/ux-enrichment-plan.md`: selected enrichments, order, bounds.
- Reviews of each feature spec with UI surface (short).
