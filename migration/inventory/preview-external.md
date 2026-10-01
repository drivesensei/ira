# Inventory: Preview and external integration
Explorer run: 2026-10-01 | TUI commit: `1cad4ce43cc72d52d4cc4eef920e0da22cb69568` | Files claimed: `src/app.rs`, `src/handler.rs`, `src/ui/mod.rs`, `src/components/preview_ui.rs`, `src/components/tab1_files_ui.rs`, `src/services/thumbnails.rs`, `src/services/clipboard.rs`, `src/services/drives.rs`, `Cargo.toml`; read-only searches across `src/**` for git, remote, plugin, hook, and command-mode integrations.

## Features

### INV-PE-001 Preview presentation modes
- Trigger: `v` in normal mode, once an active pane is selected.
- Behavior: active pane cycles `Details → Column → Grid → Off → Details`; each transition reports exact status text `Preview: details|column|grid|off`. Details is the default on fresh app state; preview mode is stored per pane and TUI state serialization maps off/column/grid/details to 0/1/2/3 (unknown bytes become Off). Preview mode is not a file viewer navigation mode; `v` changes pane layout.
- State read/written: `Pane.preview_mode`; persistent pane preview bytes.
- Source refs: `src/app.rs:92-129`, `src/app.rs:1881-1904`, `src/app.rs:3958-3960`, `src/app.rs:4015-4018`, `src/handler.rs:274-275`.
- Runtime evidence: `migration/oracle/captures/preview-external/preview-modes.md` (PTY, starting in Details, `v` to Column, subsequent `v`s to Grid/Off/Details; exact status observed).
- Edge cases observed: mode is per pane (source test `preview_modes_are_independent_per_pane`); previews are gated by selected visible entry; no selected entry gives ` select a file `.
- Error behavior: none for mode changes.
- Config knobs: none found.
- Platform notes: graphic image layer may use terminal protocols/terminal overlay; this is terminal-bound presentation.
- Depends on: core pane state, selection/filter rows, split layout.
- Confidence: high (runtime mode/status and source tests).

### INV-PE-002 Text preview content and bounds
- Trigger: `v` to Column with a text-classified file selected.
- Behavior: reads at most 256 KiB from file head. NUL anywhere in the sampled head marks binary; invalid UTF-8 is lossy-decoded. Text lines render directly as terminal cells; tabs become four spaces; content lines are clipped to preview viewport height; over-cap content appends exact ` … truncated` marker. Empty text displays ` (empty file) `; NUL-sniffed text displays ` binary file `; while async read is pending UI displays ` loading… `.
- State read/written: selected file bytes; in-memory text preview cache/pending queue.
- Source refs: `src/services/thumbnails.rs:449-463`, `src/components/preview_ui.rs:102-153`, `src/app.rs:1440-1460`.
- Runtime evidence: `migration/oracle/captures/preview-external/text-preview-editor.md`: fixture `README` visibly rendered `first line` and `second line` in Column mode.
- Edge cases observed: static unit tests cover 256-KiB cap, NUL, extensionless/dotfile text; runtime probe used a 23-byte text fixture only.
- Error behavior: worker read failure enters failure state; UI currently falls through `None` to loading unless marked failed by thumbnail event flow. Runtime unreadable-file path not exercised.
- Config knobs: none.
- Platform notes: decode uses lossy UTF-8 while the in-app editor refuses non-UTF-8; line-ending fidelity is special-cased only in editor.
- Depends on: preview mode, async decode worker, pane selection, theme/rendering.
- Confidence: high on source behavior; medium on runtime edge behavior.

