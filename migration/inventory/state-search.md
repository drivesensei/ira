# Inventory: core state model and search/filter/jump
Explorer run: 2026-10-01 | TUI commit: 1cad4ce43cc72d52d4cc4eef920e0da22cb69568 | Files claimed: `src/app.rs` (pane/state/navigation/sort/search/goto sections), `src/handler.rs` (normal/search key routing), `src/utils/fuzzy.rs`, `src/components/tab1_files_ui.rs` (list projection/cursor rendering), `src/domain/data.rs` (`Folder` record)

## Features

### INV-STATE-SEARCH-001 Pane and visible-row state
- Trigger: automatic when the app initializes, lists a folder, or changes a view.
- Behavior: `App` owns exactly two `Pane`s plus `active_pane` and `split`. A pane stores optional folder, underlying `files`, a parallel `selected: Vec<bool>`, `ListState`, render offsets, sort mode, listing generation/settled state, and confirmed-filter query/index list. The `ListState` index is a *visible-row index* under active search/filter; helpers map it back to the underlying `files` index. Each complete small-folder listing starts at row 0 (or `pending_select`, if present); an empty listing has no cursor. The initial name order is label ascending. Each listing rebuild clears selection flags. Large listing streams asynchronously and replaces the prefix with a sorted final list; generation ids reject stale overlapping listing results.
- State read/written: `Pane`, `App.active_pane`, `App.split`; `files`, visible indices, selection flags, `ListState`, offsets, generation.
- Source refs: `src/app.rs:48-90`, `src/app.rs:317-320`, `src/app.rs:987-1047`, `src/app.rs:1050-1158`, `src/app.rs:3783-3833`; `src/components/tab1_files_ui.rs:93-163,215-279`.
- Runtime evidence: `migration/oracle/captures/state-search/pty-state-search.md` (fresh HOME fixture, 120×40; initial Home listing and live filter screens).
- Edge cases observed: hidden entries are excluded by default; empty filtered views have a rendering inconsistency (INV-STATE-SEARCH-008); sort/filter/cursor/selection are not in `SessionState` (session config explorer owns persisted-state details).
- Error behavior: listing errors settle the pane; specific filesystem listing errors belong to filesystem slice.
- Config knobs: session hidden flag affects list membership; persisted session fields are covered by config-state inventory.
- Platform notes: the model stores `Folder.path`, `FEntry.path`, prompts and filter labels as `String`; path representation risk belongs to platform/filesystem review.
- Depends on: filesystem listing; INV-STATE-SEARCH-002.
- Confidence: high; source and a 120×40 runtime listing/filter capture agree.

### INV-STATE-SEARCH-002 Cursor navigation and bounds
- Trigger: normal-mode Up/Down, `z` (top), `x` (bottom); during fuzzy search Up/Down and Alt+Up/Down.
- Behavior: Up/Down move one row for a fresh press, saturate at either list boundary, and do not wrap. With no cursor, Down selects row 0; Up selects row 0 only if visible rows exist. `z` and `x` select first/last visible row, or clear selection for zero rows; they reset held-key acceleration. Repeats in the same direction within 200 ms ramp each six repeats up to six rows; pause/direction change resets. `user_navigated` is set by arrow/top/bottom navigation.
- State read/written: visible row count and `Pane.state`; scroll-ramp fields and pane `user_navigated`.
- Source refs: `src/handler.rs:184-208,245-250,348-359`; `src/app.rs:2253-2282,2413-2453`; constants at `src/app.rs:172-179`.
- Runtime evidence: `migration/oracle/captures/state-search/pty-state-search.md` records `x` selecting the last row, `z` returning to first, Down advancing to second, and movement from parent to child listing.
- Edge cases observed: a zero-result confirmed filter can leave no action cursor while the renderer still paints the full unfiltered list (INV-STATE-SEARCH-008).
- Error behavior: none.
- Config knobs: none.
- Platform notes: terminal arrow repeat cadence determines acceleration; a desktop equivalent should use deliberate key-repeat timing or single-row steps.
- Depends on: INV-STATE-SEARCH-001.
- Confidence: high; source plus live PTY.

