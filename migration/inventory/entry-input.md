# Inventory: entry points, CLI, startup, and input
Explorer run: 2026-10-01 | TUI commit: `1cad4ce` (peeled `tui-oracle-baseline`) | Files claimed: `src/main.rs`, `src/event.rs`, `src/handler.rs`, startup callsites in `src/tui.rs`; focused state/method refs in `src/app.rs`; help rendering in `src/ui/mod.rs`

## Features

### INV-ENTRY-001 `--version` / `-V` early exit
- Trigger: either exact argument occurs anywhere after argv[0].
- Behavior: prints exactly `ira 0.1.21` plus newline to stdout and returns success before terminal probing, state loading, or raw mode. It wins when both version and check-terminal args occur.
- State read/written: none observed.
- Source refs: `src/main.rs:11-24`.
- Runtime evidence: `migration/oracle/captures/entry-input/version.txt` (`exit=0`; isolated HOME).
- Edge cases observed: argument is recognized regardless of position; all other arguments are not validated by a parser.
- Error behavior: none for this path.
- Config knobs: none.
- Platform notes: intended for packager checks; usable without a TTY.
- Depends on: none.
- Confidence: high.

### INV-ENTRY-002 `--check-terminal` diagnostic exit
- Trigger: exact `--check-terminal` argument if no version argument is present.
- Behavior: loads persisted theme, probes theme/font and terminal image protocol, prints version, truecolor, icon choice/reason, Nerd glyph source, theme/source, protocol/source, cell size, and theme file path/existence to stdout; then returns success without starting the TUI.
- State read/written: reads `~/.config/ira/state` theme field and `theme.toml`/font/environment inputs; no writes observed.
- Source refs: `src/main.rs:21-24,93-177`, `src/services/picker_probe.rs:20-111`, `src/services/state.rs:48-70`.
- Runtime evidence: `migration/oracle/captures/entry-input/check-terminal.txt` (`exit=0`, `IRA_IMAGES=blocks`, isolated HOME; global font inventory affects output).
- Edge cases observed: no TTY was required for this run; diagnostic output is host/font/env dependent. A broken/unavailable config path is reported as `not found` rather than a nonzero exit.
- Error behavior: no explicit error path in the diagnostic function; picker fallback is rendered as the protocol/source.
- Config knobs: `IRA_IMAGES`, theme/session state, icon/font/color detection env inputs (full theme env inventory belongs to the configuration/theme slice).
- Platform notes: picker probing contains Windows ConPTY/VT behavior; see `src/services/picker_probe.rs`.
- Depends on: theme/config loading and terminal capability detection.
- Confidence: high for current host; platform-specific output still needs macOS/Windows captures.

### INV-ENTRY-003 Ordinary startup and initialization order
- Trigger: no recognized early-exit argument.
- Behavior: terminal image protocol/font-size query runs before raw mode/event reader; selects stdout as draw stream for graphics protocol or Windows, otherwise stderr; initializes `App`, installs picker and truecolor; constructs Crossterm backend/terminal and 500ms event handler; enables raw mode, alternate screen, mouse capture, bracketed paste; draws immediately; enters event loop. On exit persists session, closes overlay, deletes kitty images if needed, resets terminal and shows cursor.
- State read/written: loads app session/bookmarks/theme and folder listings; writes persisted session on ordinary quit.
- Source refs: `src/main.rs:26-90`, `src/tui.rs:14-95`, `src/event.rs:24-76`, `src/app.rs:759-785`.
- Runtime evidence: `migration/oracle/captures/entry-input/initial.txt`, `capture-notes.md`; fixture-backed session file used. Process initialization took about 10-12s under tmux on this host because its terminal capability query waited for timeout.
- Edge cases observed: image probe blocks startup before first frame; direct early CLI paths bypass it. Runtime always polls drives and two pane listings during `App::new`.
- Error behavior: terminal/backend setup errors propagate from `main` as `AppResult`; event-thread poll/read/send failures panic via `expect`.
- Config knobs: `IRA_IMAGES`; `WT_SESSION`, `TERM_PROGRAM`, `LC_TERMINAL`; theme/color/font environment is owned primarily by theme/config slice.
- Platform notes: output stream choice and VT setup differ on Windows; Kitty image cleanup is emitted only for Kitty protocol.
- Depends on: theme/config, drive enumeration, session restore, filesystem listing, platform overlay.
- Confidence: high for source order and Linux startup; macOS/Windows runtime details need their own captures.