### INV-PE-003 In-app text editor is offered from text Column preview
- Trigger: press `Tab` while active pane shows an editable text-classified selection in Column mode.
- Behavior: Tab first opens an in-app `ratatui_textarea` buffer and focuses it; title shows `Editing — Ctrl+S save · Esc exit`, and dirty buffer adds ` *`. `Tab` while editing discards unsaved edits and returns to normal focus cycling; on the same selection, editor is not immediately re-offered until a later focus cycle. Save is Ctrl+S. On the read-only-permission path preview says `(read-only)` and Tab behavior is not offered as editable. Image/non-text selections do not offer this editor. This is an IRA-specific built-in editor, not an `$EDITOR` subprocess.
- State read/written: editor buffer and dirty state; file bytes only on explicit save.
- Source refs: `src/app.rs:1212-1315`, `src/app.rs:1317-1349`, `src/app.rs:2495-2514`, `src/components/preview_ui.rs:23-79`, `src/handler.rs:48-78`.
- Runtime evidence: `migration/oracle/captures/preview-external/text-preview-editor.md`: with `README` selected after `v` to Column, `Tab` produced `Editing — Ctrl+S save · Esc exit` and put a block cursor at first line; `Esc` returned to file listing. No fixture content was edited.
- Edge cases observed: edit limit is 5 MiB; binary text-like files refuse editing; invalid UTF-8 is preview-only; editor canonicalizes path and refuses save if symlink target/path identity changes; save refuses externally changed mtime; CRLF normalizes LF while editing and re-encodes CRLF at save; permissions are preserved; read-only is checked; save is temp-write + rename, then preview cache invalidation. Test names: `editor_opens_saves_and_invalidates_caches`, `read_only_files_open_without_save`, `preview_focus_is_only_offered_for_editable_text`, `save_refuses_after_external_modification`, `paste_reaches_the_editor`, `tab_while_editing_discards_and_does_not_reopen_same_selection`.
- Error behavior: exact strings include `file too large to edit (> 5 MB)`, `binary file — not editable`, `non-UTF-8 file — read-only preview only`, `file path changed on disk — press Esc and reopen`, `file changed on disk — press Esc and reopen`, `{name} is read-only`, `save failed: {error}`, `Saved {name}`.
- Config knobs: none.
- Platform notes: terminal text-area key semantics; paste support flows through input handler. No syntax highlighting.
- Depends on: INV-PE-002, file metadata/permissions, input routing.
- Confidence: high (runtime opening and source tests; save effects supported by tests, not mutated in runtime probe).

### INV-PE-004 Image preview formats and decode behavior
- Trigger: `v` with a supported image selected (Details and Column use large surface; Grid uses grid surface).
- Behavior: supported raster extensions are case-insensitive `.png`, `.jpg`, `.jpeg`, `.gif`, `.bmp`, `.webp`. Decoder guesses actual content format rather than trusting extension, applies EXIF orientation, and caps decoded allocation at 256 MiB; thumbnail longest side capped at 768 px. Failed decode shows ` cannot decode `.
- State read/written: source file; in-memory/rendered and optional disk thumbnail caches.
- Source refs: `src/services/thumbnails.rs:27-30`, `src/services/thumbnails.rs:104-160`, `src/services/thumbnails.rs:271-309`, `src/services/thumbnails.rs:466-473`, `src/components/preview_ui.rs:156-192`.
- Runtime evidence: `migration/oracle/captures/preview-external/preview-modes.md`; Grid rendered a tile layout for fixture entries, but generated PNG validity/render success was not confirmed, so no claim that the test image decoded.
- Edge cases observed: directories and unsupported extensions are rejected; image body-sniffing can decode despite misleading extension by source; allocation cap intended for bombs.
- Error behavior: image decode error becomes generic ` cannot decode `; no underlying diagnostic is shown.
- Config knobs: none.
- Platform notes: terminal image protocol picker or braille block fallback described in `src/services/thumbnails.rs:1-7`; rich image display depends on the host terminal's supported graphics protocol.
- Depends on: preview modes, terminal picker/capability probe, worker queue.
- Confidence: high on source-supported extensions and failure UI; medium on protocol behavior without varied terminals.