### INV-STATE-SEARCH-003 Directory navigation and path jump
- Trigger: Right enters the selected directory; Left opens its parent; `[` opens direct-path prompt; normal shortcut letters can switch to common folders/bookmarks.
- Behavior: Right resolves the currently visible entry and enters it only when it resolves as a directory; a file is sent to the external open-file service. Enter is rename, not enter-directory. On entering a child, the pane clears confirmed search and filter, clears `pending_select`, changes folder, reloads files, then selects row 0 when the listing settles. Left resolves the parent; root/no-parent and parent lookup errors are silent no-ops. When going up, current path becomes `pending_select`, so the old folder is selected in the parent after listing settles. The `[` prompt expands a path and navigates existing targets; an existing file opens its containing folder and selects the file. A missing path creates parent directories and a final directory or file (extension determines kind), then navigates there; exact creation/errors are input/filesystem-slice behavior. No back/forward path stack, recent-location list, or distinct jump-list implementation was found in the state/handler source; `Left` is parent navigation, and `[` is explicit path entry.
- State read/written: active pane folder, filter/search, pending selection, listing; goto prompt is transient.
- Source refs: `src/handler.rs:228-272,341-346`; `src/app.rs:2206-2251,2656-2779`; `src/utils/is_dir.rs` (`get_directory`, `get_parent_directory`).
- Runtime evidence: `migration/oracle/captures/state-search/pty-state-search.md`: Right from selected `Archive` displayed its child `a.txt`; Left returned to Home and restored the `Archive` cursor; entering the existing absolute `.../Archive/a.txt` path via `[` and checking `?` showed Name `a.txt` and path `/tmp/ira-state-search-nav.gVab9i/home/Archive/a.txt`.
- Edge cases observed: search/filter is cleared on directory entry; root parent returns without an error; no history navigation binding or structure was found by source sweep.
- Error behavior: parent resolution errors are ignored; go-to-path creation errors are emitted as status messages and covered by input/filesystem slices.
- Config knobs: common-folder/bookmark shortcut sources are owned by state/config explorer.
- Platform notes: path expansion and String/lossy-path semantics require platform review. Desktop equivalent for `[` should be a path-entry dialog preserving full path semantics; no recent-history UI should be invented as parity.
- Depends on: filesystem listing; prompt/edit behavior in entry-input slice.
- Confidence: high for existing-file path and parent navigation; low for untested permission/root failure paths.

### INV-STATE-SEARCH-004 Multi-selection state
- Trigger: Space toggles current row; Ctrl+A toggles select-all/clear; Alt/Super+I inverts; Alt/Super+A aliases select-all where the terminal reports those modifiers.
- Behavior: Space maps the visible cursor row to its underlying file index, toggles that selection bit, then invokes `next_item`; at the last item the cursor stays at last. Ctrl+A selects all currently visible rows if *any* is unselected; if all visible rows are selected, it clears those visible rows. Invert toggles only visible rows. Items outside an active filter retain their selection flags. Empty visible set makes select-all a no-op. Ctrl routing precedes search-mode routing, so Ctrl+A during live search invokes select-all on current visible matches rather than editing query text.
- State read/written: `Pane.selected` parallel to `files`, `ListState`, active visible indices.
- Source refs: `src/handler.rs:18-40,293-318`; `src/app.rs:2937-2974,3807-3825`; visible selection marker in `src/components/tab1_files_ui.rs:113-125,286-307`.
- Runtime evidence: `migration/oracle/captures/state-search/pty-state-search.md`: Space selected the highlighted `Archive` row (`[*]`) and advanced cursor to `alpha.txt`; Ctrl+A marked all visible rows, second Ctrl+A cleared them; under `/alp` + Enter, Ctrl+A selected the sole visible match and Esc restored the unfiltered list with its selection intact.
- Edge cases observed: selection persists across filter toggles because flags are stored by underlying file index; rebuilding a listing resets all flags.
- Error behavior: none.
- Config knobs: none.
- Platform notes: Super/Alt modifier delivery is terminal-dependent; Ctrl+A is the reliable TUI shortcut.
- Depends on: INV-STATE-SEARCH-001, INV-STATE-SEARCH-007.
- Confidence: high; direct PTY and implementation agree.

