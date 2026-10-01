# Blind rendering and visible-message derivation

Derivation fixed before consulting any migration inventory, parity matrix, surface contribution, or advisor report.

- Oracle commit: `1cad4ce43cc72d52d4cc4eef920e0da22cb69568` (`tui-oracle-baseline` tag points here).
- Scope: `src/ui/**`, `src/components/**`, `src/theme/**`, visible-message producers in `src/app.rs`, and runtime rendering evidence attempt.
- Runtime status: frozen checkout was exported to `/tmp`; `cargo build --bin ira` failed before executable creation with `failed to write /tmp/target/.../invoked.timestamp: Disk quota exceeded (os error 122)`. Existing `target/release/ira` is built from the live checkout (`target/release/ira.d` names `/home/vlad/Projects/ira/src/...`) and was not treated as frozen-oracle evidence. No runtime screens are claimed. tmux is installed. A 120x40 pty launch/help/dialog/error/resize capture therefore remains unverified.

## Independently derived visible surface (static evidence)

| Surface / exact text or rule | Source refs | Evidence and qualification |
|---|---|---|
| Minimum-size gate: below 90 columns OR 15 rows, replace app with centered `Please increase the terminal's size` in the titled IRA panel. | `src/app.rs:943-945`; `src/ui/mod.rs:31-39` | Static only; 90x15 boundary and smaller-size behavior need frozen runtime capture. |
| Main title: `IRA (Integrated Retro Archives)` (spaces padded in title string); vertical rows drives 3, common folders 3, bookmarks/actions 3, files min 2, status 1. | `src/ui/mod.rs:41-58`; title `src/ui/mod.rs:31-35` | Static. Split panes divide files 50/50, except text preview pane gets 66%; copy board reserves 48 columns (`src/ui/mod.rs:140-168`). |
| Drives panel title `Drives`; each enumerated drive has 1-based numeric shortcut, icon and label; title hint `- unmount the current drive`; absent drives render `no drives`. | `src/components/drives_ui.rs:13-49` | Static; drive list timing/order/data from service not rederived here. |
| Common folders panel title `Common folders`; shortcuts/labels shown; absent list: `No common folders found`. | `src/components/common_folders_ui.rs:13-54` | Static. |
| Bookmarks panel title `Bookmarks`, shortcuts and labels; when `None`, no placeholder body is rendered. | `src/components/bookmarks_ui.rs:13-45` | Static; empty-vs-None state not runtime checked. |
| Actions panel title `Actions · <theme label>`; hints `+ Split Pane`, `` ` Copy Board ``, `0 Terminal`, `\ Theme`. | `src/components/actions_ui.rs:12-34` | Static. |
| Bottom normal-mode contextual hints in order: `↑/↓ move`, `←/→ open/leave`, `c copy`, `m move`, `. hidden`, `, sort`, `v preview`, `b bookmark`, `n new`, `/ search`. Empty while modal/input/focus/notice owns it. | `src/app.rs:918-937`; `src/ui/mod.rs:68-123` | Static. Overlong line uses cyclic character marquee; transient notice supersedes contextual hints. `* Keybindings` pill has fixed label plus theme-dependent width (`src/ui/mod.rs:68-123`, `src/ui/chrome.rs:24-113`). |
| File-pane modes: grid, column preview, details, plain list; inactive pane has no cursor and dim content; title shows label + path, optionally `/query`, `(Esc clears)`, `(details)`. | `src/components/tab1_files_ui.rs:16-49,51-77,171-213,688-724` | Static. Loading row exact text `Loading…` (`:97-109`). List row mark `[ ]`/`[*]`; details include size and relative time, directories size `—` (`:282-300` and later detail formatter). |
| Help overlay title `Keybindings`; 13 rows, exact visible bindings/descriptions: `Arrows — navigate; ←/→ open/leave`; `z / x — top / bottom`; `Space — multi-select entry`; `c — copy to other pane`; `m — move to other pane`; `Enter — rename entry`; `Del — delete (with confirm)`; `n — new folder / file`; `. — toggle hidden files`; `, — cycle sort mode`; `v — cycle image preview`; `Tab — switch pane`; `Ctrl+A — select / clear all`; `Alt+I — invert selection`; `+ — split pane`; `` ` — copy board ``; `b — bookmark this folder`; `/ — fuzzy search`; `Esc — clear search filter`; `? — entry info`; `[ — go to path`; `] — copy folder path`; `0 — terminal here`; `- — file browser`; `Ctrl+- — eject drive`; `* — this help`; `q — quit`. | `src/ui/mod.rs:171-245`; key content layout `src/ui/chrome.rs:135-165` | Static. Help comment says any key closes in handler; runtime not verified. 13 rows + border sized for minimum 90x15. |
| Delete progress dialog title `Deleting`; line `Deleting {done}/{total} — {basename} `; hint `any key hide (deletion continues)`. | `src/ui/mod.rs:248-270` | Static; dismiss does not cancel. |
| Confirm overlay title `Confirm`; delete subject `Delete {label}?`; copy/move subject `{verb} {label} → {dest_name}?`; keys `y es`, `n o`; copy/move also `o if exists: auto-rename|overwrite|skip`. Delete is danger-colored; copy/move confirm-colored. | `src/ui/mod.rs:273-320`; dialog chrome `src/ui/chrome.rs:167-202` | Static; source composes text from spans, so spaces are significant. |
| Rename modal title `Rename`; text cursor; hints `Enter rename`, `Esc cancel`. | `src/ui/mod.rs:323-331` | Static. |
| Go-to-path modal title `Go to path`; hints `Enter go / create`, `Esc cancel`. | `src/ui/mod.rs:333-343` | Static. |
| New modal title `New`; editable text, live preview `new folder`, `new file (.<ext>)`, or `new nested folder(s) + file`; hints `Enter create`, `Esc cancel`. | `src/ui/mod.rs:345-365` | Static. Extension classification excludes leading-dot only and path has slash semantics in this renderer. |
| Multi-selection Info dialog: `{n} folders selected`, `{n} files selected`, or `{f} folders / {fl} files selected`; size line `Size: {data} data / {on_disk} on disk ({items} items)` or `Size: {spinner} {human(data)} data / {human(on_disk)} on disk — calculating…`; hint `any key close`. | `src/ui/mod.rs:367-403` | Static; dynamic data/time unverified. |
| Entry Info dialog title `Info`; metadata from `Info.lines`, dynamically inserts size line; for folders hints `x cancel size walk`, `r recalculate`, `Esc close`. | `src/ui/mod.rs:405-443`; size text helpers `src/services/file_info.rs` | Static composition; metadata line contents should be separately inventoried from producer. |
| Error overlay title `Error`; displays `status.text` bold, blank row, hint `any key dismiss`; wraps long text at width capped to 4/5 terminal width, with minimum content width 30; error status is modal, non-error status is bottom bar. | `src/ui/mod.rs:445-483`; status data `src/app.rs:144-156,797-855` | Static; no runtime wrap capture. |
| Non-error status bottom bar exact prefix ` ●  ` and suffix space; background info color. Status expires after TTL (status producer state at `src/app.rs:144-160,797-855`). | `src/ui/mod.rs:59-66` | Static; precise TTL value and visual colors are theme-dependent. |
| Modal chrome is rounded `╭╮╰╯`/`─│`, centered, dimmed backdrop, shadow and frost ring; titles include padding. Help is a plain panel, not glass dialog. | `src/ui/chrome.rs:167-202,220-260,280-340` | Static. Chip variants Square/Rounded/Outline; Unicode Nerd fallback parentheses in outline mode (`:24-80`). |
| File grid fixed cell 20x9 (8 image rows + name), adjacent-screen thumbnail prefetch; errors show `✕`, pending `…`; selected cell cursor colors. | `src/components/tab1_files_ui.rs:400-418,521-570` | Static; glyph/icon coverage and terminal graphics protocol outside rendered text not runtime checked. |
| Details mode has size and relative-time columns; sample unit expectations: `1.5 MiB`, `3 days ago`; relative times include `just now`, `2 minutes ago`, `1 hour ago`, `2 days ago`, years as 1-decimal under 10 and integer from 10. | `src/components/tab1_files_ui.rs:171-194,797-843` | Unit assertions in frozen source, not executed here. |

