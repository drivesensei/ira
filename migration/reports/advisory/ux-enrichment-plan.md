# Advisory: UX enrichment plan by ux-advisor round 1

Scope reviewed: `migration/inventory/*.md`, `migration/oracle/surface.txt`, `migration/ENRICHMENTS.md`, `desktop/src/main.rs`, `migration/reports/gpui-notes.md`, desktop UX enrichment skill.

Summary verdict: PROCEED WITH CHANGES. The present desktop is only a click demo. The enrichment backlog is conditional; implement the UX floor and parity first, and track each accepted additive feature separately as E-xxx. Do not present these recommendations as parity-complete work.

## Ranked recommendations

R1 [must] Enrichment starts only after every parity row in its named dependency area is VERIFIED. Keep behavior-changing ideas out; parity layout tests run with enrichments off and the TUI-like profile. Add no network, telemetry, plugin, script, or new write format. Any desktop-only preference belongs in a separate versioned file and cannot alter `~/.config/ira/state` or bookmarks bytes (INV-CONFIG-001..006).

R2 [should] Tier 1, order after foundation and relevant parity rows:

| Proposed E | Enrichment and bounds | Exact parity dependencies / interaction risk |
|---|---|---|
| E-001 Breadcrumb navigation | Click path segments to navigate; current path and `..` remain visible. Use core navigation action and never add browser-history semantics. | INV-STATE-001/006, INV-STATE-SEARCH-003, INV-INPUT-006, INV-filesystem-001. Risk: path encoding, symlink resolution, drive roots; test same target as `[path]`/Left/Right. |
| E-002 Context menus | Right-click selected row or pane background; menu lists action labels and TUI keys. Menu dispatches stable actions only. | All INV-INPUT-002..012 and matching INV-files-ops rows; mouse semantics are additive (INV-INPUT-001). Risk: wrong target/selection and accidental destructive action; menu delete still opens same confirmation. |
| E-003 Key hint overlay / action palette | Keep `*` help unchanged; optional which-key hints and palette via unused desktop shortcut only. Search action registry; show original key. No command mode or user scripting. | INV-INPUT-003/013; INV-RENDER-005; every action row. Risk: inaccurate/stale key labels; generate from registry and test against matrix map. |
| E-004 Sortable headers | Clicking Name/Size/Modified/Kind invokes the same per-pane sort cycle and comparator; avoid arbitrary column sort or altered default. | INV-STATE-003, INV-STATE-SEARCH-005, INV-files-ops-002/004. Risk: sorting can move cursor/selection; parity trace assertion required. |
| E-005 Toolbar and rich status | Compact toolbar duplicates common actions; status exposes current pane, sort, filter, selection/count and job progress without replacing TUI messages/hints. | INV-RENDER-005/009, INV-STATE-002..005, INV-files-ops-012. Risk: status clutter/omission; parity content visibility checklist. |

R3 [could] Tier 2, defer until matching areas are complete and tested:

| Proposed E | Enrichment and bounds | Exact parity dependencies / interaction risk |
|---|---|---|
| E-006 Optional split-view toggle | TUI has two panes and `+` toggles split; any drag-adjusted width is desktop-only, default widths remain TUI-like and split setting off/on remains the same action. | INV-STATE-001/009, INV-INPUT-003, INV-RENDER-002, INV-CONFIG-003. Risk: hiding/resetting pane state, width reducing rows. |
| E-007 Preview pane refinement | Rich image/text rendering may add zoom/scroll, but original preview mode cycle, exclusions, optional tool failure placeholders and editor entry remain intact. | INV-PE-001..009, INV-INPUT-012, INV-RENDER-008. Risk: incompatible decoding, resource load, text editor focus. |
| E-008 Bookmarks/places sidebar | Present same bookmark store and shortcuts; no second store, no auto-reorder. Collapsible and off by default if it reduces file area. | INV-CONFIG-004, INV-PLAT-003, INV-STATE-001/006. Risk: bytes/shortcut allocation mismatch. |
| E-009 Transfer progress center | Aggregate the same Copy Board jobs and preserve pause/cancel selection and behavior; optional notification is supplemental. | INV-INPUT-009, INV-files-ops-008..013, INV-RENDER-002. Risk: hiding UI must not pause/cancel job; verify existing behavior. |
| E-010 Settings UI | Only edit the same compatible theme/config keys; unknown/legacy lines and malformed-field tolerance preserved. | INV-CONFIG-001..006, INV-STATE-003/004, INV-PE-001. Risk: serialization drops unknown lines or changes precedence; byte round-trip tests required. |

R4 [could, last] Tier 3 after file operation, path, platform, and selection parity is verified: OS drag in/out and between panes as explicit copy-only operations (never implicit move), quick look, batch rename, archive browsing, multi-window. Dependencies: DnD E-011 → INV-PE-019, INV-files-ops-005/008/009/013 and platform review; quick look → INV-PE-001..009; batch rename → INV-INPUT-005/007 and INV-files-ops-006/007; archives → INV-filesystem-001/007; multi-window → INV-STATE-001, INV-ENTRY-003/004, INV-CONFIG-003. All must be off or absent until opted into; no animation on navigation, honor reduced motion. Each needs its own E id, compatibility tests, keyboard access, and adversarial interaction review.

## UX floor is not enrichment

Before completion gate, implement mouse, window lifecycle, native text input, OS integration, accessibility, themes, responsive async feedback and error/empty states per the floor in `desktop-ux-enrichment/SKILL.md` and `ux-layout.md`. Track them as parity UX-floor matrix rows, not as E-xxx. Existing terminal mouse no-op is not reason to omit useful desktop mouse support; its additive paths must converge on the same actions.

## Evidence and unverified boundaries

Relevant source refs include `src/handler.rs:19-361`, `src/ui/mod.rs:31-169`, `src/components/tab1_files_ui.rs:21-180`, `src/app.rs:2456-2595` and inventory citations in the tables. GPUI pin is 0.2.2; `uniform_list` and test contexts are confirmed in local locked source. Starter presently has no file manager UI (`desktop/src/main.rs`). There is no evidence yet for native macOS/Windows visuals, dialogs, drag/drop, text IME, or accessibility. GPUI/OS behavior and all listed enrichment dependencies remain unverified at runtime until corresponding platform and parity evidence is recorded.

Risks not covered: the discovery surface is extensive and some capabilities have medium confidence or source-only edge evidence; do not infer that an enrichment dependency has closed merely because the feature appears in inventory. The matrix status and reviewer evidence govern start eligibility.
