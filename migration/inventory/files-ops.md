# Inventory: filesystem listing and file operations
Explorer run: 2026-10-01 | TUI commit: `1cad4ce43cc72d52d4cc4eef920e0da22cb69568` (`tui-oracle-baseline`) | Files claimed: `src/services/list_files.rs`, `src/services/transfer.rs`, `src/services/state.rs`, `src/app.rs` (listing and file operations), `src/handler.rs`, `src/components/tab1_files_ui.rs`, `src/components/copy_board_ui.rs`, `src/ui/mod.rs` (listing/operation rendering)

## Features
### INV-files-ops-001 Directory enumeration and metadata
- Trigger: pane startup, navigation, refresh, or file-operation completion.
- Behavior: bounded listing reads up to 512 visible entries synchronously; <=512 is complete and sorted by case-sensitive `label.cmp`. Larger folders stream chunks of 512 in directory enumeration order, then replace with a fully sorted list. Opening `read_dir` errors propagate; individual entry errors print `Error reading entry: {e}` and are skipped. Non-UTF-8 names are silently skipped. `metadata` follows symlinks: directory size is 0; file size and modified time come from target. On stat failure, size=0, modified=None, and dir hint is `DirEntry::file_type()`.
- State read/written: pane folder/files, selection/cursor, listing generation; no listing file.
- Source refs: `src/services/list_files.rs:1-145` (`entry_with_meta`, bounded/chunked listing); `src/app.rs:987-1103` (`list_files_for_pane`, worker); `src/app.rs:1105-1150` (generation guard and result application).
- Runtime evidence: `migration/oracle/captures/files-ops/initial-listing.txt`; fixture: `migration/oracle/fixtures/make_files_ops_fixture.py`. At 120×40 the left list showed `README`, `a.txt`, `b.txt`, `folder`, `large.bin`; right showed `a.txt`; hidden `.secret` was absent.
- Edge cases: exact 512 is complete; >512 streams; symlink-to-directory is classified as directory; invalid UTF-8 is omitted; stale generation is dropped. Unit tests: `bounded_listing_reports_complete_for_small_folders`, `bounded_listing_reports_partial_at_the_cap`, `chunked_listing_batches_entries`, `symlink_to_a_directory_is_listed_as_a_directory` in `src/services/list_files.rs`.
- Error behavior: `read_dir` returns its I/O error; per-entry failure prints `Error reading entry: {e}`.
- Config knobs: session `hidden` affects enumeration.
- Platform notes: lossy string paths; UTF-8 labels required.
- Depends on: pane/navigation state.
- Confidence: high source/tests; dynamic evidence covers only initial small listing.

### INV-files-ops-002 Sort cycle and cursor retention
- Trigger: `,` in normal mode.
- Behavior: cycles Name → Size → Modified → Kind → Name. Name is ascending case-sensitive string order. Size is largest files first, then directories, alphabetical within groups. Modified is newest first, unknown times last, ties alphabetical. Kind is directories first then files, alphabetical. Cursor stays on the same path; selection flags follow entries; confirmed filter indices are recomputed.
- State read/written: active pane sort mode, files, selection, cursor, filter indices; transient status notice.
- Source refs: `src/handler.rs:326-327`; `src/app.rs:2294-2410` (`cycle_sort`).
- Runtime evidence: source only; sort test names in `tests/sort_test.rs`: `comma_cycles_sort_modes_and_reorders_files`, `cursor_and_selection_follow_the_same_file_across_sorts`, `sort_cycle_recomputes_the_confirmed_filter_view`.
- Edge cases: directory grouping; unknown timestamps; visible search/filter index mapping.
- Error behavior: status `Sorted by size (largest first)`, `Sorted by last modified (newest first)`, `Sorted by kind`, or `Sorted by name`.
- Config knobs: none; sort mode is not persisted.
- Platform notes: modified timestamps before epoch/unavailable are unknown.
- Depends on: INV-files-ops-001 and selection/filter mapping.
- Confidence: high source/tests; reorder not dynamically captured.

### INV-files-ops-003 Hidden-file toggle
- Trigger: `.` in normal mode.
- Behavior: toggles visibility of names beginning `.`; re-lists both panes and persists immediately. Fresh default is off; restored state wins. Filtering occurs before streaming chunks are submitted.
- State read/written: `show_hidden`, session `hidden=0|1`, both pane listings.
- Source refs: `src/handler.rs:320-322`; `src/app.rs:2976-2981`; `src/services/list_files.rs:78-109`; `src/app.rs:1079-1089`; `src/services/state.rs:91-100,119-135`.
- Runtime evidence: hidden-off screen in initial listing; toggle-on not captured.
- Edge cases: all leading-dot names are hidden regardless of type.
- Error behavior: none.
- Config knobs: persisted session `hidden`.
- Platform notes: dot-name convention used on all platforms.
- Depends on: INV-files-ops-001.
- Confidence: high source; medium runtime.

