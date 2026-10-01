# Inventory: configuration, persisted state, and core browsing state
Explorer run: 2026-10-01 | TUI commit: `1cad4ce43cc72d52d4cc4eef920e0da22cb69568` (`tui-oracle-baseline^{}`) | Files claimed: `src/services/state.rs`, `src/services/bookmarks.rs`, `src/theme/mod.rs`, `src/theme/caps.rs`, `src/theme/icons.rs`, `src/domain/data.rs`, `src/app.rs`, `src/handler.rs`, `src/services/list_files.rs`, `src/utils/fuzzy.rs`, `src/main.rs`

## Features
### INV-CONFIG-001 User configuration roots and supported files
- Trigger: startup / preference reads and writes.
- Behavior: all three user files are resolved under `dirs_next::config_dir()/ira/`: `state`, `bookmarks`, and `theme.toml`. The function is platform-selected by `dirs-next`; on this Linux run an explicit `XDG_CONFIG_HOME` resolved to `<XDG_CONFIG_HOME>/ira/theme.toml`. There is no separate generic config file, keymap file, per-directory config, or hot reload in the claimed code.
- State read/written: `state`, `bookmarks`, `theme.toml` (read-only theme input).
- Source refs: `src/services/state.rs:54-56`; `src/services/bookmarks.rs:32-35`; `src/theme/mod.rs:617-620, 481-488`.
- Runtime evidence: `migration/oracle/captures/config-state/theme-toml-check.txt` shows the resolved temp XDG location; `first-run.txt` was launched from an isolated fixture with empty config.
- Edge cases observed: inability to resolve a config directory makes state/bookmark writes no-op and reads default/empty; read/write errors are suppressed.
- Error behavior: no user-facing config I/O error.
- Config knobs: OS config-directory convention; `XDG_CONFIG_HOME` on Linux.
- Platform notes: macOS and Windows path resolution is delegated to `dirs_next::config_dir()` and was not dynamically exercised in this Linux run.
- Depends on: INV-CONFIG-002, INV-CONFIG-004, INV-CONFIG-005.
- Confidence: high for source contract and Linux path; medium for unrun OS-specific locations.

### INV-CONFIG-002 Session state file format, defaults, and tolerant parsing
- Trigger: app startup loads `state`; normal exit writes it; hidden/theme toggles also persist immediately.
- Behavior: UTF-8 `key=value` lines, no schema/version header. Recognized scalar keys: `split` (`1` true), `active` (usize, malformed => 0, clamped to 1), `hidden` (`1` true), `preview0` / `preview1` (u8, malformed => 0, clamped to 3), `theme` (recognized preset/alias only), `left` / `right` (`label<TAB>path`, also tolerates a bare legacy path), and repeatable `size` records. Serialization writes scalar fields then optional theme, left/right, and all size entries. A size line is `size=<bytes><TAB><items><TAB><on_disk><TAB><complete 0|1><TAB><updated_epoch><TAB><path>`; path is last so spaces/`=` are allowed, tab/newline are not. Unknown keys and malformed records are ignored; parser trims each whole line. Folder entries with empty value/path are ignored. Repeated recognized keys overwrite scalar/folder fields; repeated valid `size` lines accumulate. Missing/unreadable file returns defaults: split false, active pane 0, hidden false, no folders/theme/sizes, previews `[3,3]` (details).
- State read/written: state fields listed above; complete folder-size cache records only are persisted.
- Source refs: `src/services/state.rs:18-52, 54-89, 91-203`; `src/app.rs:3939-3967, 3985-4025`.
- Runtime evidence: `seeded-session.txt` shows state-provided split folders and hidden `.hidden`; `seeded-check-terminal.txt` shows `theme=Tokyo_Night` normalized/resolved to Tokyo Night despite TOML preset `nord`. Direct-PTY `pty-state-after-q.txt` records clean-quit state, and `pty-script-exit.txt` records exit code 0. The seeded fixture contains unknown and size records.
- Edge cases observed: state write creates parent directory, overwrites directly (not atomically), and suppresses errors; bad `complete` values other than `0`/`1`, empty size path, or missing fields discard the size record. Unknown legacy `icons=` lines are ignored. Theme aliases are canonicalized on parse/save.
- Error behavior: malformed/missing state silently becomes default or partial state.
- Config knobs: exact keys above.
- Platform notes: paths stored as UTF-8 `String`, unlike native `PathBuf`; lossy/native path fidelity is a migration risk.
- Depends on: INV-CONFIG-005, INV-STATE-001.
- Confidence: high; existing baseline unit tests `default_state_opens_in_details_mode`, `parses_label_path_pairs_and_bare_paths`, `size_entries_roundtrip_through_the_state_format`, `legacy_icons_line_is_ignored_and_not_rewritten`, `theme_key_normalizes_aliases_and_drops_unknown`, `malformed_size_lines_are_ignored` cover parser details in `src/services/state.rs`.