## Visible-message producer sweep

All producers below set a transient status; `is_error=true` routes to the modal and `false` to the info-colored bottom bar. The wording may include OS error text and thus has variable suffixes.

| Producer strings/patterns | Source refs |
|---|---|
| `open failed: {e}`; `file too large to edit (> 5 MB)`; `binary file — not editable`; `non-UTF-8 file — read-only preview only`; `{name} is read-only`; `file path changed on disk — press Esc and reopen`; `file changed on disk — press Esc and reopen`; `save failed: {e}`; `Saved {name}` | `src/app.rs:1239-1437` |
| `Failed to mount {device}: {err}`; `Failed to eject {device}: {reason}`; eject-busy message begins `Failed to eject` | `src/app.rs:2099-2152` |
| `Preview: {label}`; `Switched to {theme label}` | `src/app.rs:1903`, `2284-2290` |
| Sorted notices: `Sorted by size (largest first)`, `Sorted by last modified (newest first)`, `Sorted by kind`, `Sorted by name` | `src/app.rs:2404-2410` |
| `No folder open to start a terminal in.`; `No folder open to reveal.`; `The other pane has no folder to copy into.`; `Cannot copy/move a folder into itself.` | `src/app.rs:2534-2609` |
| `Folder path copied to clipboard`; `Failed to copy the folder path to the clipboard.` | `src/app.rs:2787-2789` |
| `Enter a name first.`; `'{name}' already exists.`; `'{name}' already exists and is not a folder.`; `Failed to create '{name}': {err}` / `Failed to create '{path}': {err}` | `src/app.rs:2702-2734,2845-2903` |
| `Cannot rename: '{original}' already exists.`; `Failed to rename: {e}` | `src/app.rs:3018-3039` |
| `Failed to delete '{path}': {err}` plus ` (+{n} more)` | `src/app.rs:3659-3678` |
| `No free bookmark shortcut available (a-p are taken)` | `src/app.rs:3925-3932` |

