# Inventory: platform helpers, common folders, and terminal preview overlays
Explorer run: 2026-10-01 | TUI commit: `1cad4ce43cc72d52d4cc4eef920e0da22cb69568` | Files claimed: `src/lib.rs`, `src/components/common_folders_ui.rs`, `src/services/folders.rs`, `src/services/overlay/{mod.rs,macos.rs,windows.rs}`, `src/services/windows_drives_labels.rs`, `src/theme/{font_probe.rs,wt_font.rs}`

## Features

### INV-PLAT-001 Public library module surface
- Trigger: Rust consumer imports the root `ira` library.
- Behavior: `lib.rs` publicly exposes `app`, `event`, `ui`, `tui`, `handler`, `services`, `components`, `domain`, `theme`, and `utils`; this is the TUI crate's module API boundary, not an additional end-user screen.
- State read/written: none.
- Source refs: `src/lib.rs:1-28`.
- Runtime evidence: `migration/oracle/captures/platform-helpers/linux-common-folders.txt` confirms the built CLI entry renders its TUI. No downstream crate API compatibility probe was run.
- Edge cases observed: all modules are public, so migration to a core crate must preserve or intentionally version this Rust API.
- Error behavior: compile-time module visibility only.
- Config knobs: none.
- Platform notes: `ui`, `tui`, `event`, and `handler` expose terminal-specific types; do not infer these are already GPUI-independent.
- Depends on: none.
- Confidence: medium (source is complete; no external library consumer fixture).

### INV-PLAT-002 Common-folder path discovery
- Trigger: app startup calls `list_common_folders()`.
- Behavior: returns available `dirs-next` home, desktop, documents, downloads, audio/music, video, and public directories in that order. Each is included only when the OS helper returns a path and that path converts to UTF-8 via `to_str`; paths are not checked for existence. Labels and shortcuts are respectively Home/w, Desktop/e, Documents/r, Downloads/t, Music/y, Videos/u, Public/i.
- State read/written: OS-known home and user-directory conventions; no file writes.
- Source refs: `src/services/folders.rs:1-54`; `src/app.rs:626`.
- Runtime evidence: `migration/oracle/captures/platform-helpers/linux-common-folders.txt` (fresh HOME, 120x40; Home appeared; other folders absent for this temporary profile); `migration/oracle/captures/platform-helpers/pty-run.txt` (start and `q` exit).
- Edge cases observed: non-UTF-8 paths silently disappear; returned folder paths can be nonexistent; folders returning `None` are omitted and therefore their key is not reserved.
- Error behavior: no errors are surfaced; helper failures/absence yield omissions.
- Config knobs: OS user-directory configuration (e.g. XDG user dirs), HOME; no IRA config key.
- Platform notes: directory resolution is delegated to `dirs-next`; precedence and results need per-OS evidence for macOS/Windows. The TUI stores paths as `String`, unlike the migration path invariant.
- Depends on: INV-PLAT-003.
- Confidence: high on code behavior, low on cross-platform runtime outcomes.

### INV-PLAT-003 Common-folder panel and key navigation
- Trigger: startup UI; press the shortcut shown beside a listed common folder.
- Behavior: component renders each available shortcut, folder-type icon, and label inside the focused “Common folders” panel; if `app.folders` is `None`, displays exact text `No common folders found`. In normal mode, a matching non-digit character selects that folder (before bookmark shortcut lookup), clears the active pane's filter, clears filter indices, sets the folder, clears global search, reloads listing, then clears cursor selection.
- State read/written: active pane folder/filter/cursor and global search.
- Source refs: `src/components/common_folders_ui.rs:13-55`; `src/handler.rs:330-337`; `src/app.rs:2073-2078,2175-2192`; `src/theme/icons.rs:121`.
- Runtime evidence: `migration/oracle/captures/platform-helpers/linux-common-folders.txt` records the 120x40 startup screen and the `w` interaction that selected Home and loaded the home directory; app then exited with `q` (0).
- Edge cases observed: common shortcut matching occurs before bookmarks for a non-digit key. If the same key is assigned to a common folder, bookmark selection cannot receive it. Digits are handled by the drive path earlier and are excluded by this branch.
- Error behavior: an invalid shortcut has no effect; folder lookup missing at index returns without state change.
- Config knobs: folder path set by OS helper; shortcut assignment is currently fixed in `folders.rs`.
- Platform notes: keyboard shortcuts must remain reachable in the GPUI keymap. Icon rendering depends on terminal capability/font probing (INV-PLAT-006).
- Depends on: INV-PLAT-002, INV-PLAT-006.
- Confidence: medium-high; live PTY verified on Linux only.