### INV-PE-005 Video preview uses optional ffmpeg
- Trigger: selected extension `.mp4`, `.mov`, `.m4v`, `.webm`, `.mkv`, or `.avi` in a preview mode.
- Behavior: preview is enabled only if startup background probe can execute `ffmpeg -version` successfully. Decoder shells to `ffmpeg` and extracts one video frame to PNG, scaled to a maximum 512-pixel width/frame dimension; child stdin/stderr are null, stdout piped; 10-second watchdog kills a hung child. No playback controls/audio; this is a representative still image only.
- State read/written: source video, `ffmpeg` process, thumbnail caches.
- Source refs: `src/services/thumbnails.rs:99-102`, `src/services/thumbnails.rs:149-151`, `src/services/thumbnails.rs:312-377`; startup probe `src/app.rs:1188-1205`; support gate `src/app.rs:1774-1785`.
- Runtime evidence: source/tests only; fixture `clip.mp4` was deliberately not sent to an installed media player and is invalid content. Preview placeholder behavior under missing ffmpeg was not captured.
- Edge cases observed: missing command disables support; command present but incompatible media yields decode failure. 10-second watchdog. Exact loading/unsupported messaging in `src/components/preview_ui.rs:198-204`.
- Error behavior: missing dependency placeholder ` install ffmpeg for video previews `; bad media generic ` cannot decode `.
- Config knobs: PATH controls discovery; no config key.
- Platform notes: optional external runtime process availability differs across packaged platforms.
- Depends on: INV-PE-004 preview surface, PATH/dependency probing.
- Confidence: high on source implementation; low on runtime because no valid media fixture used.

### INV-PE-006 HEIC/HEIF preview uses optional ffmpeg
- Trigger: `.heic` or `.heif` selection and successful `ffmpeg -version` probe.
- Behavior: uses the same one-frame ffmpeg image extraction route as video; ffmpeg must support the HEVC-coded file.
- State read/written: source file, process, thumbnail cache.
- Source refs: `src/services/thumbnails.rs:112-113`, `src/services/thumbnails.rs:149-152`, `src/services/thumbnails.rs:292-296`, `src/app.rs:1774-1785`.
- Runtime evidence: source only; no HEIC fixture.
- Edge cases observed: ffmpeg build without HEVC decoding still gives decode failure although executable probe passed.
- Error behavior: missing ffmpeg placeholder ` install ffmpeg for HEIC previews `; otherwise ` cannot decode `.
- Config knobs: PATH only.
- Platform notes: external binary packaging.
- Depends on: INV-PE-005.
- Confidence: medium (source route explicit; no runtime file).

### INV-PE-007 PDF first-page preview uses optional Poppler
- Trigger: `.pdf` selection and successful `pdftoppm -v` startup probe.
- Behavior: invokes `pdftoppm -png -f 1 -l 1 -scale-to 512 -singlefile <path> <temp-root>`, waits for child, loads `<temp-root>.png`, then removes PNG. Uses same 10-second watchdog; page 1 only, no document navigation or text extraction.
- State read/written: source PDF; temporary PNG under OS temp dir; thumbnail caches.
- Source refs: `src/services/thumbnails.rs:115-118`, `src/services/thumbnails.rs:379-447`, `src/app.rs:1188-1205`, `src/components/preview_ui.rs:205-207`.
- Runtime evidence: source only; fixture `doc.pdf` contains invalid placeholder text and was not decoded.
- Edge cases observed: temp root has PID + atomic sequence for parallel extractions; output is removed even on decode result path; child failure/empty output fails generically.
- Error behavior: missing dependency placeholder ` install poppler (pdftoppm) for PDF previews `; decode failure ` cannot decode `.
- Config knobs: PATH only.
- Platform notes: Poppler executable is an optional dependency; package inclusion is unknown from source inspection.
- Depends on: INV-PE-004.
- Confidence: medium (implementation visible, no valid PDF runtime).