### INV-ENTRY-004 Session restore and initial pane source
- Trigger: `App::new` during ordinary startup.
- Behavior: initializes defaults; loads bookmarks; enumerates drives and starts first mounted drive if available; restores persisted state/theme; starts drive poller; requests async listings for both panes. A persisted pane folder overrides the default first mounted drive.
- State read/written: session state and bookmarks; pane paths/listing state.
- Source refs: `src/app.rs:759-785`, `src/services/state.rs:48-70,87-115`.
- Runtime evidence: fixture-backed left/right state in `/tmp/ira-entry-input-home/.config/ira/state` produced `Fixture` pane in `initial.txt`; initial file manifest is captured in `capture-notes.md`.
- Edge cases observed: even with isolated HOME, the global drive bar is populated from host drive enumeration. No operations were issued to those drives.
- Error behavior: absent/unreadable state defaults; drive list failure is ignored and leaves the drive list unset.
- Config knobs: session file under config directory.
- Platform notes: drive enumeration differs by OS.
- Depends on: config/state, bookmarks, drive listing, filesystem listing.
- Confidence: high.

### INV-ENTRY-005 Unrecognized CLI args and arguments without a TTY
- Trigger: ordinary startup with arguments other than the two recognized flags, or no args.
- Behavior: there is no clap parser/subcommand handling; unrecognized args are ignored and normal TUI startup proceeds. The TUI needs a terminal; its setup errors propagate.
- State read/written: ordinary startup state as INV-ENTRY-003.
- Source refs: `src/main.rs:11-47` (only argument checks in the entry point).
- Runtime evidence: source inspection only; no unknown-arg non-TTY runtime trial because doing so would trigger normal terminal startup.
- Edge cases observed: both recognized flags are detected by scanning all args, not only argv[1].
- Error behavior: terminal/backend errors are returned; exact OS error text is not captured.
- Config knobs: none.
- Platform notes: TTY expectations differ under ConPTY/terminal emulators.
- Depends on: INV-ENTRY-003.
- Confidence: medium; a dedicated unknown-arg runtime check would raise confidence.

### INV-INPUT-001 Press-only event loop, ignored events, and paste routing
- Trigger: crossterm keyboard, mouse, resize, focus, paste events and 500ms ticks.
- Behavior: accepts only `KeyEventKind::Press`; ignores focus gained/lost; forwards mouse and resize events but main loop does nothing for them; routes bracketed paste text to `App::handle_paste`. Tick drains async jobs/listings, drive updates, thumbnails, and notices.
- State read/written: UI/app state and async workers; paste mutates the active text/search buffer.
- Source refs: `src/event.rs:24-76`, `src/main.rs:48-69`, `src/app.rs:807-840`.
- Runtime evidence: startup capture confirms ordinary TUI frame; press/paste/focus/resize edge behavior source-only.
- Edge cases observed: repeat/release key events are ignored; plain terminal mouse events are intentionally no-op in the main event loop.
- Error behavior: event thread panics on crossterm poll/read or closed sender.
- Config knobs: none.
- Platform notes: event decoding depends on terminal/crossterm; bracketed paste exists only where terminal supports it.
- Depends on: GPUI event adapter, async job runner.
- Confidence: high from source; mouse/focus/paste need runtime characterization.

### INV-INPUT-002 Normal navigation and destination shortcuts
- Trigger: normal pane focus; arrows, `z`, `x`, digits, letters.
- Behavior: Up/Down move selection; Alt+Up/Down first moves to top/bottom and then still calls previous/next; `z`/`x` jump top/bottom; Left/Right leave/enter folder; digit `1`… maps to enumerated drive shortcut if in range (0 is separate terminal action); non-digit letters choose the matching common-folder shortcut first, else bookmark shortcut. No count prefix or multi-key sequence is present in handler.
- State read/written: active pane cursor/path/history/listing; reads dynamic drive/common-folder/bookmark shortcuts.
- Source refs: `src/handler.rs:228-245,329-359`, `src/app.rs:2080-2208`.
- Runtime evidence: fixture pane list and Home navigation were reproduced through the direct PTY driver (`migration/oracle/captures/phase0/interactive-driver.md`); the standalone `initial.txt` only records the startup view.
- Edge cases observed: digits outside available drive range do nothing; common-folder shortcut takes precedence over same bookmark character.
- Error behavior: no-op for invalid shortcut / non-directory enter.
- Config knobs: available drives, common folders, bookmarks.
- Platform notes: shortcut sets are platform/environment dependent.
- Depends on: pane state, drive/common-folder/bookmark inventories, folder navigation.
- Confidence: medium; precedence/source are clear, dynamic shortcut coverage remains.