### INV-files-ops-004 Details row format
- Trigger: fresh default preview mode 3 or persisted `preview0/preview1=3`.
- Behavior: selection marker, icon, truncated name, right-aligned size and relative modification time. Folders show `—` and do not start recursive size walk. Names truncate by character count with ellipsis; missing timestamp is `—`; future timestamp is `just now`.
- State read/written: FEntry metadata, selection, per-pane preview mode.
- Source refs: `src/components/tab1_files_ui.rs:171-327,329-365`; `src/services/state.rs:36-49`.
- Runtime evidence: not captured; fixture selected preview off.
- Edge cases: single-width assumption for labels; no folder-size walk in Details mode.
- Error behavior: unknown mtime renders `—`.
- Config knobs: persisted per-pane preview values.
- Platform notes: sizes use base 1024, relative dates use wall clock.
- Depends on: INV-files-ops-001.
- Confidence: high source/tests; runtime presentation pending.

### INV-files-ops-005 Operation source selection
- Trigger: Space multi-select, Ctrl+A select/clear, Alt+I invert, then `c`, `m`, Delete; with no selection, current row is used.
- Behavior: any selected flag means all selected entries are gathered from full file vector. Otherwise uses selected visible row translated through filter/search mapping. No cursor/empty visible set silently no-ops.
- State read/written: pane selection/cursor; transfer start clears selection; delete completion clears it.
- Source refs: `src/handler.rs:293-317`; `src/app.rs:3352-3372` (`collect_sources`), `3434-3441`, `3663-3666`.
- Runtime evidence: source only.
- Edge cases: selected items span the full list even while filtered.
- Error behavior: empty source vector silently returns.
- Config knobs: none.
- Platform notes: macOS Backspace also initiates delete (INV-files-ops-011).
- Depends on: pane state, filtering, INV-files-ops-001.
- Confidence: high source; interactive selection unverified.

### INV-files-ops-006 Create file/folder
- Trigger: `n`; type name; Enter creates; Esc cancels. Modal previews `file (.ext)` for a non-leading final dot with nonempty suffix; otherwise `folder`. Slash-containing input previews `nested folder(s) + file`.
- Behavior: trims surrounding whitespace. Empty input leaves dialog open and reports `Enter a name first.` A valid final suffix creates empty file with `create_new`; otherwise `create_dir_all`. Existing names are preserved and report `'{name}' already exists.` Race-time existing file reports same; existing non-folder directory case reports `'{name}' already exists and is not a folder.` Other errors report `Failed to create '{name}': {err}`. Nested missing parents are created. On success simple name refreshes listing and selects new entry; slash-containing input navigates to created folder (or file parent).
- State read/written: current pane listing/cursor and filesystem.
- Source refs: `src/handler.rs:106-117,252-254`; `src/ui/mod.rs:345-365`; `src/app.rs:2793-2923`.
- Runtime evidence: `migration/oracle/captures/files-ops/new-entry-dialog.txt`, `new-entry-typed.txt`, `create-folder-result.txt`; file creation also confirmed on disk. Live PTY showed `n` dialog and created folder/file.
- Edge cases: `.config`, `backup.`, `notes` are folders; `notes.txt`, `data.v2.json` are files. `/` is explicitly used for nested display check; Windows `\\` behavior needs characterization.
- Error behavior: exact strings above.
- Config knobs: none.
- Platform notes: `Path` handles target OS semantics; dialog hint checks `/` specifically.
- Depends on: modal/input handling and INV-files-ops-001.
- Confidence: high source, medium path-separator portability.

### INV-files-ops-007 Rename entry
- Trigger: Enter opens prefilled rename editor; Left/Right move cursor, Backspace deletes before cursor; Enter applies; Esc cancels.
- Behavior: empty or unchanged name closes without change. Destination found by `metadata` is rejected; prompt closes and status is `Cannot rename: '{original}' already exists.` Otherwise `std::fs::rename` runs, then listing refreshes and selection clears. On I/O error status is `Failed to rename: {e}`.
- State read/written: prompt/cursor, active-pane file vector and filesystem.
- Source refs: `src/handler.rs:79-90,345-346`; `src/ui/mod.rs:323-331`; `src/app.rs:2984-3044`.
- Runtime evidence: `migration/oracle/captures/files-ops/rename-dialog.txt`, `rename-result.txt`, `rename-collision.txt`; successful rename and existing-name rejection dynamically observed, with filesystem checks.
- Edge cases: collision check follows symlinks; dangling destination may be missed; no visible basename/path-separator guard; collision race delegates to OS rename behavior.
- Error behavior: exact status strings; OS error text varies.
- Config knobs: none.
- Platform notes: rename has no cross-device copy fallback.
- Depends on: INV-files-ops-001.
- Confidence: high source; race/platform behavior needs characterization.