### INV-STATE-SEARCH-005 Sort modes
- Trigger: `,` in normal mode.
- Behavior: advances the active pane's mode in a four-mode cycle: Name ascending → Size (files largest first; directories last; labels ascending within directories and equal-size files) → Modified (newest first; unknown timestamps last; labels break ties) → Kind (directories first, then files, labels ascending) → Name. The entries and their parallel selected flags receive one stable permutation. It attempts to preserve the same selected path across sorting, mapping through filtered/live visible indices as required; filter indices are recomputed. Live search results are recomputed against the new order. Render scroll resets to zero. Status notices are exactly `Sorted by size (largest first)`, `Sorted by last modified (newest first)`, `Sorted by kind`, `Sorted by name`. Sort mode defaults to Name per new Pane and is not restored by `App::restore_state` / absent from `SessionState`.
- State read/written: active `Pane.sort_mode`, files, selected flags, filter indices, cursor, render scroll, transient status.
- Source refs: `src/app.rs:87-89,2294-2410,3939-3967,4006-4021`; `src/handler.rs:326-327`; notices are rendered by UI/status slice.
- Runtime evidence: `migration/oracle/captures/state-search/pty-state-search.md`: `,` displayed `Sorted by size (largest first)` and placed the 9-byte fixture file before smaller files; directories followed files. Other comparator orders are source-derived only.
- Edge cases observed: missing sort cursor stays None; live/confirmed filtering preserves cursor if its item remains visible; if it no longer resolves, selection may become None.
- Error behavior: none.
- Config knobs: none; sort isn't serialized.
- Platform notes: timestamps/size metadata can be missing or have platform-dependent precision; comparator explicitly places unknown mtimes last.
- Depends on: INV-STATE-SEARCH-001.
- Confidence: high on source and size-order runtime; medium for Modified/Kind tie edge cases (not dynamically varied).

### INV-STATE-SEARCH-006 Fuzzy scoring
- Trigger: each live or confirmed search query against visible file labels.
- Behavior: empty query returns all file indices. Otherwise query and target are Unicode-lowercased into `char`s; query must be a subsequence in order. Each matched char gives 10 points, +8 when consecutive with prior match, and +6 when at target start or after separator (`space . _ - / \\ :`). Score sorts descending; equal scores retain input index order. Matching is over label only, not contents or path. No regex/glob/content-search/saved-search engine was found in the claimed source.
- State read/written: query, labels, visible index vectors.
- Source refs: `src/utils/fuzzy.rs:1-46`; `src/app.rs:734-745,3753-3758`.
- Runtime evidence: `migration/oracle/captures/state-search/pty-state-search.md`: `/alp` matched `alpha.txt` and `alphabet.log`; `/zzzz` had zero live matches. Ranking behavior is derived from scoring source; equal-score ordering followed the already name-sorted input in fresh listing.
- Edge cases observed: case insensitive and subsequence (not substring); query characters remain ordered; ties preserve current file ordering. Unicode normalization is not applied.
- Error behavior: no error/notice for zero matches.
- Config knobs: none.
- Platform notes: normalization/case rules may differ from platform filesystem naming; algorithm itself lowercases Rust strings.
- Depends on: INV-STATE-SEARCH-001.
- Confidence: high; source and runtime match fixtures.

### INV-STATE-SEARCH-007 Live search mode
- Trigger: `/` in normal mode.
- Behavior: starts an empty live query and selects visible row 0 when rows exist. Each typed character appends and resets cursor to visible index 0; Backspace pops one Unicode scalar value (Rust `String::pop`) and resets cursor to 0. Matching results render live in score order. Up/Down move the cursor in results; Alt+Up/Down set top/bottom then also execute previous/next in the same handler branch; Right enters the selected result if it is a directory. Enter confirms the query instead of opening the selected result; Esc cancels and clears the query, resetting selection to underlying row 0 when files exist (not restoring the pre-search cursor). Other key events in search routing are ignored. Left has no case in search routing and is ignored.
- State read/written: `App.search_query`, pane cursor, live fuzzy match vector.
- Source refs: `src/handler.rs:184-208,271-272`; `src/app.rs:3748-3758,3835-3885`.
- Runtime evidence: `migration/oracle/captures/state-search/pty-state-search.md`: `/alp` narrowed live screen to alpha-prefixed fixture files; `/zzzz` produced a blank files area; Esc canceled and restored full list. Search uses 120×40 direct PTY.
- Edge cases observed: no selection gets assigned when starting search over an empty list; live zero matches still set cursor to visible index 0 after typing even though visible count is zero.
- Error behavior: none.
- Config knobs: none.
- Platform notes: slash activation and character entry are terminal-specific; desktop can use keyboard-first search input while preserving live match order.
- Depends on: INV-STATE-SEARCH-006.
- Confidence: high for live filtering and zero-match screen; medium for unprobed modifiers/paste/IME in search.