### INV-CONFIG-003 Session contents and persistence timing
- Trigger: restart/quit, hidden toggle, theme cycle.
- Behavior: session restores split flag, active pane, hidden visibility, two pane folders, preview mode per pane, selected theme, and complete size measurements. It does not persist selected row, multi-selection, scroll, sort mode, search/filter query, folder navigation history, or a tab collection. Theme cycling (`\`) and hidden toggle (`.`) save immediately; the main event loop persists state after clean quit. `persist_state` serializes the currently active in-memory state.
- State read/written: the `state` keys above.
- Source refs: `src/app.rs:319-327, 3939-3967, 3985-4025`; `src/handler.rs:283-285, 320-322`; `src/main.rs:69-73`.
- Runtime evidence: `seeded-session.txt` restores both distinct fixture paths and displays the hidden entry in the split view; `seeded-check-terminal.txt` proves stored theme preference wins over TOML; `pty-state-after-q.txt` proves clean quit saves current left folder `fixture`, active pane `0`, and hidden flag `1`.
- Edge cases observed: invalid/missing restored paths are still assigned as folders; later listing behavior handles the path. Partial size walks are omitted from persisted state.
- Error behavior: persistence failures are silent.
- Config knobs: `state.split`, `state.active`, `state.hidden`, `state.left`, `state.right`, `state.preview0`, `state.preview1`, `state.theme`, repeatable `state.size`.
- Platform notes: no native path serialization.
- Depends on: INV-CONFIG-002, INV-STATE-001, INV-STATE-003, INV-STATE-004.
- Confidence: high; direct PTY captured immediate hidden-toggle write plus clean-quit persistence and successful process exit.

### INV-CONFIG-004 Bookmarks persistence and shortcut allocation
- Trigger: `b` toggles the current folder bookmark; a matching bookmark shortcut jumps to it.
- Behavior: `~config/ira/bookmarks` is UTF-8 plain text in saved order, one `label<TAB>path` per line. Blank lines are skipped. A line without a tab is treated as a legacy bare path; label derives from final path component, or the original line if no UTF-8 component. No escaping/versioning. On load, shortcuts are reallocated in QWERTY order, reserving common-folder letters `w,e,r,t,y,u,i` and action letters `q,c,z,x,b,m,v`, yielding `o,p,a,s,d,f,g,h,j,k,l,n`; max 12 loaded entries. Adding a new bookmark gets first free key; toggling existing path removes first matching entry; writes replace the whole file and errors are ignored.
- State read/written: bookmark label/path sequence; shortcuts are derived, not persisted.
- Source refs: `src/services/bookmarks.rs:6-75`; `src/app.rs:3887-3937`; `src/handler.rs:269-270, 329-338`.
- Runtime evidence: `pty-bookmark-added.txt` shows shortcut `(o)` and label `fixture`; persisted line is `fixture<TAB><temp fixture path>`. `pty-bookmark-jump.txt` shows the pane back at the bookmark after Right enters `left` and `o` jumps to it. `pty-bookmark-removed.txt` and `pty-bookmarks-after-remove.txt` show the entry removed and file empty.
- Edge cases observed: paths/labels containing tabs/newlines are ambiguous; duplicate paths load and first matching path is removed.
- Error behavior: when all shortcut letters are occupied, status text is exactly `No free bookmark shortcut available (a-p are taken)` and no file rewrite occurs (`src/app.rs:3928-3933`).
- Config knobs: none.
- Platform notes: path and label are strings.
- Depends on: INV-CONFIG-001, INV-STATE-001.
- Confidence: high; add, jump, and remove were driven through a direct PTY against the throwaway fixture.

### INV-CONFIG-005 Theme preset source, aliases, and cycle
- Trigger: startup or `\` cycles built-in theme.
- Behavior: built-ins and cycle order: `mocha` (Catppuccin Mocha), `cyberpunk2077` (Cyberpunk 2077), `gruvbox-dark`, `nord`, `dracula`, `tokyo-night`; cycle wraps. Startup precedence is valid session `theme=` > valid `theme.toml` `preset` > default mocha. Parsing trims, lowercases, and changes `_` to `-`; aliases include `default`, `catppuccin`, `catppuccin-mocha`; `cyberpunk`, `cyberpunk-2077`, `2077`, `nightcity`, `night-city`; `gruvbox`, `gruvboxdark`; `tokyonight`, `tokyo`. Cycling shows `Switched to <label>` and immediately persists canonical ID.
- State read/written: `state.theme`; `theme.toml` `preset`.
- Source refs: `src/theme/mod.rs:321-400, 407-417, 553-562`; `src/app.rs:2284-2292`; `src/services/state.rs:138-143`.
- Runtime evidence: `theme-toml-check.txt` resolves `nord (from theme.toml preset)`; `seeded-check-terminal.txt` resolves `tokyo-night (from session state, last \\ press)` while same TOML says nord.
- Edge cases observed: unknown preset is ignored and lower-precedence source applies.
- Error behavior: invalid preset is silent; defaults apply.
- Config knobs: `theme.preset`, `state.theme`.
- Platform notes: palette is adapted to detected terminal capabilities; desktop app should preserve semantic palette, not terminal color quantization.
- Depends on: INV-CONFIG-002, INV-CONFIG-006.
- Confidence: high.

### INV-CONFIG-006 Theme TOML fields, colors, and icon environment override
- Trigger: startup theme loading.
- Behavior: optional `theme.toml` is parsed with serde/TOML defaults. Top-level optional keys: `preset`, `icons`, `chips`, `bg`, `surface`, `surface_alt`, `border`, `border_active`, `text`, `text_muted`, `accent`, `key_fg`, `key_bg`, `success`, `warning`, `error`, `info`, `cursor_bg`, `cursor_fg`, `selection`, `dir`, `hidden`, `shadow`; optional file-category colors live under `[files]` with `image`, `video`, `audio`, `archive`, `code`, `document`, `executable`, `data`. Unknown TOML fields are ignored. Invalid TOML causes the entire `ThemeFile` to default silently; an invalid color leaves that color's preset value. Color parser delegates to Ratatui `Color::from_str`. Chip aliases: square/plain/filled; rounded/pill; outline/outlined/border. Icon choice precedence: valid `IRA_ICONS` (`nerd|emoji|unicode`) > valid TOML `icons` > auto capability/font detection. Invalid/`auto` env value falls through. Theme file is read once at startup, not watched.
- State read/written: `theme.toml`; process env `IRA_ICONS`.
- Source refs: `src/theme/mod.rs:407-453, 481-503, 516-568, 617-620, 640-670, 758-805`; `src/theme/caps.rs:14`.
- Runtime evidence: `theme-toml-check.txt` reports `icons: unicode (from theme.toml icons)` and `theme: nord (from theme.toml preset)`; `theme-env-check.txt` reports `icons: emoji (IRA_ICONS override)` while retaining theme preset.
- Edge cases observed: unknown fields are tolerated; bad TOML disables all settings rather than partially parsing.
- Error behavior: absent/unreadable/invalid TOML is silent, then defaults/auto detection apply.
- Config knobs: all TOML keys above; `IRA_ICONS`.
- Platform notes: auto icon selection depends on detected terminal/font capability and is not a desktop preference model.
- Depends on: INV-CONFIG-001, INV-CONFIG-005.
- Confidence: high for parse/precedence source; medium for individual color rendering (not visually compared).

### INV-STATE-001 Two panes, active pane, and no tab/history stack
- Trigger: startup and `+` / Tab.
- Behavior: `App` owns exactly `[Pane; 2]`, `split`, and `active_pane`; no `Tab` collection or directory back/forward stack exists in claimed app/domain/services source. Split off uses pane 0 and active index 0. Opening split always focuses pane 0 and mirrors pane 0's folder, sort mode, and (when settled) file/filter view into pane 1, but clears pane 1 multi-selection and cursor; if source is still loading, pane 1 gets an independent listing. Closing split resets active pane to 0. Tab cycles pane focus when split; also offers editable preview and copy-board focus according to current state, so it is not a tab-switch command.
- State read/written: two pane folders, active pane, split; no folder history stack.
- Source refs: `src/app.rs:46-90, 316-327, 2456-2540`; `src/handler.rs:277-292`; `src/domain/data.rs:1-14`.
- Runtime evidence: `seeded-session.txt` displays both restored folders side by side with active pane 1 from fixture state. No-tab/history finding is static search evidence (`git grep -i -E 'tab|history|back|forward' tui-oracle-baseline -- src/app.rs src/handler.rs src/domain src/services`; matches describe focus/editor/search/cache, but no tab array/path-history stack).
- Edge cases observed: split clones pane 0, not the stale previous pane 1 location saved in state; pane 1 starts with no selected items.
- Error behavior: none.
- Config knobs: `state.split`, `state.active`, `state.left`, `state.right`.
- Platform notes: desktop tabs would be additive; preserve the two-pane toggle and Tab focus semantics.
- Depends on: INV-CONFIG-003, INV-STATE-002.
- Confidence: high.

### INV-STATE-002 Cursor and multi-selection model
- Trigger: listing settles; arrows/j/k move; Space multi-selects; Ctrl/Alt/Super+A toggles select-all; Alt/Super+I inverts.
- Behavior: each pane has `files`, Ratatui `ListState`, and a parallel `selected: Vec<bool>`. Cursor index addresses visible rows; in live search/confirmed filter, helpers map visible row index back to source-file index. Space toggles current entry and advances cursor. Select-all operates on visible set: if any visible entry is unselected, select all visible; otherwise clear all visible. Invert toggles visible set only. Listing refresh replaces selection with all false; cursor rests first row, or none for empty list; parent navigation remembers child path and selects that entry after listing settles.
- State read/written: per-pane cursor, selection flags, visible mapping, pending-select path; no persistence for cursor/selection.
- Source refs: `src/app.rs:46-58, 1007-1041, 1115-1142, 2935-2974, 3783-3833`; `src/handler.rs:293-317`.
- Runtime evidence: seeded UI shows `[ ]` checkboxes; direct-PTY `pty-selection.txt` shows `[*] · .hidden` after Space and the cursor advanced to `Beta.txt`.
- Edge cases observed: sort carries selection along with its file; filter restricts select-all/invert and actions; new folder listing resets selection.
- Error behavior: empty visible set makes select-all a no-op.
- Config knobs: none.
- Platform notes: desktop needs independent cursor and multi-select state, not a single focused row only.
- Depends on: INV-STATE-001, INV-STATE-004, INV-STATE-005.
- Confidence: high; selection and cursor advance were captured in direct PTY output.

### INV-STATE-003 Per-pane sort modes
- Trigger: `,` cycles active pane mode Name → Size → Modified → Kind → Name.
- Behavior: name is ascending label; size puts files largest-first and directories last (alphabetical inside groups); modified newest-first, unknown timestamps last; kind directories first then files, alphabetical. Sort permutes file list and parallel selections, preserves cursor by selected path, recomputes active filter, resets render scroll. Mode is per pane; fresh panes default Name; opening split copies pane 0 mode. Sort mode is not serialized, so restart returns to default listing order.
- State read/written: in-memory `Pane.sort_mode`, `files`, `selected`, `ListState`; not persisted.
- Source refs: `src/app.rs:87-89, 2294-2410, 2472` and `src/handler.rs:326-327`.
- Runtime evidence: `sort-size.txt` records the exact status `Sorted by size (largest first)` after `,` in a seeded split session; initial seeded listing shows alphabetical labels (`.hidden`, `Beta.txt`, `alpha.txt`, `large.bin`, `small.bin`).
- Edge cases observed: stable sort; ties by label; sorting while searching/filtering keeps cursor on same file.
- Error behavior: none.
- Config knobs: none.
- Platform notes: filesystem modified-time/size semantics come from listing layer.
- Depends on: INV-STATE-002, INV-STATE-005.
- Confidence: high.

### INV-STATE-004 Hidden-file visibility
- Trigger: `.` toggles hidden entries.
- Behavior: one global `show_hidden` bool applies to both panes; both panes relist, then state is immediately persisted. Default false. Hidden means labels beginning with `.` are filtered by listing.
- State read/written: `state.hidden` and both pane listings.
- Source refs: `src/app.rs:326-327, 1079-1088, 2978-2984`; `src/handler.rs:320-322`.
- Runtime evidence: fixture includes `.hidden`; `hidden-off-session.txt` (state `hidden=0`) omits it; `seeded-session.txt` (state `hidden=1`) displays it in the left pane.
- Edge cases observed: a toggle relists both panes regardless of split state.
- Error behavior: none.
- Config knobs: `state.hidden`.
- Platform notes: dot-name convention.
- Depends on: INV-CONFIG-002, INV-STATE-001.
- Confidence: high.

### INV-STATE-005 Live fuzzy search and sticky filter
- Trigger: `/` begins live search; typing updates fuzzy-ranked visible matches; Enter confirms as sticky pane filter; Esc in search cancels; Esc in normal mode clears sticky filter.
- Behavior: empty search query shows all entries. Nonblank confirmation stores query and fuzzy indices; all actions use filtered visible set. Clearing a confirmed filter preserves cursor on the same underlying file when possible, else selects first row/none. Refresh recomputes sticky filter. Fuzzy scoring is `utils::fuzzy::fuzzy_score`; ties preserve source index order. Search/filter state is in-memory only.
- State read/written: global live `search_query`; per-pane `filter_query` and `filter_indices`; cursor.
- Source refs: `src/app.rs:70-75, 3755-3875`; `src/handler.rs:186-191, 266-268, 271-272`; `src/utils/fuzzy.rs`.
- Runtime evidence: direct-PTY `pty-search-final.txt` shows only `alpha.txt` in pane 0 after `/alpha` + Enter; Esc restores all rows before navigation (`pty-navigation-final.txt`). Search/filter state is absent from `pty-state-after-q.txt`.
- Edge cases observed: empty/whitespace query clears filter on confirm; sort recomputes filter; entering folders clears filter.
- Error behavior: no match yields an empty visible list (no explicit error message in this code path).
- Config knobs: none.
- Platform notes: matching and selection semantics should be shared in UI-agnostic core.
- Depends on: INV-STATE-002, INV-STATE-003.
- Confidence: high; live and confirmed filtering plus clearing were driven through the direct PTY.

### INV-STATE-006 Folder navigation and absent navigation history
- Trigger: Right enters directory, Left navigates parent; Enter starts rename; common-folder/bookmark/drive shortcut selects a folder.
- Behavior: navigation changes the active pane's folder and issues a listing. Right on a directory clears filter and pending selection; Left remembers the child path and selects that entry in the parent's refreshed list. There is no previous/next directory history stack in the claimed source. Restored pane folders are the only session resume location.
- State read/written: active pane folder, pending child path, current listing/cursor; saved pane folders on session persistence.
- Source refs: `src/app.rs:2175-2249, 3939-3967`; `src/handler.rs:329-345`.
- Runtime evidence: direct-PTY `pty-navigation-entered-left.txt` shows Right opening `fixture/left`; `pty-navigation-final.txt` shows Left returning to parent `fixture` and listing `left`/`right` with `left` selected. Enter was separately observed to open the rename prompt; Esc canceled it without a filesystem change.
- Edge cases observed: parent at root is a no-op; parent lookup errors are silently ignored; folder list async results are generation-checked to discard stale responses (`src/app.rs:993-1003, 1105-1114`).
- Error behavior: navigation does not emit a status in the shown `get_parent_directory` error branch.
- Config knobs: `state.left`, `state.right`.
- Platform notes: path handling is string-based in the state model; filesystem layer owns OS path interpretation.
- Depends on: INV-STATE-001, INV-STATE-002.
- Confidence: high; Right/Left navigation was captured in the isolated fixture.

## Unclaimed or uncertain
- Config-only scope is covered by state/bookmark/theme modules above. `docs/themes.md`, `docs/theme.cyberpunk2077.toml`, and other examples are documentation/sample inputs, not runtime schemas. Key decoding and full action inventory remain slice 3/other explorers.
- Direct PTY verified search typing/confirmation, Space selection, bookmark add/remove/jump, Right/Left navigation, hidden-toggle persistence, and clean `q` exit in `pty-session.ansi.txt` and decoded captures. Tabs and navigation back/forward history are not present in the claimed source.
- `dirs_next::config_dir()` uses native OS paths, but this run only verified Linux/XDG.
- No session migration/version mechanism was found; state parser's tolerant legacy behavior is as described above.