This is a source search, not a claim that it exhausts every error in the application: direct panics, service errors, command output, and asynchronous failure paths need dedicated review. `src/ui/mod.rs` has tests for error wrapping, dismiss affordance, notice bottom bar and modal hint suppression (`:540-725` and `:870-910`); tests were not run.

## Files inspected

`src/ui/mod.rs`, `src/ui/chrome.rs`, `src/components/{actions_ui,bookmarks_ui,common_folders_ui,copy_board_ui,drives_ui,preview_ui,tab1_files_ui}.rs`, `src/theme/{mod,caps,font_probe,icons,wt_font}.rs`, `src/app.rs`, `src/services/file_info.rs`, `src/main.rs`, `Cargo.toml`. Commit-based reads used `git show`/`git grep` on the frozen commit. `tmux` exists; no baseline executable was successfully built/launched, so startup/help/dialog/error and 120x40/90x15/undersized renderings are absent.

## Unclaimed or uncertain

- Full exact text generated by `preview_ui`, `file_info`, theme/font/icon service, transfer, and filesystem producers was not exhaustively enumerated; file contents must be checked for parity coverage.
- Runtime startup content depends on environment, initial folder, state, drives and font/image protocol. No frozen runtime capture; layout clipping, key-close behavior and status TTL remain static-only.
- TUI initialization and terminal restore side effects live in `src/tui.rs`, outside the rendering visual surface; stdout/stderr protocol behavior is not captured.
- No claims of absence are made for messages/widgets based only on this partial source path survey.

## Cross-check after derivation was fixed

Compared only after the preceding independent derivation was written:

- `migration/inventory/rendering-messages.md` (INV-RENDER-001 through 009, plus its message catalog), `migration/oracle/captures/rendering-messages/startup-help.txt`, and the already-existing `migration/oracle/captures/entry-input/keybindings.txt`, `migration/oracle/captures/phase0/initial-screen.txt`, and `migration/oracle/captures/phase0/new-dialog.txt`.
- Coverage matches: minimum-size gate/layout, drives/common folders/bookmarks/actions, split/copy-board/preview layout, file rows/grid/details, theme/chips/hints/help, modal chrome and all dialog types, status/error routing, plus the runtime startup/help/error facts where source derivation and capture overlap.
- Independent capture corroborates the initial empty file pane, populated top rows, help content, help any-key consumption, error `Enter a name first.`, centered red Error modal, timeout after the 8-second lifetime, and clean exit/status 0. The error dismissal by key is explicitly not runtime-proven by that capture; our derivation likewise did not claim it was.
- Existing inventory is materially broader on details omitted by my intentionally narrow blind rendering pass: exact theme preset ordering/overrides; copy-board state/progress strings; additional preview and metadata messages; all status producer templates, including no mounted removable drive and missing terminal/file-browser messages; more detailed preview/file-size behavior; tests and handler routing. These should be retained as further source-derived candidates, with runtime qualification as stated in that inventory.

### Differences and conflicts to resolve

1. **My derivation under-covered producer strings.** It does not enumerate the two inventory entries `No removable drive is mounted at {folder_path}` and `No terminal emulator found (tried {tried}).`, nor `No file browser found (tried {tried}).`; the inventory’s broader producer list and refs are consistent with source callsites. Likewise its preview fallback/message catalog exceeds mine. Treat these as omissions in this blind report, not contradictions.
2. **Runtime status claims need careful phrasing.** The capture records the error modal disappearing after the 8-second TTL and says key dismissal was not independently verified. `INV-RENDER-009` correctly calls key dismissal source/test-backed but unverified live. Do not convert this into a verified key-dismiss runtime claim.
3. **Help close evidence is consistent.** Capture says both `q` and `x` are consumed by an open help modal; source/handler behavior states any key closes. My initial derivation's comment claim is source-only, and the capture is the runtime evidence.
4. **No small-terminal or varied-layout runtime evidence in the reviewed rendering capture.** Inventory acknowledges missing small-screen/varied-state capture. This remains an evidence gap; the frozen build attempt in this blind pass failed due disk quota, so I did not produce a second independent oracle run.

## Post-derivation inspection log correction

Files actually opened/read during my blind pass: `src/ui/mod.rs`, `src/ui/chrome.rs`, `src/components/actions_ui.rs`, `bookmarks_ui.rs`, `common_folders_ui.rs`, `copy_board_ui.rs`, `drives_ui.rs`, `tab1_files_ui.rs`, `src/app.rs`, and `src/main.rs`; targeted `git grep` over `src` for status producers. I did not fully read `preview_ui.rs`, `src/theme/**`, or `src/services/file_info.rs` during blind derivation. Their details above are from the post-derivation comparison with the existing inventory and are labeled as such. Runtime artifact files were opened only after the derivation was fixed.