### INV-PE-008 Preview exclusions and contextual placeholders
- Trigger: no selected row, selected directory, unsupported extension, preview dependency unavailable, or preview decode still pending/fails.
- Behavior: text file gets cell-native text preview only in Column mode. Image preview is absent for directories and unsupported formats. Preview UI says ` select a file `; ` folders have no image preview `; ` format not supported (png/jpg/gif/bmp/webp/mp4/mov/heic/pdf) `; and optional dependency prompts listed in INV-PE-005/007. While loading image it says ` loading {filename}… `; failed decode says ` cannot decode `. A modal over the files area suspends protocol preview and draws ` preview paused `.
- State read/written: selected visible entry, modal overlay state, decode queue.
- Source refs: `src/app.rs:1212-1217`, `src/app.rs:1774-1785`, `src/app.rs:1934-1960`, `src/app.rs:1866-1879`, `src/components/preview_ui.rs:85-99`, `src/components/preview_ui.rs:176-219`.
- Runtime evidence: `migration/oracle/captures/preview-external/text-preview-editor.md` (no selection showed ` select a file `; selecting README switched to text content); `preview-modes.md` shows grid, off, details.
- Edge cases observed: a recognized extension with wrong content still classifies to its extension's kind; in-process image format sniffing can then decode, while video/PDF shell-outs depend on the extension route.
- Error behavior: generic placeholders above; no inline stack trace.
- Config knobs: none.
- Platform notes: `preview paused` is needed because sixel/iTerm2 images can float above cell rendering.
- Depends on: INV-PE-001, INV-PE-002, INV-PE-004–007, modal state.
- Confidence: high.

### INV-PE-009 Thumbnail async scheduling, prefetch, and cache
- Trigger: preview becomes visible; prefetch near cursor in Column or visible/out-of-window Grid entries.
- Behavior: bounded worker pool at most six workers; high-priority visible requests cap 32 and prefetch queue cap 256; visible work is dequeued before prefetch and queues poll every 5ms. Column prefetch covers eight rows ahead and up to two above, excluding selected; Grid prefetch enumerates eligible rows. Disk cache key combines path, mtime, size; optional `IRA_THUMBNAIL_CACHE_DIR` override; default cache is `$XDG_CACHE_HOME/ira/thumbnails` or platform cache equivalent. Cache write is atomic, cache cap 512 PNGs, oldest-by-mtime pruned at startup; cache read self-heals corrupt entries by re-decoding. In-memory success/failure/pending caches dedupe jobs; failed request stays marked failed in session.
- State read/written: worker channels, memory thumb cache, disk cache.
- Source refs: `src/services/thumbnails.rs:27-63`, `src/services/thumbnails.rs:99-102`, `src/services/thumbnails.rs:215-269`, `src/services/thumbnails.rs:271-309`, `src/services/thumbnails.rs:475-557`, `src/services/thumbnails.rs:570-625`, `src/app.rs:1800-1864`, `src/app.rs:1963-2024`.
- Runtime evidence: preview UI visibly showed `loading…` then text preview completed in the direct PTY capture. Image cache/decode did not get a validated runtime sample.
- Edge cases observed: full visible queue drops a request, retried on later render; full prefetch queue drops low priority work; absent cache directory means no disk cache, preview still works; prune errors silently ignored.
- Error behavior: decode failures are memoized as failed to prevent a perpetual loading indicator.
- Config knobs: `IRA_THUMBNAIL_CACHE_DIR`.
- Platform notes: path/cache dir resolved by `dirs_next`; temp handling uses OS temp directory.
- Depends on: preview format decode, selection, layout.
- Confidence: high source; medium runtime.

### INV-PE-010 Open selected file with system default app
- Trigger: right-arrow / open key (`→` or `l`) on a non-directory entry; directories navigate instead.
- Behavior: call `open_file` detached. Linux first tries `gio open <path>` with stdio null; if process spawn itself fails, fallback is `open::that_detached(path)`. macOS/Windows use `open::that_detached`. It does not present a chooser and does not inspect MIME type itself. A running child process that later exits nonzero is not surfaced by the Linux spawn-success branch.
- State read/written: path passed to platform launcher.
- Source refs: `src/app.rs:2206-2229`, `src/app.rs:4698-4721`, `src/handler.rs:341-343`, `Cargo.toml` dependency `open`.
- Runtime evidence: source only; no handler was launched because no real host application was to be opened during this isolated fixture check.
- Edge cases observed: Linux prefers GIO to preserve `Terminal=true` desktop handlers; fallback error exact shape `Failed to open '{path}': {err}`.
- Error behavior: open failure sets status error; file remains selected.
- Config knobs: desktop default file associations and `PATH` on Linux; no in-app config.
- Platform notes: behavior is OS association-driven.
- Depends on: selection/navigation and platform open service.
- Confidence: high source; runtime launch intentionally unverified.

