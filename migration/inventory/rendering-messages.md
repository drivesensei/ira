# Inventory: rendering, widgets, errors, and messages
Explorer run: 2026-10-01 | TUI commit: `1cad4ce43cc72d52d4cc4eef920e0da22cb69568` | Files claimed: `src/ui/mod.rs`, `src/ui/chrome.rs`, all `src/components/*_ui.rs`, `src/theme/mod.rs`, `src/theme/icons.rs`, `src/services/file_info.rs`; rendering/status call sites in `src/app.rs`, `src/handler.rs`.

Static source was read from the working tree whose TUI source is frozen at `tui-oracle-baseline`. `src/app.rs` status producers are inventoried here only for visible message behavior; underlying operation semantics belong to other slices.

## Features
### INV-RENDER-001 Fixed screen layout and minimum size fallback
- Trigger: every render.
- Behavior: background uses theme text/background; rows are Drives (3), Common folders (3), Bookmarks+Actions (3), Files (minimum 2), status/hints (1). If `App::should_increase_size(width,height)` is true, all other content is replaced by centered `Please increase the terminal's size` in title panel. Root render starts at `src/ui/mod.rs:19-62`.
- State read/written: dimensions, app/theme, thumbnail frame cap.
- Source refs: `src/ui/mod.rs:19-62`.
- Runtime evidence: `migration/oracle/captures/rendering-messages/startup-help.txt`; actual 120x40 PTY first-run; startup takes ~12s due terminal capability probe. Too-small runtime probe not captured.
- Edge cases: help claims minimum 90x15; small-screen exact rendering has unit test `keybindings_dialog_fits_minimum_supported_terminal` in `src/ui/mod.rs:851`.
- Error behavior: terminal size message replaces full UI.
- Config knobs: theme/chip style affects widths.
- Platform notes: terminal cells/ANSI and glyph support.
- Depends on: all visual state features.
- Confidence: high (source + live screen; low for resize probe).