### INV-PLAT-004 Terminal image overlay geometry and composition
- Trigger: app has a ready image-preview overlay job on a terminal without a usable inline graphics protocol.
- Behavior: pure helpers map terminal cell rectangles into top-left-origin global pixels, account for window chrome, snap/clamp measured cell sizes, fit image tiles preserving aspect ratio, composite a two-pane maximum of tiles, fingerprint placement/content, and cap a surface side at 4096 px. Overlay geometry tracker polls at 300 ms while needed, idles at 1500 ms otherwise, and treats samples older than 2 s as stale/hidden. Image tiles use 80% grid fill; a pane exceeding surface cap keeps its braille underlay and hides native surface.
- State read/written: process-local cached geometry, content fingerprints, visibility and fitted thumbnails.
- Source refs: `src/services/overlay/mod.rs:22-47,87-117,122-179,187-312,337-404,515-628,630-857`; caller behavior `src/app.rs:1531-1598,1644-1680`.
- Runtime evidence: `migration/oracle/captures/platform-helpers/overlay-linux-tests.txt` records targeted existing pure overlay test run; native overlay is compiled out on Linux. Test functions in `src/services/overlay/mod.rs:900-1534` cover cell geometry, caps, hashes, tracker transitions, cache and lifecycle.
- Edge cases observed: zero dimensions hide; oversized bitmap side clamps; stale geometry hides; tracker Pending is distinct from Hidden; at most two overlay slots. Source-side tests do not establish OS window behavior.
- Error behavior: geometry absence/errors yield no visible overlay; caller retains terminal braille preview. No user-facing native error is emitted.
- Config knobs: none user-facing; platform/terminal geometry and image data determine sizing.
- Platform notes: macOS uses AppleScript window geometry and point coordinates; Windows uses HWND/client geometry and console/font metrics; Linux native overlay is no-op. The GPUI desktop should not inherit terminal window-overlap machinery.
- Depends on: preview inventory, terminal dimensions, thumbnail cache.
- Confidence: high for pure math/source; low for OS native behavior pending macOS/Windows runtime evidence.

### INV-PLAT-005 macOS native preview overlay
- Trigger: preview jobs request an overlay while a recognized terminal is frontmost.
- Behavior: creates a non-activating, borderless floating AppKit panel, ignores mouse, hides on deactivation, paints PNG-backed image views, and drains at most eight AppKit events with a nonblocking distant-past deadline when `pump()` is called. Coordinates are converted from top-left screen coordinates to Cocoa bottom-left using the main screen height. Geometry query uses AppleScript for Terminal.app, iTerm2, Ghostty or a whitelist of known terminal front apps; it correlates stdin TTY where possible, uses front-window fallback when tty correlation returns `NOTFOUND`, and returns hidden when another app is frontmost.
- State read/written: transient panel/image view and image fingerprint cache; AppleScript and tty introspection.
- Source refs: `src/services/overlay/macos.rs:18-95,180-214,219-390`; shared caller `src/services/overlay/mod.rs:823-856`.
- Runtime evidence: no macOS host available. Source-backed platform map only; `migration/oracle/captures/platform-helpers/platform-runtime-limitations.txt` records limitation.
- Edge cases observed: missing main-screen data falls back to 1080 points; inability to obtain a main-thread marker means no overlay; unsupported front process or another frontmost app hides it; AppleScript failures produce no geometry.
- Error behavior: failures are treated as no overlay, no terminal UI error.
- Config knobs: `TERM_PROGRAM`; stdin tty; Accessibility/Automation permission behavior is not verified.
- Platform notes: window frame and Retina coordinate units are an identified risk; AppleScript permission prompts and terminal process names require actual macOS testing.
- Depends on: INV-PLAT-004.
- Confidence: medium on source, low on runtime.