### INV-PE-011 Open-with chooser and editor/pager/shell spawning are absent
- Trigger: not present. Normal Enter/right-arrow uses only OS default handler (INV-PE-010); `Tab` is IRA's internal text editor when text Column preview applies (INV-PE-003); `0` launches a terminal emulator in current folder (INV-PE-012).
- Behavior: no chooser, configurable `$EDITOR`, `$PAGER`, or shell escape command is registered in `src/handler.rs`; source search found only built-in editing and default-opener invocation.
- State read/written: none.
- Source refs: `src/handler.rs:228-361`, `src/app.rs:1223-1315`, `src/app.rs:2206-2229`, `src/app.rs:2530-2545`; search in `src/**` and Cargo.toml for `open-with|EDITOR|PAGER|shell escape` (only code comments or embedded editor references, no dispatch/config).
- Runtime evidence: `migration/oracle/captures/preview-external/integration-absence.md` states no open-with action appeared during this session; runtime not exhaustive, source absence is primary.
- Edge cases observed: default OS handler might itself be a terminal editor on Linux; GIO supports terminal desktop entries. That external editor is outside IRA's editor UX.
- Error behavior: only default opener errors; there is no no-association picker.
- Config knobs: no editor/pager/shell integration config found.
- Platform notes: OS association might launch a terminal process.
- Depends on: INV-PE-003, INV-PE-010, INV-PE-012.
- Confidence: high for source search scope; no literal runtime command discovery exists to exercise.

### INV-PE-012 Spawn a native terminal in current directory
- Trigger: `0` while a pane folder is open.
- Behavior: launches the first available candidate detached with cwd set to active folder. Linux candidate preference: `foot --working-directory DIR`, `alacritty --working-directory DIR`, `kitty --directory DIR`, `gnome-terminal --working-directory=DIR`, `konsole --workdir DIR`, `xfce4-terminal --working-directory DIR`, `xterm`. macOS selects terminal matching `TERM_PROGRAM` first, then PATH-installed Kitty/Alacritty/WezTerm, user app bundle iTerm/WezTerm/Ghostty, and Terminal.app. Windows tries `wt -d DIR`, then `cmd /C start "" /D DIR cmd.exe`. Missing folder says exact `No folder open to start a terminal in.`; no executable says `No terminal emulator found (tried {list}).`
- State read/written: current pane path and OS child process.
- Source refs: `src/handler.rs:232-235`, `src/app.rs:2530-2545`, `src/app.rs:4483-4600`, `src/app.rs:4603-4624`, `src/app.rs:4668-4687`.
- Runtime evidence: source only; launching another terminal was intentionally not attempted.
- Edge cases observed: candidate existence is `PATH` file existence, then spawn; child stdio is null; cwd set to active dir. macOS app-bundle probing checks `/Applications` and `$HOME/Applications`.
- Error behavior: exact strings above.
- Config knobs: `PATH`, `TERM_PROGRAM`, installed app bundles.
- Platform notes: desktop-specific per OS terminal launch logic; terminal-only utility.
- Depends on: current folder, platform launcher.
- Confidence: high source; runtime spawn unverified.