### INV-files-ops-008 Copy
- Trigger: `c` stages confirmation; `y`/Enter confirms; `n`/Esc cancels; `o` changes conflict policy.
- Behavior: current selection/cursor copies to other pane. Missing other folder reports `The other pane has no folder to copy into.` Destination at or below any selected source folder is rejected with `Cannot copy/move a folder into itself.` Confirmation displays `Copy '{name}' → {dest-name}?` or `Copy N items → {dest-name}?`; default is auto-rename. Starts one sequential batch worker, clears selection, opens Copy Board without taking focus, selects new job, and periodically refreshes destination once per second unless user navigated there.
- State read/written: source/destination filesystem, job/progress, selection, board, cursor.
- Source refs: `src/handler.rs:172-181,286-288`; `src/app.rs:2582-2631,2633-2653,3374-3447`; `src/services/transfer.rs:19-37,168-330`.
- Runtime evidence: source only. Existing source tests include `batch_copies_all_files_with_one_worker`, `copy_never_truncates_existing_destination`, `copy_preserves_permissions_and_mtime`.
- Edge cases: copy uses symlink_metadata for collision; partial destination removed on failure/cancel; job total unknown if scan fails or >200,000 entries.
- Error behavior: progress strings `SKIPPED (already exists): {path}`, `FAILED ({error}): {path}`; final failure `{n} of {N} items failed`; Board row says `error`.
- Config knobs: one-time per-confirmation policy.
- Platform notes: Unix preserves symlinks and permissions; non-Unix follows symlink using canonicalize/copy.
- Depends on: INV-files-ops-005, 009 (policy), 010 (board).
- Confidence: high source/tests; basic UI copy and auto-rename observed; other policies/errors remain unverified.

### INV-files-ops-009 Move
- Trigger: `m`, same confirm/cancel/policy keys as copy.
- Behavior: same source/destination validation and batch worker. Tries `std::fs::rename`; falls back only for `ErrorKind::CrossesDevices` to recursive copy then source removal. Default conflict policy auto-renames. Checks folder-into-itself before dialog and before spawn.
- State read/written: source/destination filesystem and job state.
- Source refs: `src/handler.rs:286-288`; `src/app.rs:2588-2631,3374-3447`; `src/services/transfer.rs:235-330`.
- Runtime evidence: `migration/oracle/captures/files-ops/move-confirm.txt`, `move-result.txt`; move/removal observed with filesystem checks. Tests: `batch_move_removes_sources`, `move_never_replaces_existing_destination`.
- Edge cases: cancellation/error while cross-device copying removes partial destination and keeps source. Failure removing source after cross-device copy is treated as item failure and cleanup removes destination.
- Error behavior: batch failure summary and per-item progress detail.
- Config knobs: conflict policy.
- Platform notes: native error-kind and rename behavior depend on filesystem.
- Depends on: INV-files-ops-005, 011.
- Confidence: high source/tests; basic live move observed; cross-device fallback unverified.

### INV-files-ops-010 Conflict policy
- Trigger: `o` while copy/move confirmation open.
- Behavior: AutoRename → Overwrite → SkipExisting → AutoRename. AutoRename picks first free `name (2).ext`, then increasing numbers. Overwrite removes colliding non-directory destination before copy; directory collisions intend merge. Skip emits skip marker, counts skipped item as failed, continues remaining batch. Copy uses `create_new` to avoid truncating a raced-in destination.
- State read/written: confirmation policy and destination tree.
- Source refs: `src/handler.rs:172-181`; `src/ui/mod.rs:273-320`; `src/app.rs:2621-2653`; `src/services/transfer.rs:26-37,197-233,264-329,420-423`.
- Runtime evidence: source only; collision unit tests in transfer module.
- Edge cases: source comment/docs say folders merge, but recursive `copy_entry` calls `create_dir(dst)` and may fail on existing child directories; characterize before implementation. Symlink destinations count as existing.
- Error behavior: `SKIPPED (already exists): {path}` and `{n} of {N} items failed`; overwrite remove failure becomes item error.
- Config knobs: ephemeral confirmation choice.
- Platform notes: remove/rename semantics OS-specific.
- Depends on: INV-files-ops-008/009.
- Confidence: high source; recursive merge inconsistency unresolved.

