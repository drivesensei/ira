# Inventory: Filesystem listing, metadata, symlinks, permissions, refresh, caching, errors
Explorer run: 2026-10-01 | TUI commit: `1cad4ce43cc72d52d4cc4eef920e0da22cb69568` (`tui-oracle-baseline^{}`; source files match this baseline at discovery HEAD `5a99091743083b483de9999516329e492668cdad`) | Files claimed: `src/services/list_files.rs`, `src/services/file_info.rs`, `src/app.rs` listing / metadata / refresh paths, `src/components/tab1_files_ui.rs`, `tests/sort_test.rs`, `tests/drives_refresh_test.rs`

## Features
### INV-filesystem-001 Directory listing and display columns
- Trigger: opening a folder; refreshes after navigation, file jobs, or explicit listing requests.
- Behavior: rows are sorted by `label.cmp` at the final result; directory metadata reports size `0`, files use `Metadata::len()`. UI presents names, file-type icons, byte-size humanization, and relative modified time. Default small-list path returns immediately if `<=512` visible entries; an empty listing selects no row. Order is Rust string comparison (case-sensitive lexicographic, no natural-number sorting here).
- State read/written: active pane path and listing vector (`FEntry { path: String, label: String, is_dir, size, modified }`). Paths are converted with `to_string_lossy`.
- Source refs: `src/services/list_files.rs:1-65`, `src/app.rs:989-1047`, `src/components/tab1_files_ui.rs:300-324,346-380`.
- Runtime evidence: `migration/oracle/captures/filesystem/listing-hidden-default.txt` (120x40, `w` from Drives to isolated `/tmp/ira-fs-oracle-fixture`; visible names sorted `alpha.txt`, `dangling-link`, `empty-dir`, `folder`, `link-dir`, `link-file`, `pipe`; file sizes/modified columns visible).
- Edge cases observed: dot names omitted by default; FIFO is represented as an ordinary non-directory file row with size 0 and modified time; empty directories show size em-dash in UI.
- Error behavior: failure opening the folder falls through to async listing; see INV-filesystem-006. Individual read-entry errors print `Error reading entry: {error}` to stdout and are skipped.
- Config knobs: `show_hidden` runtime/persisted flag (state persistence belongs to config-state inventory).
- Platform notes: File and directory paths/names must be valid UTF-8 to enter listing: `to_str()` failures silently skip the entry. On Windows, Rust path strings are lossy-converted for `FEntry.path`; names whose `OsStr` cannot convert to UTF-8 are omitted entirely.
- Depends on: core pane state, hidden toggle, sort-state behavior.
- Confidence: high for source and Linux fixture; platform-dependent metadata and non-UTF-8 behavior need Windows/macOS execution.

### INV-filesystem-002 Hidden entry filtering
- Trigger: normal key `.`; default `show_hidden=false`.
- Behavior: names whose label starts with `.` are omitted from both bounded and streamed listing when disabled; enabling includes them. This is a filename convention, not OS hidden-attribute detection. The toggle requests both pane listings and persists state.
- State read/written: `App.show_hidden`, persisted via `persist_state()`.
- Source refs: `src/handler.rs:320-321`, `src/app.rs:325-326,2976-2982`, `src/services/list_files.rs:78-109`, `src/app.rs:1079-1088`.
- Runtime evidence: default omission is visible in `listing-hidden-default.txt` while fixture contains `.hidden.txt` (created by `migration/oracle/fixtures/filesystem/make_fixture.py`). The interactive `.` key did not produce a verified state change in this PTY run; therefore inclusion/toggle observation remains source-backed only.
- Edge cases observed: any dot-prefixed name is treated hidden; no special Windows hidden attribute query in this path.
- Error behavior: none.
- Config knobs: persisted hidden flag.
- Platform notes: behavior is portable basename logic, but hidden-attribute files on Windows are not detected by this rule.
- Depends on: INV-filesystem-001.
- Confidence: medium (default omission runtime-confirmed; runtime toggle not confirmed in this run).