### INV-PE-013 Reveal in OS file manager
- Trigger: `-` on normal screen.
- Behavior: Finder gets `open -R <target>`; Windows gets `explorer /select,<target>` (comma in path quoted); Linux opens target directory if directory, or containing directory if file via preference list `xdg-open`, `nautilus`, `dolphin`, `thunar`, `nemo`, `caja`, `pcmanfm`. The Linux implementation cannot select an individual file portably. If no candidate starts, status `No file browser found (tried {list}).`; without a pane folder `No folder open to reveal.`
- State read/written: selected target or pane folder.
- Source refs: `src/handler.rs:261-265`, `src/app.rs:2547-2580`, `src/app.rs:4510-4555`, `src/app.rs:4668-4687`.
- Runtime evidence: source only; intentionally did not open host file-manager window.
- Edge cases observed: no selection uses current folder; Linux file target is parent folder.
- Error behavior: exact messages above.
- Config knobs: PATH on Linux; default Finder/Explorer behavior elsewhere.
- Platform notes: platform asymmetry (Linux cannot reliably reveal/select a file).
- Depends on: selection and platform launcher.
- Confidence: high.

### INV-PE-014 Copy current folder path to clipboard
- Trigger: `]` in normal mode.
- Behavior: copies active pane folder path, not selected filename. Native command tries `wl-copy`, `xclip -selection clipboard`, `pbcopy` (Unix) in this fixed order; Windows uses `clip`; if no native tool succeeds, writes OSC 52 `ESC ] 52;c;BASE64 BEL` to `/dev/stderr`. Empty text returns false. Statuses: `Folder path copied to clipboard` or `Failed to copy the folder path to the clipboard.`
- State read/written: system clipboard or terminal OSC52 request.
- Source refs: `src/handler.rs:255-260`, `src/app.rs:2781-2790`, `src/services/clipboard.rs:1-90`.
- Runtime evidence: source only; PTY had no isolated clipboard provider, key intentionally not sent to avoid touching the surrounding desktop clipboard.
- Edge cases observed: under SSH, OSC52 may request clipboard on local terminal if accepted by host terminal; acceptance isn't verified by IRA.
- Error behavior: boolean best-effort summary only.
- Config knobs: available clipboard programs and terminal OSC52 policy.
- Platform notes: Unix includes `pbcopy` probe; Windows only `clip` then attempts `/dev/stderr` fallback (which ordinarily is unavailable), so Windows path may return false if `clip` cannot start.
- Depends on: active pane folder and terminal/OS clipboard.
- Confidence: high source; runtime intentionally unverified.

### INV-PE-015 Linux removable media mount/eject is the only on-demand mount integration
- Trigger: drive shortcut selects unmounted Linux drive; eject uses Ctrl+- while on a current drive.
- Behavior: Linux `lsblk` lists devices; unmounted drive selection runs `udisksctl mount -b DEVICE`; errors become `Failed to mount {device}: {error}`. Successful eject runs `udisksctl unmount -b DEVICE`; for errors, strips a `GDBus.Error:` prefix then reports `Failed to eject {device}: {reason}`. On success, panes on mount path are redirected to Home. Non-Linux functions explicitly return Unsupported for on-demand mount/eject (desktop OS manages its own mounts).
- State read/written: platform device/mount table; pane folder after eject.
- Source refs: `src/services/drives.rs:206-289`, `src/app.rs:2080-2120`, `src/app.rs:2122-2173`, `src/handler.rs` drive dispatch (Ctrl+- noted at `src/handler.rs:261-264`).
- Runtime evidence: source only; no physical device was mounted/ejected.
- Edge cases observed: mount output parsing expects substring ` at ` and trims final period; eject only drives recognized as removable by drive-list logic; an unrecognized current mount says `No removable drive is mounted at {path}`.
- Error behavior: exact strings above; service failure includes stderr text when nonempty.
- Config knobs: system block devices and udisks2.
- Platform notes: Linux-only; macOS/Windows use native OS mechanisms instead, but no API for manual mount exists.
- Depends on: drive discovery/platform device APIs.
- Confidence: high source; runtime not safe or possible in fixture-only context.