### INV-INPUT-003 Normal file and view actions
- Trigger: normal pane focus.
- Behavior: `q` and Ctrl+C quit; `*` opens help; `n` creates entry; `[` opens go-to-path; `]` copies current folder path; `-` reveals selected entry in OS file browser; `b` toggles folder bookmark; `/` starts fuzzy search; `v` cycles image preview; `+` toggles split; backtick toggles Copy Board; `\\` cycles theme; `.` toggles hidden files; `?` opens entry info; `,` cycles Name→Size→Modified→Kind sorting; Tab switches pane/board focus.
- State read/written: selection/folder, preview/split/theme/session state, clipboard, bookmark list, overlay, async jobs.
- Source refs: `src/handler.rs:228-327`, `src/app.rs:885-897,2497-2595,2955-2978,3835-3888`.
- Runtime evidence: initial frame in `initial.txt`; live help overlay and consumed `x` dismissal in `keybindings.txt`; rendering source/test at `src/ui/mod.rs:170-229,719-760`.
- Edge cases observed: `Esc` clears a confirmed search filter; `-` uses punctuation to preserve letter shortcut pool; theme/preview actions show transient status which does not consume the next key.
- Error behavior: action-specific errors surface as modal/status state (covered by other slices).
- Config knobs: theme, persisted preview/split, bookmarks, hidden state.
- Platform notes: reveal/open behavior is OS-specific; `-` differs from Delete.
- Depends on: selection/list state, theme/config, clipboard, bookmarks, preview, split, Copy Board, platform integration.
- Confidence: high for help presentation/dismissal; most other actions require feature-specific runtime captures.

### INV-INPUT-004 Copy, move, delete, selection, and eject bindings
- Trigger: normal pane focus and selection; Ctrl/Alt/Super modifiers.
- Behavior: `c` requests copy to other pane; `m` requests move; Space toggles current entry selection; Delete requests delete (with confirmation); Backspace also invokes delete only on macOS; Ctrl+A toggles select-all/clear-all except rename; Alt/Super+A also toggles select-all; Alt/Super+I inverts selection; Ctrl+- ejects active drive. Copy/move/delete confirmations are separate modal mode.
- State read/written: per-entry selection, transfer/deletion jobs, current drive.
- Source refs: `src/handler.rs:58-76,286-318`, `src/handler.rs:293-298`, `src/app.rs:2955-2985`.
- Runtime evidence: source only; destructive action not run against real data; fixture not modified.
- Edge cases observed: terminal Super forwarding is unreliable by source comment; delete guarded by confirmation; Ctrl+A is ignored during rename.
- Error behavior: action-specific modal/status; confirmation keys in INV-INPUT-010.
- Config knobs: none.
- Platform notes: Backspace delete is macOS-only; Windows/Linux use Delete key. Drive eject platform-dependent.
- Depends on: selection model, transfer jobs, confirmation modal, drive service.
- Confidence: high from source; destructive flow requires isolated fixture capture.

### INV-INPUT-005 Rename prompt mode
- Trigger: Enter in normal pane mode.
- Behavior: opens inline rename prompt for selected entry; Esc cancels, Enter commits, Backspace removes previous char, Left/Right move cursor, Char inserts; Ctrl+C still quits globally. Other Ctrl combinations are swallowed by the global Ctrl handler.
- State read/written: selected path/name and filesystem on commit.
- Source refs: `src/handler.rs:58-90,345-347`, `src/app.rs:2987-3050`.
- Runtime evidence: rename prompt open/cancel was captured in `migration/oracle/captures/config-state/pty-session.ansi.txt`; successful rename and collision behavior are captured in `migration/oracle/captures/files-ops/rename-result.txt` and `rename-collision.txt`.
- Edge cases observed: Ctrl+A does not select all while rename is active; only Char is inserted.
- Error behavior: commit collision/IO errors become status or modal (see file-operation/error slices).
- Config knobs: none.
- Platform notes: Enter is intentionally macOS-style rename convention.
- Depends on: file listing, rename operation, status/error UI.
- Confidence: high from source.

