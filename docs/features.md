# IRA Features

## Overview

IRA is a single-window terminal file manager. The screen is split into four stacked panels:

1. **Drives** — storage devices, mounted or not; selecting an unmounted one mounts it.
2. **Common folders** — frequently used directories with single-key shortcuts.
3. **Bookmarks** — quick links.
4. **Files** — the current folder's contents (the only interactive list).

The application holds all state in a single `App` struct (`src/app.rs`). Rendering is split per panel under `src/components/`, and input is dispatched from `src/handler.rs`.

## Panels

### Drives

Drive discovery is platform-specific (`src/services/drives.rs`):

| OS | Source | Notes |
| --- | --- | --- |
| Linux | `lsblk` | Enumerates block devices via `lsblk -P` and lists filesystem partitions (mounted **and** unmounted) of type `btrfs`, `exfat`, `ext2/3/4`, `f2fs`, `fuseblk`, `hfs/hfsplus`, `iso9660`, `jfs`, `ntfs`, `ntfs3`, `reiserfs`, `udf`, `ufs`, `vfat`, `xfs`, `zfs`. Excludes virtual devices, swap/LUKS/LVM, system partition types (EFI System, Microsoft Reserved, Windows Recovery), and filesystems already mounted at system paths (`/`, `/home`, `/boot`, …). |
| macOS | `/Volumes` | Lists every entry under `/Volumes`. |
| Windows | `A:\` … `Z:\` | Probes each letter via `std::fs::metadata` and reads the volume label with `GetVolumeInformationW`. |

Each drive gets a positional shortcut `1`…`9`. The list is re-scanned on every tick (~2 s), so newly connected or removed drives appear automatically without a restart — the same observable behavior as a file manager. The polling approach is portable: the platform `list_drives()` implementations (lsblk / `/Volumes` / drive-letter probing) are all cheap enough to call on a timer, avoiding udev/`WM_DEVICECHANGE`/disk-arbitration plumbing.

The panel displays `[n] 🖥️ <label>` where `<label>` is the filesystem label (falling back to the device name when unlabelled). Selecting an unmounted drive mounts it with `udisksctl mount -b <device>` — the same udisks2 backend the desktop file manager uses — then opens it. An unmounted drive has an empty `path` until mounted.

### Common folders

Resolved via the `dirs-next` crate (`src/services/folders.rs`) from the user's standard directories:

| Key | Folder |
| --- | --- |
| `w` | Home |
| `e` | Desktop |
| `r` | Documents |
| `t` | Downloads |
| `y` | Music |
| `u` | Videos |
| `i` | Public |

Folders that don't exist on the platform are skipped automatically.

### Bookmarks

Bookmarks give one-key access to folders. Press `b` while viewing a folder to bookmark it (press `b` again to remove it). Each bookmark gets a single-letter shortcut auto-assigned in QWERTY keyboard order, skipping letters already used by the common folders (`w e r t y u i`) and reserved keys (`q` quit, `c` Ctrl-C, `z`/`x` top/bottom, `b` toggle, `v` image preview).

The letters are assigned in this order: `o`, `p`, `a`, `s`, `d`, `f`, `g`, `h`, `j`, `k`, `l`, `n`, `m`.

Bookmarks persist to `~/.config/ira/bookmarks` (one `label<TAB>path` line each).

### Files (tab 1)

Shows the contents of the currently selected folder (`src/services/list_files.rs`), sorted alphabetically. The cursor highlight is reverse-video with a `→` marker. Selecting a folder and pressing `→` descends into it; selecting a file and pressing `→` opens it with the system default application (via the `open` crate).

## Navigation

| Key | Action |
| --- | --- |
| `1`–`9` | Open the Nth drive (mounts it first if unmounted) |
| `w e r t y u i` | Jump to the matching common folder |
| `b` | Bookmark / unbookmark the current folder |
| (assigned letter) | Jump to that bookmark |
| `c` | Copy the selected entry(ies) to the other pane |
| `m` | Move the selected entry(ies) to the other pane |
| `Space` | Multi-select the entry under the cursor |
| `Ctrl+A` (or `Alt+A`) | Select all / clear all |
| `Alt+I` | Invert the selection |
| `.` | Toggle hidden (dot) files |
| `Del` | Delete the selection (with confirmation) |
| `Enter` | Rename the selected entry |
| `?` | Show metadata (size, type, dates) for the selected entry |
| `` ` `` | Toggle the Copy Board sidebar (progress, pause, cancel) |
| `→` | Enter selected folder / open selected file |
| `←` | Go up one directory |
| `↑` / `↓` | Move selection |
| `Alt+↑` / `Alt+↓` | Jump to top / bottom of the list |
| `z` / `x` | Jump to top / bottom of the list |
| `/` | Start fuzzy search of the current folder's files |
| `v` | Cycle the active pane's image preview: off → column → grid (per pane, persisted) |
| `+` | Split / unsplit the files pane (side by side) |
| `Tab` | Switch focus (panes / Copy Board) |
| `q` / `Ctrl+C` | Quit |

While searching, typed characters filter the file list by fuzzy subsequence match (case-insensitive, scoring consecutive and word-boundary matches). `Backspace` deletes a character, `Enter` confirms (jumps to the selected match), `Esc` cancels, `↑`/`↓` move within the matches, and `→` enters the selected match. The search is scoped to the files already listed in the current folder and resets whenever the folder changes.