### INV-filesystem-003 Metadata following symlinks
- Trigger: listing any entry.
- Behavior: `DirEntry::file_type()` is a hint, then `std::fs::metadata()` follows symlinks. A symlink to a directory is classified as a directory and receives size 0; a symlink to a file is a regular file whose target size/mtime are reported. A dangling symlink's stat fails, so the row remains with `file_type` hint, size 0, and no modified time; hint usually says non-directory.
- State read/written: listing `FEntry` metadata; target path is not stored separately.
- Source refs: `src/services/list_files.rs:14-43,53-59`; existing unit test `src/services/list_files.rs::tests::symlink_to_a_directory_is_listed_as_a_directory`.
- Runtime evidence: default listing capture shows `link-dir` as folder with em-dash metadata, `link-file` as 6 B and recent, and dangling link as a file row with `0 B` and `—`: `migration/oracle/captures/filesystem/listing-hidden-default.txt`.
- Edge cases observed: a broken link remains listed; kind hint and followed stat can disagree. The test additionally asserts directory link size 0 and file link target length 16.
- Error behavior: stat error is intentionally swallowed into `(0, None, d_type_is_dir)`.
- Config knobs: none.
- Platform notes: symlink fixture/test is `#[cfg(unix)]`; Windows symlink creation/privilege behavior and link metadata are unverified. Symlink displayed target is not included in this listing record.
- Depends on: INV-filesystem-001.
- Confidence: high on Linux; Windows requires direct verification.

### INV-filesystem-004 Bounded listing and large-folder streaming
- Trigger: every pane listing.
- Behavior: bounded path reads up to `LISTING_CHUNK=512` visible entries. If fewer than 512, or exactly 512 followed by EOF, it returns `complete=true`; if an additional entry remains, returns 512 and `false`. The app uses the bounded result only when complete. Larger folders clear old rows and launch a worker. Worker sends chunks of 512 in readdir order for an early partial display, then sends the entire accumulated list sorted by label as the final authoritative replacement. Hidden filtering is applied to streamed chunks; listing generations drop stale worker messages after a newer request.
- State read/written: `FEntry` vector, `listing_generation`, `listing_settled`, selection and render scroll.
- Source refs: `src/services/list_files.rs:68-145`; tests `bounded_listing_reports_complete_for_small_folders`, `bounded_listing_reports_partial_at_the_cap`, `chunked_listing_batches_entries`; `src/app.rs:989-1157`; UI Loading hint at `src/components/tab1_files_ui.rs:97-109`.
- Runtime evidence: unit tests specify 10/512/522 boundaries and exact chunk sizes `[512, 512, 76]` for 1100 entries. No live large-folder capture taken.
- Edge cases observed: during streaming only, arrival order is unsorted; final list sorts. Empty chunks (e.g. hidden-only chunks) are suppressed. An entry error is logged/skipped and traversal continues.
- Error behavior: if streaming fails before any entries, worker returns an empty completed list; UI ultimately shows empty listing, without preserving stale rows or surfacing a listing error.
- Config knobs: hidden flag affects bounded listing and worker-side chunk filtering; chunk size constant 512.
- Platform notes: reads and metadata for the small-folder bounded path occur synchronously in the caller; large folder scanning moves to a worker. Filesystems whose `d_type` is unavailable use false as the fallback hint if metadata also errors.
- Depends on: INV-filesystem-001, pane state/generations.
- Confidence: high from unit tests and code; no dynamic slow/network filesystem timing evidence.

### INV-filesystem-005 Metadata dialog and recursive folder size
- Trigger: `?` on a selected item.
- Behavior: dialog opens with fast lines `Name`, `Path`, `Kind`, `Hidden`; a worker adds stat lines. For files, `Size: {human} ({bytes} bytes)`, `Added`, `Modified`; for directories, size is omitted until a separate cancellable recursive walk supplies `Size: … ({items} items)`. “Added” uses creation time if available, otherwise modified time. Dialog key `x` cancels a directory walk while retaining partial measurement; `r` restarts; any other key closes while walk continues/cache remains. Walks skip symlinks (file_type is used without following), recurse directories, sum regular-file lengths and allocated-size estimate; unreadable directories/entries are silently skipped. Progress callback every `PROGRESS_STEP` files.
- State read/written: `InfoDialog`, background `size_cache`, size-walk registry; no permission bits or owner fields appear in info output.
- Source refs: `src/app.rs:3079-3125,3134-3343`; `src/services/file_info.rs:188-260,311-370`; handler dialog precedence `src/handler.rs:146-166`.
- Runtime evidence: source/test evidence only in this slice; existing operations tests `info_dialog_shows_metadata`, `folder_size_walk_streams_progress_and_caches`, `multi_info_dialog_sums_files_and_folders` cover portions. No screenshot of `?` dialog captured (the PTY key event was not confirmed).
- Edge cases observed: `created()` absence uses modified fallback; metadata error text is exactly `Error reading metadata`; pre-epoch/failed times become `unknown` in the info dialog.
- Error behavior: metadata errors produce the exact line above. Recursive `read_dir` errors and per-entry metadata errors do not escape or report.
- Config knobs: none.
- Platform notes: creation timestamp availability differs; allocated size uses platform-specific metadata in `file_info.rs`; Windows semantics need CI/runtime verification.
- Depends on: INV-filesystem-001; info dialog/input behavior is also owned by other slices.
- Confidence: medium; source and tests strong, direct dialog runtime capture missing.