### INV-PE-016 Git status decorations are absent
- Trigger: not present.
- Behavior: source search found only icon/type labels for `.git`, `.gitignore`, `git` and file metadata; no repository status query, changed-file overlay, branch, or git subprocess in the TUI.
- State read/written: none.
- Source refs: `src/theme/icons.rs:152,285,310,329,499`; project-wide source search for git/status/git2 found no integration.
- Runtime evidence: source inventory only; fixture did not contain repository state.
- Edge cases observed: an icon/category is not a Git status feature.
- Error behavior: none.
- Config knobs: none.
- Platform notes: none.
- Depends on: none.
- Confidence: medium-high (source-wide token search; final auditor should still check orphan files).

### INV-PE-017 Remote filesystems / VFS are absent
- Trigger: not present.
- Behavior: no SSH/SFTP/FTP/S3 or remote/VFS backend was found in `src/**` or Cargo.toml. Clipboard OSC52 over SSH only means the TUI may run in a remote shell; it is not a remote filesystem integration.
- State read/written: none.
- Source refs: `src/services/clipboard.rs:1-4,85-89`; source and dependencies search for `ssh|ftp|sftp|s3|remote|vfs` found no filesystem layer integration.
- Runtime evidence: source search only.
- Edge cases observed: ordinary mounted/network filesystem paths may appear through `std::fs`, but remote protocol support was not found.
- Error behavior: normal local filesystem operations.
- Config knobs: none.
- Platform notes: generic mounted volumes can be network-backed outside IRA's control.
- Depends on: filesystem layer (slice 5).
- Confidence: medium-high.

### INV-PE-018 Plugins, user scripting hooks, and command mode are absent
- Trigger: not present; no `:` command mode, alias/history/completion, `!` shell escape, plugin registry, or user command hook.
- Behavior: key handler has direct match-based commands; `*` shows fixed help; no parsed command registry found. `Cargo.toml` has no plugin/script runtime dependency.
- State read/written: none.
- Source refs: `src/handler.rs:228-361`; `src/ui/mod.rs:171-224`; `Cargo.toml`; source-wide search for `plugin|hook|command palette|command mode|shell escape|EDITOR|PAGER` found only Rust's panic hook, documentation/context references, and unrelated theme probes.
- Runtime evidence: `migration/oracle/captures/preview-external/integration-absence.md`; help surface describes fixed keys, no command prompt.
- Edge cases observed: rename/new/goto/search modals are text inputs but are not generic command mode.
- Error behavior: none.
- Config knobs: none.
- Platform notes: none.
- Depends on: input system and config only if later added.
- Confidence: medium-high source search, runtime help inspection.

### INV-PE-019 Drag/drop integration is absent
- Trigger: not present.
- Behavior: no drag/drop event handler, OS drop target, or file-drop API integration appears in the Ratatui event loop or handler. Selection and transfer use key-driven copy/move workflow.
- State read/written: none.
- Source refs: `src/event.rs` event enum/reader; `src/handler.rs:228-361`; project-wide search for drag/drop APIs and crossterm mouse usage did not identify a file-drop path.
- Runtime evidence: source-only; terminal UI does not receive desktop OS drop events.
- Edge cases observed: terminal mouse handling is covered by input slice, not OS drag/drop.
- Error behavior: none.
- Config knobs: none.
- Platform notes: inherently desktop-only enrichment candidate.
- Depends on: platform input shell, filesystem transfer actions.
- Confidence: medium.

## Unclaimed or uncertain
- `src/services/overlay/{macos,windows}.rs`, `src/services/overlay/mod.rs`, `src/services/picker_probe.rs`, `src/services/blocks.rs`: preview rendering's protocol encoding, terminal size/font/window capture, and native floating overlay are terminal-image subsystem detail. They require a focused rendering/platform audit; this inventory only captures that image preview output is terminal protocol/native overlay-backed. Those modules were not exhaustively reviewed for all terminal-specific branches.
- Valid image/video/HEIC/PDF decoding outcomes, external command failures, OS open associations, clipboard writes, mount/eject and actual terminal emulator discovery were not triggered dynamically for isolation/safety. Source implementation and unit-test names are evidence but do not prove runtime success on all OSes.
- Potential user expectation: shell-based `$EDITOR` is not supported; Tab launches the IRA built-in text area only for text Column preview.