### INV-INPUT-006 Go-to-path prompt mode
- Trigger: `[` in normal mode.
- Behavior: character and paste append path text; Backspace removes; Enter confirms (navigate or create); Esc cancels. Other keys are ignored.
- State read/written: prompt buffer, pane path/listing or filesystem if target is created.
- Source refs: `src/handler.rs:93-104,255-257`, `src/app.rs:2660-2794`, `src/app.rs:807-840`.
- Runtime evidence: source only; the go-to-path prompt was opened but its input behavior was not characterized in this run.
- Edge cases observed: paste cleaning strips one trailing LF then one trailing CR; pasted text route only handles active prompt.
- Error behavior: path resolution/create errors are surfaced by app status.
- Config knobs: none.
- Platform notes: path syntax must preserve platform paths; current app stores prompt in string (implementation concern for migration).
- Depends on: path parsing, folder navigation/create, paste.
- Confidence: high.

### INV-INPUT-007 Create-new prompt mode
- Trigger: `n` in normal mode.
- Behavior: character and paste insert at prompt cursor; Backspace removes; Left/Right move cursor; Enter creates; Esc cancels. Other keys ignored.
- State read/written: prompt text/cursor, filesystem on confirm.
- Source refs: `src/handler.rs:106-117,252-254`, `src/app.rs:2796-2865`, `src/app.rs:807-840`.
- Runtime evidence: `migration/oracle/captures/entry-input/new-entry.txt` shows `n` opening the prompt, typed name text appearing, and Esc canceling before creation.
- Edge cases observed: Ctrl+C is globally handled before prompt routing and quits instead of inserting text.
- Error behavior: invalid name/create errors surface through status.
- Config knobs: none.
- Platform notes: filename validation must remain OS-aware.
- Depends on: filesystem create, prompt, error/status.
- Confidence: high.

### INV-INPUT-008 Fuzzy-search input and confirmed filter
- Trigger: `/` in normal mode.
- Behavior: Char appends query, Backspace removes, Up/Down changes candidate (Alt variants jump top/bottom then navigate), Right enters selected folder, Enter confirms a persistent file filter, Esc cancels unconfirmed search. Once confirmed, normal Esc clears the filter.
- State read/written: query, selected index, pane filter query/indices.
- Source refs: `src/handler.rs:184-209,266-273`, `src/app.rs:3749-3859`.
- Runtime evidence: `search-input.txt`, `search-confirmed.txt`, and `search-cleared.txt` show `/bravo` matching only `bravo.md`, Enter retaining it as a filter, and Esc restoring the full listing; see `capture-notes.md`.
- Edge cases observed: query is per active pane; `/zebra` showed an empty result area without a separate no-match message; Esc before Enter canceled and restored the full listing; search-mode Right enters folder instead of normal navigation only when search active.
- Error behavior: no explicit error; empty/no matches handled by filter/list logic.
- Config knobs: none.
- Platform notes: key decoding for `/` depends on keyboard layout/terminal.
- Depends on: fuzzy scorer, pane filtering, listing/cursor state.
- Confidence: high for matching, confirm, clear, no-match display, and cancel flow; empty-query behavior remains source-only.