### INV-filesystem-006 Listing errors and refresh behavior
- Trigger: navigation, `list_files_from_selected_folder`, toggle hidden, create/delete/transfer completion; transfer-destination live sync.
- Behavior: there is no filesystem directory watcher. Ordinary external edits do not automatically refresh an open pane. Refresh happens when app action requests a relist. While a transfer runs, destinations or descendant folders are relisted at most once per second if the pane has not been manually navigated; after completion both panes are listed again. Pane listing calls use a worker only when the bounded listing is incomplete or errors. Drive discovery is separate: background poll every 2 seconds and UI updates only when drive vector changes.
- State read/written: generation guarded pane listings; transfer destination path/reveal target/last refresh; drive vector+generation cache.
- Source refs: `src/app.rs:863-881,956-967,989-1103,3685-3745`; `tests/drives_refresh_test.rs::tick_leaves_drive_list_populated_after_poll`, `refresh_drives_never_blocks`.
- Runtime evidence: static search found no filesystem watcher library/API (`rg -n "notify|watch\(|FSEvents|ReadDirectoryChanges|inotify" Cargo.toml src tests`); default listing capture shows initial fixture state only. No live external-change interval test performed.
- Edge cases observed: stale listing worker results are discarded by generation; transfer live refresh skips a pane once the user navigates in it; descendant-folder test uses string prefix `starts_with(format!("{}/", dest_dir))`.
- Error behavior: `read_dir(path)` errors are returned by service. Async worker discards an initial listing error into empty list, with no visible error notice. Per-entry errors are printed to stdout as `Error reading entry: {e}`.
- Config knobs: none.
- Platform notes: descendant path checks are string-prefix-based and use `/` separators; likely mismatches Windows path separators/canonicalization. Not verified dynamically on Windows.
- Depends on: INV-filesystem-004, file operation completion events.
- Confidence: high for code behavior/no watcher; dynamic observation for external edits remains outstanding.

### INV-filesystem-007 Permissions and special files
- Trigger: filesystem listing and text-editor opening.
- Behavior: directory rows do not expose permission mode/owner/ACL. Listing only identifies directory-vs-other, followed target byte size, and mtime. FIFO from fixture is shown as ordinary file row with 0 B; listing itself does not identify sockets/devices as distinct types. Editor uses `metadata.permissions().readonly()` for read-only state and preserves original permissions on save; Unix test sets read-only mode and confirms saving leaves content unchanged.
- State read/written: `FEntry` has no permissions or special-file kind. `EditState` keeps permissions and readonly.
- Source refs: `src/services/list_files.rs:1-11,23-43`; `src/app.rs:1243-1313,1403-1414`; test `src/app.rs::read_only_file_does_not_save_edits` (Unix permission construction under `#[cfg(unix)]`, inspect exact test attributes before portability decisions).
- Runtime evidence: FIFO row is in `listing-hidden-default.txt`; no permission/dialog runtime capture.
- Edge cases observed: special files are neither followed into an operation nor guarded in listing; downstream preview/open behavior is outside this slice and should avoid blocking on FIFO.
- Error behavior: inaccessible metadata collapses to size zero and missing mtime; no permission-denied display distinction.
- Config knobs: none.
- Platform notes: `Permissions::readonly()` is coarse on Unix (not a full mode/ACL model), and Windows permission semantics differ. `FEntry.path` UTF-8/lossy conversion risk described in INV-filesystem-001.
- Depends on: INV-filesystem-001 and preview/editor behavior.
- Confidence: medium; source supports schema and code, runtime FIFO classification confirmed.

## Unclaimed or uncertain
- `src/services/list_files.rs`, `src/services/file_info.rs`, relevant app/renderer paths, `tests/sort_test.rs`, `tests/drives_refresh_test.rs` are claimed. Full info-dialog text UX and direct navigation keys are shared with core/input/rendering slices.
- Runtime key input from tmux was accepted for `w` to open Home, but `.` and `?` did not produce a reliably attributable listing/dialog state change in this run; don’t treat the attempted follow-up capture as evidence of the toggle or metadata modal. Repeat with PTY driver after confirming exact key translation.
- Access-denied `read_dir`, iterator-level error, non-UTF-8 filename, birth-time and Windows hidden-attribute observations were not safely exercised. The app-level async listing error is silently represented as empty; a read-dir error was not induced dynamically.
- No watcher API/dependency was found by repository search, but external-change behavior was not demonstrated with a live interval test.