### INV-RENDER-002 Component rows, pane split, copy-board and preview columns
- Trigger: every render; split `+`, Copy Board `` ` ``, per-pane `v` actions change layout.
- Behavior: drive/common-folder/bookmark/action components occupy top rows; copy board takes 48 columns from file area; split divides remaining area 50/50 except text-preview pane receives 66%; file render delegated per-pane. Source includes conditional drives/common-folder empty content and copy-board status/progress rows.
- State read/written: active pane, split, copy-board, preview mode, app lists/jobs.
- Source refs: `src/ui/mod.rs:50-169`; `src/components/drives_ui.rs:13-55`, `common_folders_ui.rs:13-53`, `bookmarks_ui.rs:13-50`, `actions_ui.rs:15-34`, `copy_board_ui.rs:15-149`, `preview_ui.rs:21-216`, `tab1_files_ui.rs:21-718`.
- Runtime evidence: startup screen in `migration/oracle/captures/rendering-messages/startup-help.txt`; varied layout states not dynamically captured here.
- Edge cases: pane width rebalances for text editing/preview; empty common-folder text exact: `No common folders found`; file list and detail/grid renderers have separate empty/loading/scroll paths.
- Error behavior: copy-board maps failed job to `[!]` and `error`; preview surfaces load failures in its cell.
- Config knobs: theme and icon presentation.
- Platform notes: display path labels can be lossy; specific file ops elsewhere.
- Depends on: pane/state, job list, preview, theme.
- Confidence: medium (code read; startup runtime only).

### INV-RENDER-003 File rows, truncation, detail formatting and grid rendering
- Trigger: file-list display mode.
- Behavior: list/details render a bounded visible window and selection state; labels use char-count truncation with `…` (not terminal display-width-aware); detail row has fixed name/size/relative-modified columns; relative time selects seconds/minutes/hours/days/months/years. Grid has image cells, error `✕`, loading `…`, and overlay image tiles.
- State read/written: pane files, cursor, scroll, selection, sort/filter/listing state, thumbnail cache.
- Source refs: `src/components/tab1_files_ui.rs:51-180,182-328,329-408,410-611,612-719`; `src/components/preview_ui.rs:119-146`.
- Runtime evidence: startup file area initially empty because no pane folder is selected; details/list fixture interaction not captured in this explorer.
- Edge cases: `truncate_chars("0123456789abc",10) == "012345678…"`; empty and zero-height windows are covered by unit tests at `src/components/tab1_files_ui.rs:932-934,1086-1092`. Unicode wide graphemes can consume more terminal cells than char-count estimate (inference from implementation).
- Error behavior: image/render failure placeholder; preview loading/error status in its preview pane.
- Config knobs: list/details/grid view state and icon settings.
- Platform notes: terminal display-cell width differs from char count.
- Depends on: file listing, preview, selection and theme.
- Confidence: medium (source/tests; not all modes runtime exercised).

### INV-RENDER-004 Theme palette, semantic file colors, and chip styles
- Trigger: startup/config theme load; `\\` cycles preset; theme config supplies overrides.
- Behavior: theme contains semantic text/background/border/accent/key/status/cursor/selection/file-category colors; presets in cycle order: Catppuccin Mocha, Cyberpunk 2077, Gruvbox Dark, Nord, Dracula, Tokyo Night. Color overrides parse Ratatui colors, invalid values ignored. Chip style square/rounded/outline changes key affordance rendering and fixed widths; outline fallback uses parentheses without Nerd glyphs.
- State read/written: selected preset and theme override file (persistence slice owns exact file format).
- Source refs: `src/theme/mod.rs:12-404,412-768`; `src/ui/chrome.rs:14-93`; action label `src/components/actions_ui.rs:15-20`.
- Runtime evidence: startup screen shows `Catppuccin Mocha`; no theme change capture in this slice.
- Edge cases: indexed colors quantize where RGB unsupported; unknown/unparseable override ignored; unsupported Nerd glyph fallback.
- Error behavior: bad override is silently ignored by parser/apply path.
- Config knobs: theme preset/color keys, chip style, Nerd glyph bool.
- Platform notes: terminal color capability and Nerd Font availability.
- Depends on: config-state.
- Confidence: medium (source + startup).

### INV-RENDER-005 Chips, hints, marquee, and keybindings help
- Trigger: normal-mode footer each frame; `*` opens help, any key closes.
- Behavior: fixed right-side `* Keybindings` pill; contextual hints occupy remaining width and scroll cyclically one character by `hint_offset` when long. Hint chip separators are 3 spaces except rounded chips use 2. Help is centered plain panel, 13 binding rows; documents navigation, selection, operations, help, terminal/file-browser/eject. Help uses any-key dismissal.
- State read/written: hint offset, modal keyboard ownership, keybindings visibility.
- Source refs: `src/ui/mod.rs:89-119,171-246`; `src/ui/chrome.rs:97-197`; `src/handler.rs:120-125`; test `src/ui/mod.rs:719-767`.
- Runtime evidence: `migration/oracle/captures/rendering-messages/startup-help.txt`, 120x40 exec PTY, `*`; screen displays centered Keybindings table.
- Edge cases: contextual hints suppressed if status exists or modal/text prompt blocks keyboard; minimum-size help fit tested at 90x15. Keybindings table is partly a manually curated list; not a full registry.
- Error behavior: none.
- Config knobs: chip style/theme.
- Platform notes: arrows/glyphs rely on terminal rendering.
- Depends on: input bindings/modes, theme.
- Confidence: high (runtime + source/tests).

### INV-RENDER-006 Modal chrome and text input cursor
- Trigger: any confirmation, input, progress, info, or error modal.
- Behavior: glass dialog dims backdrop, adds bottom/right shadow, frost ring, tinted semantic border, rounded corners, title clipped by chars; content wraps with `trim:false`. Centering clamps rect to frame. Input cursor is block cell; cursor at end gets trailing block, empty input shows a block.
- State read/written: terminal frame and theme.
- Source refs: `src/ui/chrome.rs:200-389`.
- Runtime evidence: help is plain panel; initial screen. Modal-specific dynamic examples below.
- Edge cases: tiny rectangles avoid border drawing; dimensions saturate. Long messages wrap; `long_error_messages_wrap_instead_of_cutting` and `hint_line_fits_inside_the_dialog` unit tests in `src/ui/mod.rs:551-632`.
- Error behavior: semantic colors by kind Info/accent, Confirm+Progress/warning, Danger+Error/error, Input/active border.
- Config knobs: theme/palette and chip style.
- Platform notes: visual oracle requires terminal cells; proposed desktop equivalent is native modal/panel with same content, intent, colors semantic only.
- Depends on: theme and modal state.
- Confidence: high (source + tests).

### INV-RENDER-007 Confirmation, rename, goto, new, info, multi-info and deletion-progress widgets
- Trigger: operation state creates dialog in app; see input/ops inventory.
- Behavior: Confirmation subjects are `Delete|Copy|Move <label>?` or `... → <destination basename>?`; copy/move show overwrite policy `auto-rename|overwrite|skip`, `o` cycles; `y`/Enter confirms and `n`/Esc cancels. Rename shows Enter/Esc. Go-to-path says `Enter go / create`; New live kind reads `folder`, `file (.<ext>)`, or `nested folder(s) + file`. Metadata Info includes dynamic size walk hints; multi-info sums sizes. Deleting shows `Deleting done/total — current` and “any key hide (deletion continues)”.
- State read/written: confirmation/new/rename/goto/info/multi-info/deletion states.
- Source refs: `src/ui/mod.rs:248-443`; dismissal/action routing `src/handler.rs:93-185`.
- Runtime evidence: dialog layouts asserted by `src/ui/mod.rs` tests; blank-name create error observed live in `migration/oracle/captures/rendering-messages/startup-help.txt`; other dialogs not exercised.
- Edge cases: long dynamic labels may wrap/clamp; deletion progress dismissing only hides dialog, does not cancel worker; metadata folders show x cancel/r recalculate/Esc close.
- Error behavior: creation/rename/input errors are status messages (INV-RENDER-009).
- Config knobs: theme.
- Platform notes: destination basename uses lossy string conversion.
- Depends on: file ops, selection, input, size worker.
- Confidence: medium (code+tests).

### INV-RENDER-008 Preview panes, file metadata and size formatting
- Trigger: preview/list/info requests.
- Behavior: preview cells distinguish `(empty file)`, `loading…`, `loading <label>…`, truncated text `… truncated`, and error color `✕`/status. Metadata lines include Name, Path, Kind, Size, Modified, Added as available; unreadable metadata exact line `Error reading metadata`. Size uses decimal units (B, KB, MB, GB, TB, PB) and directory aggregation formats data/on-disk/items, animated spinner while walking, lower-bound partial after cancel.
- State read/written: cached preview/metadata/size walks.
- Source refs: `src/components/preview_ui.rs:21-216`; `src/services/file_info.rs:58-294,360-390`; dialog rendering `src/ui/mod.rs:367-443`.
- Runtime evidence: only initial screen; implementation/unit tests cover values.
- Edge cases: preview has truncated flag and binary distinction; large/non-text outcomes belong preview implementation. Folder-size partial is explicitly lower-bound.
- Error behavior: metadata failure is a content line (not modal error); preview failures have preview-specific display.
- Config knobs: preview settings elsewhere.
- Platform notes: metadata timestamps printed UTC; file path display conversion occurs in service.
- Depends on: preview and metadata services.
- Confidence: medium.

### INV-RENDER-009 Status notice/error channel and message catalog
- Trigger: `App::set_status(text,is_error)` from operation/action paths.
- Behavior: status state has transient text+error bit and expires after 8 seconds (`STATUS_TTL`). Non-error renders in bottom info-colored bar prefixed ` ●  `; contextual hints are suppressed while status exists. Error renders centered red modal titled `Error` and “any key dismiss”; handler clears it before other modal routing. Tests ensure a later success notice replaces an old error and status button stays visible.
- State read/written: `App.status` / `set_status` / `clear_status`.
- Source refs: `src/app.rs:798-812` (confirm with line read), `src/ui/mod.rs:66-86,445-484`; `src/handler.rs:127-135`; tests `src/ui/mod.rs:551-594,704-717`, `src/app.rs:4472-4480,5975-5985`.
- Runtime evidence: error modal text, color/shape, and location are captured live in `migration/oracle/captures/rendering-messages/startup-help.txt`; dismissal-by-key is source/test-backed but unverified live. Notice display has TestBackend evidence only.
- Edge cases: error wraps to 80% frame width; a newer notice overwrites prior status. Error modal takes precedence over info dialog in input handler.
- Error behavior: class and exact messages below; OS error fragments are dynamic.
- Config knobs: theme colors.
- Platform notes: messages include path/device strings, currently Strings/lossy paths.
- Depends on every action that emits status.
- Confidence: high for mechanism; medium for exhaustive catalog.

#### Observed/static status strings and classes
These exact literals/template forms come from status producers in `src/app.rs`; not all were induced at runtime. Dynamic `{e}`, `{err}`, `{reason}`, `{name}`, `{path}`, `{device}`, `{parent}` are OS/action-dependent.

| Class | Exact string/template | Producer |
|---|---|---|
| Error | `open failed: {e}` | app.rs:1239,1249,1256,1270 |
| Error | `file too large to edit (> 5 MB)` | 1261,1274 |
| Error | `binary file — not editable` | 1278 |
| Error | `non-UTF-8 file — read-only preview only` | 1284 |
| Error | `{name} is read-only` | 1370 |
| Error | `file path changed on disk — press Esc and reopen` | 1380 |
| Error | `file changed on disk — press Esc and reopen` | 1393 |
| Error/success | `save failed: {e}` / `Saved {name}` | 1414,1437 |
| Notice | `Preview: {label}` | 1903 |
| Error | `Failed to mount {device}: {err}` | 2099 |
| Error | `No removable drive is mounted at {folder_path}` | 2133 |
| Error | `Failed to eject {device}: {reason}` | 2152 |
| Error | dynamic `msg` from device watcher | 2226 |
| Notice | `Switched to {theme label}` | 2290 |
| Notice | dynamic `notice` (selection/sort/filter etc.) | 2410 |
| Error | `No folder open to start a terminal in.` | 2534 |
| Error | `No terminal emulator found (tried {tried}).` | 2540 |
| Error | `No folder open to reveal.` | 2569 |
| Error | `No file browser found (tried {tried}).` | 2575 |
| Error | `The other pane has no folder to copy into.` | 2597 |
| Error | `Cannot copy/move a folder into itself.` | 2609,3387 |
| Error | `Failed to create '{parent}': {err}` / `Failed to create '{path}': {err}` | 2706,2730 |
| Notice/error | `Folder path copied to clipboard` / `Failed to copy the folder path to the clipboard.` | 2787,2789 |
| Error | `Enter a name first.` | 2855 |
| Error | `'{name}' already exists.` | 2867,2895 |
| Error | `'{name}' already exists and is not a folder.` | 2897 |
| Error | `Failed to create '{name}': {err}` | 2873,2900 |
| Error | `Cannot rename: '{original}' already exists.` | 3031 |
| Error | `Failed to rename: {e}` | 3039 |
| Error | `Failed to delete '{path}': {err}{more}` | 3677 |
| Error | `No free bookmark shortcut available (a-p are taken)` | 3930 |

Service-level fallback/content messages also include `Error reading metadata` (`src/services/file_info.rs:245`), `No common folders found` (`src/components/common_folders_ui.rs:47-53`), preview ` (empty file) ` (`preview_ui.rs:119-123`), ` … truncated` (`preview_ui.rs:134-138`), ` cannot decode ` (`preview_ui.rs:166-175), and no-selection/type/dependency reasons (`preview_ui.rs:194-219): ` select a file `, ` folders have no image preview `, ` install ffmpeg for video previews `, ` install ffmpeg for HEIC previews `, ` install poppler (pdftoppm) for PDF previews `, ` format not supported (png/jpg/gif/bmp/webp/mp4/mov/heic/pdf) `. Text preview loading is ` loading… `; image preview loading is ` loading {label}… `. Transfer/job failure text is separate from this list and needs a dedicated error sweep with the operations inventory.

## Unclaimed or uncertain
- `src/services/transfer.rs`, `src/services/list_files.rs`, `src/services/drives.rs`, and `src/services/state.rs` emit/shape data that affects UI; underlying behavior belongs to file operations, filesystem, or config-state inventories.
- `src/app.rs` has many user-visible operation-specific status strings; catalog above enumerates all literal `set_status` occurrences found by `rg` in claimed area but dynamic `msg`/`notice` builders and worker errors require extraction from non-render slices.
- Runtime checked initial first-run 120x40 screen, help modal and blank-name create error. Small terminal, remaining dialogs, themes, previews, other view modes require live probes.
- `src/ui/mod.rs` includes existing unit test module; no production tests written.