### INV-files-ops-011 Permanent delete confirmation
- Trigger: Delete on all platforms; Backspace additionally on macOS. `y`/Enter confirms; `n`/Esc cancels.
- Behavior: selected paths or cursor path are shown as `Delete '{name}'?` or `Delete N items?`. Confirmation starts worker. Files/symlinks use `remove_file`; directories use recursive `remove_dir_all`. This is permanent; no trash/recycle call occurs. Worker handles paths sequentially. Cancel check is between paths; recursive removal of one folder cannot be interrupted. Successful completion clears active-pane selection and refreshes both panes. Missing paths are silently ignored by worker.
- State read/written: filesystem, deleting path set, progress state.
- Source refs: `src/handler.rs:4-15,138-143,172-181,293-298`; `src/app.rs:3517-3565,3647-3679`; `src/services/transfer.rs:525-565`; `src/ui/mod.rs:248-320`.
- Runtime evidence: source only; destructive operation was not dynamically run.
- Edge cases: progress is by path; missing path yields no failure due to `symlink_metadata(...).ok()`; remove error collected.
- Error behavior: `Failed to delete '{path}': {err}` and, for more, ` (+N more)`; success has no toast.
- Config knobs: none.
- Platform notes: `Backspace` key path only under `cfg(target_os="macos")`.
- Depends on: INV-files-ops-005, 012.
- Confidence: high source; live confirmation/progress unverified.

### INV-files-ops-012 Copy Board progress, pause, cancel
- Trigger: transfer start auto-opens board; Tab focuses board; Space/`p` pause/resume; `x` requests cancel; Esc/backtick closes board.
- Behavior: one row per job; Running/Paused/Cancelled/Done/Failed use `▶`/`⏸`/`✕`/`✓`/`!`; active progress has 8-cell bar, percent, copied/total bytes (`?` if indeterminate). Copy reads 256 KiB chunks; emits progress every ~4 MiB and at item boundaries. Pause/cancel are checked at chunk/item gates. Mid-file cancel removes partial destination and leaves source. Completed jobs stay in board. Any key hides delete progress dialog, but does not cancel it (separate deletion contract). Destination refresh on transfer completion/cancel.
- State read/written: in-memory jobs/atomic pause-cancel controls, destination partial output.
- Source refs: `src/app.rs:3434-3447,3449-3514,3585-3646,3684-3746`; `src/services/transfer.rs:86-133,164-195,352-478`; `src/components/copy_board_ui.rs:14-126`; `src/ui/mod.rs:140-147,248-270`.
- Runtime evidence: source only; tests `cancel_mid_copy_removes_partial_destination`, `batch_cancelled_before_start_copies_nothing`.
- Edge cases: cancel granularity is chunk/item; dismissing delete progress is not cancel; jobs are not restored after restart.
- Error behavior: Board status `error`; detail is carried in job state and progress marker.
- Config knobs: none.
- Platform notes: one worker thread per batch job.
- Depends on: INV-files-ops-008/009/011.
- Confidence: high source/tests; runtime timing and board visuals pending.

### INV-files-ops-013 Transfer metadata and symlink behavior
- Trigger: recursive transfer.
- Behavior: directory children copied recursively; directory permissions applied after children; Unix file create preserves permission bits; modified timestamps are attempted on all OSes. Unix symlinks are copied as links preserving link target. Non-Unix follows symlink target via canonicalize/copy. No owner/group/ACL/xattr/hardlink identity preservation is implemented in transfer code.
- State read/written: source metadata and destination.
- Source refs: `src/services/transfer.rs:333-478`.
- Runtime evidence: source only; tests `copy_preserves_permissions_and_mtime`, `symlinks_are_preserved_not_followed`, `cyclic_symlink_terminates_without_hanging`.
- Edge cases: broken/cyclic non-Unix link canonicalize fails; copy error removes partial destination.
- Error behavior: I/O error recorded in batch failure.
- Config knobs: none.
- Platform notes: symlink privilege differences; Unix-only mode preservation.
- Depends on: INV-files-ops-008/009.
- Confidence: high source/tests, not Windows-runtime-verified.

## Not present or unclaimed
- Trash/recycle-bin: no operation found; delete directly uses `remove_file` / `remove_dir_all` (`src/services/transfer.rs:543-548`). Undo/redo, bulk rename, link creation, chmod/chown, archive creation/extraction, duplicate are not in `src/handler.rs:228-361` or app operation methods. Archive references in icons/thumbnail services are rendering/preview only.
- `[` go-to-path can create nested paths; belongs to navigation/entry-input slice. Tabs, panes, filtering, selection model, theme, icon assignment and general status/modal behavior overlap other slices.
- Runtime method: direct 120×40 PTY after 12-second startup wait; press `w` first if initially on Drives. Create, rename/collision, copy/auto-rename, and move were dynamically observed. Delete confirmation/progress, pause/cancel, other conflict policies, recursive/error paths and sorting remain unverified at runtime.