### INV-STATE-SEARCH-008 Confirmed filter, clear, and zero-match rendering inconsistency
- Trigger: Enter while live search is active confirms; Esc in normal mode clears confirmed filter.
- Behavior: Enter promotes a trimmed-nonempty query to pane-sticky `filter_query` and `filter_indices`; only these rows are considered visible by action helpers (so operations/selections target filtered set). Empty or whitespace-only query exits search without filter. Normal-mode Esc clears filter while retaining the underlying file corresponding to the filtered cursor; if no row was selected, it selects underlying row 0, or None if files empty. Live query cancellation uses Esc in search mode and resets cursor to row 0. On refresh, current confirmed filter is recomputed. **Observed defect:** after a query with zero matches is confirmed, the file UI renders the full unfiltered list, despite action helpers reporting zero visible rows: both `src/components/tab1_files_ui.rs:102-129` only enter filter rendering when `filter_indices` is nonempty, and then take the full-list branch; `pane_visible_rows` returns empty when `filter_query` is Some. Runtime `/zzzz` + Enter painted all fixture rows; a subsequent Down cleared cursor (zero visible count) while full rows remained displayed. This can expose rows visually that keyboard actions do not address.
- State read/written: pane filter query/index set, active search cleared, cursor mapping; action target projection.
- Source refs: `src/handler.rs:184-208,266-267`; `src/app.rs:3760-3781,3790-3825,3851-3868,1036-1041`; `src/components/tab1_files_ui.rs:102-129`.
- Runtime evidence: `migration/oracle/captures/state-search/pty-state-search.md` contains no-match live, confirm, then full unfiltered render, and Down-with-no-cursor observation; also `/alp` confirm and Esc clear.
- Edge cases observed: a no-match confirmed filter displays full list while visible count is zero; this is a parity bug in the oracle that migration should characterize and preserve unless an explicit decision changes it.
- Error behavior: no notice or empty-state message for zero results.
- Config knobs: none.
- Platform notes: desktop must keep displayed rows and actionable rows consistent; parity expectation currently is the observed TUI behavior, but a fix requires an explicit recorded decision.
- Depends on: INV-STATE-SEARCH-006, INV-STATE-SEARCH-007, INV-STATE-SEARCH-001.
- Confidence: high; source control flow and direct PTY both reproduce.

### INV-STATE-SEARCH-009 Two-pane split state
- Trigger: `+` toggles split; Tab cycles pane focus when split.
- Behavior: closing split sets `split=false` and `active_pane=0`. Opening split sets active pane 0 and mirrors left folder and sort mode into right. It resets right cursor, scroll/grid offsets, pending selection, and navigation marker. If left listing is settled, clones its files/filter/index/settled state and initializes all right selection flags to false; if unsettled, requests a separate listing. No tab stack exists; app state contains a fixed two-element pane array.
- State read/written: `App.split`, `active_pane`, both panes.
- Source refs: `src/app.rs:317-321,2456-2493,2495-2510`; Tab dispatch `src/handler.rs:290-291`.
- Runtime evidence: `migration/oracle/captures/state-search/pty-state-search.md` notes `+` produced two pane columns mirroring the current folder; pane reset details are source-derived.
- Edge cases observed: split closure always restores focus to pane 0; right-pane prior folder is deliberately not resumed on newly opened split.
- Error behavior: if left listing was still streaming, right starts its own listing.
- Config knobs: `split`, left/right folder persisted by config-state slice; filter/selection specifics aren't persisted.
- Platform notes: desktop dual-pane state should remain distinct from editor tabs.
- Depends on: INV-STATE-SEARCH-001.
- Confidence: medium; source and split screen were observed, right-side selection reset not separately asserted at runtime.

## Unclaimed or uncertain
- Files claimed only at relevant section scope: `src/app.rs`, `src/handler.rs` also contain config, file operations, preview, theme, and external-service behaviors owned by other slices. `src/services/state.rs` persistence schema and `src/services/list_files.rs` metadata/sorting inputs are owned by config-state/filesystem explorers.
- No state/navigation history stack or back/forward binding was found by `rg -n 'history|back|forward|last.dir|last_dir|jump' src/app.rs src/handler.rs src`; this grep is a source absence check, not a runtime test of a nonexistent command. Key `o` bookmark jump is config/bookmarks slice; direct `[` path input prompt content/editing is entry-input slice.
- The high-confidence no-match render mismatch is a confirmed oracle inconsistency; the migration manager should create an adversarial GAP/test if it elects to intentionally correct it rather than silently diverge.
- Sort Modified/Kind comparators and zero/one-row navigation edges were not varied with a dedicated timestamp/kind-rich runtime fixture; exact behavior is source-derived.