### INV-INPUT-009 Copy Board focus mode
- Trigger: `` ` `` toggles Copy Board; Tab may focus it.
- Behavior: while board has focus: Esc/backtick closes/toggles; `*` opens help; `v` preview; `\\` theme; Up/Down changes selected job; `p` or Space toggles pause; `x` cancels selected job; `q` quits. Every other key is swallowed. Ctrl+C is still handled globally first.
- State read/written: board focus/selection; transfer job status/pause/cancel.
- Source refs: `src/handler.rs:211-226`, `src/app.rs:2497-2530`.
- Runtime evidence: source only.
- Edge cases observed: normal pane keys do not fall through while board-focused.
- Error behavior: job errors through transfer status/notifications.
- Config knobs: none.
- Platform notes: none specific.
- Depends on: Copy Board, transfer job queue.
- Confidence: high.

### INV-INPUT-010 Confirmation prompt mode
- Trigger: delete/copy/move action requires a confirmation.
- Behavior: `y` or Enter confirms; `n` or Esc cancels; `o` cycles overwrite policy; other keys do nothing. Ctrl+C quits because global handler precedes confirmation routing.
- State read/written: pending operation and overwrite policy.
- Source refs: `src/handler.rs:172-182`, confirmation/app methods in `src/app.rs` (search `confirm_pending`, `cancel_confirm`, `cycle_confirm_policy`).
- Runtime evidence: source only; confirmation not opened against real files.
- Edge cases observed: additional `o` cycles policy rather than executing; no key fall-through.
- Error behavior: operation-specific confirmation/error result.
- Config knobs: none.
- Platform notes: none specific.
- Depends on: transfer/delete operations, pending confirmation state.
- Confidence: high.

### INV-INPUT-011 Error, progress, info, and multi-selection dialogs
- Trigger: modal state already active.
- Behavior: error dialog consumes any key other than globally handled Ctrl+C to dismiss; deletion progress consumes any key other than Ctrl+C to hide UI while delete continues; multi-selection info consumes any key other than Ctrl+C to close while size walks continue; single-entry info uses plain `x` to cancel size walk without closing and plain `r` to restart it, while any other non-Ctrl+C key closes it. These modal checks precede confirmation/search/normal actions (after text editors and keybinding help).
- State read/written: dialog visibility, async size/deletion tasks.
- Source refs: `src/handler.rs:120-170`.
- Runtime evidence: source only.
- Edge cases observed: transient non-error status notices do not consume keys; error status does. Dialog priority can swallow keys that would otherwise run actions.
- Error behavior: error dialog first subsequent key only dismisses; it does not retry action.
- Config knobs: none.
- Platform notes: none specific.
- Depends on: status/errors, async deletion and directory-size jobs.
- Confidence: high.

### INV-INPUT-012 Preview text-edit focus
- Trigger: preview text editor has focus (`app.edit_focus`).
- Behavior: Ctrl+S saves; Esc returns to pane focus; Tab switches pane and discards edits; Ctrl+C quits; every other key is passed to textarea, including `q`, `s`, Ctrl+A (line start), and Ctrl+E (line end). Ctrl+S is case-insensitive; plain `s`/Shift+S inserts text.
- State read/written: edit buffer and file on save.
- Source refs: `src/handler.rs:19-56`, edit methods in `src/app.rs` (search `save_edit`, `close_edit`, `edit_input`).
- Runtime evidence: source only.
- Edge cases observed: editor has first key precedence; Tab changes pane rather than inserted tab; Ctrl+C is still a hard quit.
- Error behavior: save errors surface via app status.
- Config knobs: none.
- Platform notes: terminal textarea key semantics differ from native editor widgets.
- Depends on: text preview/editor, filesystem save.
- Confidence: high.

### INV-INPUT-013 Transient shortcut help and key priority
- Trigger: `*` from normal/Copy Board focus; any key while help open.
- Behavior: help documents 13 rows including actions absent from footer; any key other than globally handled Ctrl+C closes it and is consumed, so a background notification cannot steal that dismissal key. It is checked before error/info/confirmation/search action routing but after text prompts/editor and global Ctrl+C handling.
- State read/written: `keybindings_visible`.
- Source refs: `src/handler.rs:19-26,58-76,120-125,211-225,247-249`, `src/app.rs:889-897`, `src/ui/mod.rs:170-229,719-760`.
- Runtime evidence: `keybindings.txt` records `*` opening the help and `x` dismissing it without performing normal-mode bottom navigation; source test at `src/ui/mod.rs:719-760`.
- Edge cases observed: Ctrl+C bypasses help and quits due earlier priority; any other key only closes help.
- Error behavior: none.
- Config knobs: theme affects visual styling only.
- Platform notes: help is terminal-specific presentation; desktop should expose discoverable keymap/help while preserving every listed binding.
- Depends on: modal host and keymap registry.
- Confidence: high from source, existing unit test, and live PTY capture.

## Unclaimed or uncertain
- `src/services/picker_probe.rs` and `src/theme/caps.rs` are read for startup/CLI behavior only; their full protocol, theme, color, and font decision tables belong to platform/theme/config discovery.
- `src/app.rs` includes app-level feature implementations referenced above; explorer claim is input dispatch and startup callsites, not file-operation semantics.
- Exact behavior of plain unknown CLI args with a real terminal, every paste target, mouse, resize and focus behavior, all modal inputs, and macOS/Windows terminal event decoding still needs dedicated runtime captures.
- No terminal “normal/insert/visual/command” mode set exists in the observed dispatcher. Modes are transient prompt/editor/modal/focus states listed above. No count prefix or multi-key chord parser was found in `src/handler.rs`.
- Terminal-only behaviors and candidate desktop mappings: 0 spawns native terminal at active path → desktop “Open terminal here” platform service; `-` reveals selected entry in OS file browser → desktop “Show in file manager”; raw-mode/alternate-screen/mouse/paste/crossterm event lifecycle → native window focus/input, dialogs, clipboard and key bindings; Ctrl+C hard quit → desktop quit action with platform conventions. Keep key intent and shortcuts available.