**Copy / move (async):** `c` copies the selected entry(ies) to the other pane's folder, `m` moves them (rename when possible, otherwise copy + delete). `Space` multi-selects entries (marked `[*]`), so several can be copied/moved at once; with nothing selected, `c`/`m` act on the cursor entry. Copies run in background threads and never block the UI; progress and all active jobs are shown in the Copy Board (`` ` ``). With the board focused (`Tab` cycles focus), `↑`/`↓` select a job, `Space`/`p` pause/resume, `x` cancels (removes the partial destination), and `Esc` closes the board.

**Delete:** `Del` deletes the selection (or the cursor entry when nothing is selected) after a confirmation prompt — `y`/`Enter` confirms, `n`/`Esc` cancels. Directories are removed recursively.

**Rename:** `Enter` opens an in-place rename dialog with a visible cursor; type to edit, `←`/`→` move the cursor, `Backspace` deletes, `Enter` applies, `Esc` cancels. Rejects empty names and name collisions (won't overwrite).

**Info:** `?` opens a metadata dialog showing name, full path, kind (folder, or a file type derived from the extension), hidden status, size (recursive for folders), and added/modified dates (UTC). Any key dismisses it.

**Selection & visibility:** `Space` multi-selects (rows show `[*]`/`[ ]`); `Ctrl+A` selects or clears everything, `Alt+I` inverts the selection. (Super-based binds are not reliable in terminals, so `Ctrl`/`Alt` are used instead.) `.` toggles whether hidden (dot) files are listed. Rows carry a folder glyph (`□`) or file glyph (`·`), both single-width Unicode that render identically on Linux/macOS/Windows terminal fonts.

**Edit text in the preview:** with a text file previewed in column mode, `Tab` focuses the editor — type to edit, `Ctrl+S` saves atomically, `Esc` exits — plain letters type. `q` types a `q`; nothing quits while editing. Read-only, binary, non-UTF-8, and >5 MB files open read-only instead. Unsaved changes are discarded on `Esc`/`Tab` (shown by a `*` in the border). **Image preview:** `v` cycles the ACTIVE pane's preview mode — modes are per pane, survive restarts, and switching panes with Tab never changes either pane's mode. **Column** renders the selected file as an image in a side column. **Grid** replaces the file list with a thumbnail grid (image thumbnails, glyphs for folders and unsupported files, cursor highlighted; all formats the list shows are shown, including filtered views). Rendering picks the best protocol the terminal supports at startup — kitty, iTerm2, or Sixel for true graphics — and falls back to Unicode half-blocks everywhere else. Supported formats: PNG, JPEG, GIF, BMP, WebP — plus MP4/MOV videos and HEIC photos when `ffmpeg` is on `PATH`, and PDF first pages when `pdftoppm` (poppler) is on `PATH` (both optional runtime dependencies; without them those files show glyphs). **Text and code files** (`txt md json toml yaml csv rs py sh c html`…) render natively in the column — head of the file, NUL-sniffed so binaries never render as garbage, capped at 256 KB with a truncation notice. Thumbnails for the screens adjacent to the viewport are prefetched in the background, so scrolling large folders feels instant. Decoding runs on a bounded background worker pool and never blocks the UI; thumbnails are cached in memory and on disk (`~/.cache/ira/thumbnails`, keyed by path + mtime + size, so edited images refresh automatically).
  On iTerm2 — whose kitty implementation does not render kitty's unicode placeholders and
  would show "?" glyphs — kitty is skipped at detection and the iTerm2 protocol is used
  instead. Terminal.app supports no graphics protocol and previews render as half-blocks.

## Domain model

The core type is `Folder` (`src/domain/data.rs`):

```rust
pub struct Folder {
    pub label: String,
    pub path: String,
    pub shortcut: char,
    pub device: Option<String>, // e.g. "/dev/sdb1" for drives; None otherwise
}
```

For a drive, `device` holds the block-device path and `path` holds the mount point (empty until mounted); `device` is `None` for ordinary folders and bookmarks.

Drives, common folders, and bookmarks all produce `Folder` values, which feed the panels and the shortcut handlers.

## Event loop

`src/event.rs` spawns a background thread that polls crossterm and emits `Tick`, `Key`, `Mouse`, and `Resize` events over an mpsc channel. The main loop (`src/main.rs`) draws the UI, then blocks on the next event. The tick rate is 500 ms so async results (drive list, restored pane listings) appear promptly. `Tick` and `Mouse` events are handled by `App::tick` (drains jobs, drive cache, and pending file listings).

## Terminal lifecycle

`src/tui.rs` enables raw mode, switches to the alternate screen, hides the cursor, and installs a panic hook that restores the terminal before propagating the panic — so a crash does not leave the terminal in raw mode.

## Implemented vs. planned

| Capability | Status |
| --- | --- |
| Drive detection — mounted + unmounted (Linux/macOS/Windows) | Implemented |
| Mount-on-select (Linux, via udisks2) | Implemented |
| Common-folder shortcuts | Implemented |
| File browsing + open with default app | Implemented |
| Minimum terminal-size guard (90×15) | Implemented |
| Bookmarks (add/remove, auto-assigned shortcuts, persisted) | Implemented |
| Second file panel (`tab2`) | Declared, not rendered |
| Mouse interaction | Captured, not handled |
| Incremental / deduplicated backups | Not implemented (README aspiration only) |

## Known limitations

- **Linux drives:** detection uses `lsblk` (util-linux) and mounting uses `udisksctl` (udisks2); both must be installed. Mounting internal (non-removable) partitions may prompt for polkit authorization. Filesystems already mounted outside `/run/media`/`/media`/`/mnt` are treated as system mounts and hidden.
- **Single panel:** only `tab1` is functional.
- **Mouse:** captured but ignored.