### INV-PLAT-006 Windows native preview overlay and font-cell metrics
- Trigger: preview overlay job in Windows Terminal or classic console.
- Behavior: creates a layered, transparent, no-activate, tool popup owned by the foreground terminal; validates/recreates stale HWNDs; only paints while the terminal host is foreground; uses client rect converted to screen coords. It prefers actual console font cell metrics only when they fit; otherwise estimates from Windows Terminal settings (`font.size`, `lineHeight`/`cellHeight`) and host DPI, then falls back to window dimensions. It clamps and snaps cell sizes to avoid stale/fake ConPTY metrics and excessive chrome.
- State read/written: transient popup HWND, foreground window/console handles and output console font state.
- Source refs: `src/services/overlay/windows.rs:36-180,185-286,287-350,351-508`; `src/services/overlay/mod.rs:857-881`.
- Runtime evidence: Windows APIs are cfg-gated and unavailable on this Linux host. Unit tests exercise shared metric math only; see `platform-runtime-limitations.txt`.
- Edge cases observed: hidden/stale console HWNDs, reused handle values, ConPTY ownership and fake `{0,16}` font metrics are explicitly handled; when host cannot be correlated it refuses to paint.
- Error behavior: Win32 call failures yield `None`/`false`, which hides overlay without an app-level error.
- Config knobs: environment `WT_PROFILE_ID`, Windows Terminal settings font fields and `IRA_WT_SETTINGS`.
- Platform notes: DPI-awareness and foreground/focus transitions require Windows runtime evidence. This terminal overlay is not a suitable desktop-app window implementation.
- Depends on: INV-PLAT-004, INV-PLAT-008.
- Confidence: medium on source; low on Windows runtime.

### INV-PLAT-007 Windows volume label lookup
- Trigger: drive listing asks Windows for a volume label.
- Behavior: calls `GetVolumeInformationW` with UTF-16 drive string and 256 WCHAR label buffer; failure returns `last_os_error`; successful UTF-16 is converted lossily to String and all non-ASCII-graphic characters are stripped. The function exists only on Windows.
- State read/written: OS volume metadata; no mutation.
- Source refs: `src/services/windows_drives_labels.rs:1-39`; consumer `src/services/drives.rs:34-54`.
- Runtime evidence: no Windows runtime available; platform-only source check recorded in `platform-runtime-limitations.txt`.
- Edge cases observed: conversion is lossy and filtering drops spaces and Unicode, not merely controls; missing/unready drives can make API fail.
- Error behavior: boxed `std::io::Error` from Win32 last-error; consumer/fallback behavior belongs to drive inventory.
- Config knobs: none.
- Platform notes: volume labels can be Unicode on Windows and should not be round-tripped through lossy ASCII filtering in a desktop migration unless parity decisions approve it.
- Depends on: filesystem/drives inventory.
- Confidence: high on source; low on runtime.

### INV-PLAT-008 Nerd-font capability detection
- Trigger: theme/icon capability initialization calls `font_probe::probe`.
- Behavior: tests three glyph codepoints (folder, Devicons NodeJS, Powerline pill) and returns source classification. Priority: terminals that bundle Nerd Symbols (kitty by TERM/kitty id, WezTerm/Ghostty/Warp by terminal program or TERM) are accepted; Windows Terminal reads active profile face (default Cascadia Mono if config unreadable), VS Code-family terminal/editor profile is checked, and installed font fallback is used only off Windows. A face is accepted by Nerd/NF/NFM/NFP/Caskaydia/Delugia naming heuristic; PL alone is rejected. On Linux a successful fontconfig charset query is authoritative; otherwise or on macOS scan user/system font dirs to max depth 4 for .ttf/.otf/.ttc files with matching names.
- State read/written: terminal environment snapshot, selected editor/terminal configuration, installed font tree and fontconfig process output.
- Source refs: `src/theme/font_probe.rs:1-286`; capability consumer `src/theme/caps.rs:1-108`.
- Runtime evidence: isolated Linux PTY initial screen rendered Nerd-style glyph icons in drive/common-folder panels; source probe-specific branches are validated by unit tests in `src/theme/font_probe.rs:287-end`, but active machine font provenance was not exposed by UI. Capture `linux-common-folders.txt`.
- Edge cases observed: unsupported/missing probe returns `NotFound` and theme falls back; Windows does not scan installed fonts because DirectWrite PUA fallback differs; depth cap omits deeper fonts; scan order follows supplied dirs and read_dir order.
- Error behavior: command/filesystem failures generally degrade to `NotFound` rather than visible errors.
- Config knobs: environment-derived `TERM`, `TERM_PROGRAM`, kitty window ID, WT session/profile and VS Code settings; see caps inventory for snapshot fields.
- Platform notes: desktop renderer uses its own font stack; this TUI capability inference should not constrain GPUI fonts/icons.
- Depends on: INV-PLAT-009.
- Confidence: high on code/unit behavior; medium on runtime glyph coverage.

### INV-PLAT-009 Windows Terminal and VS Code JSONC font settings
- Trigger: Windows Terminal/VS Code host capability probe; Windows Terminal font size/line-height also inform overlay sizing.
- Behavior: Windows Terminal candidates are searched in this order: nonblank `IRA_WT_SETTINGS`, each `%LOCALAPPDATA%`-style root's Store, Preview, unpackaged settings paths, then portable `<exe-dir>/settings/settings.json` only if `.portable` exists. WSL additionally walks `/mnt/c/Users/*/AppData/Local`. Files over 4 MiB, unreadable or non-UTF-8 are skipped. JSONC comments, trailing commas, and leading BOM are stripped; malformed settings are ignored. Active profile id match is case-insensitive; its font wins, then profile defaults, with legacy keys/array profile layout supported. Cache keys profile id and file path/mtime/length. VS Code-family config candidates (Code, Insiders, Cursor, VSCodium) are checked at `config_dir()/.../User/settings.json`, reading terminal font family then editor font family.
- State read/written: reads external terminal/editor JSON files; process-local cache, no writes.
- Source refs: `src/theme/wt_font.rs:26-438`; overlay consumer `src/services/overlay/windows.rs:331-350`; probe consumer `src/theme/font_probe.rs:75-123,250-285`.
- Runtime evidence: existing unit tests in `src/theme/wt_font.rs:614-917` cover JSONC/BOM, precedence, aliases, candidate paths, size limits, cache invalidation/profile ID. Real Windows profile file was not available; see `platform-runtime-limitations.txt`.
- Edge cases observed: files >4 MiB skipped; non-UTF-8 silently skipped; changed bytes with same mtime+length hit cache until another fingerprint field changes; missing or malformed settings return `None`; portable directory is gated by marker.
- Error behavior: silently falls through candidates or defaults; no user-visible error.
- Config knobs: `IRA_WT_SETTINGS`, `LOCALAPPDATA`, `WT_PROFILE_ID`; VS Code profile filesystem locations.
- Platform notes: settings paths and WSL assumptions are Windows-specific. Env override has precedence and may point outside normal profile. Do not import terminal-config parsing into a native GPUI app unless required for TUI parity.
- Depends on: INV-PLAT-006, INV-PLAT-008.
- Confidence: high on parsing/candidate unit behavior; low on actual installed terminal path variants.

## Unclaimed or uncertain
- Native overlay behavior is intentionally source-only on this Linux host. Mac and Windows screenshots/runtime probes are still necessary before any visual parity claim.
- The source tree also has path existence/ordering behavior delegated to `dirs-next`; actual results depend on per-user directory settings.
- Public `lib.rs` re-exports are not a user-visible TUI surface; preserve only if the crate API is in scope.
