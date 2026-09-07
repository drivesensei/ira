# ira — Feature Overview

ira is a keyboard-driven terminal file manager that puts two folders on screen at once and moves
files between them without leaving the keyboard. One static binary, no runtime dependencies, the
same experience on Linux, macOS, and Windows.

## Dual-Pane Workspace

| Key | Action |
| --- | --- |
| `+` | Split the file area into two side-by-side panes (press again to unsplit) |
| `Tab` | Cycle focus: left pane → right pane → Copy Board |

Both panes browse, select, and navigate independently. Transfers flow between them: what you see
on the left lands on the right, and both listings stay in sync when a folder is shown twice.

## Drives at a Glance

The drive bar lists every storage device — mounted or not — with a positional shortcut:

| Key | Action |
| --- | --- |
| `1`–`9` | Open the Nth drive (mounts it first if unmounted) |
| `-` | Eject (unmount) the removable drive of the active pane |

The list is re-scanned every tick, so USB sticks and external disks appear and disappear on their
own — no restart, no manual refresh. Mounting uses the same udisks2 backend as your desktop file
manager. Errors surface as dismissable dialogs instead of failing silently.

## Instant Navigation

| Key | Action |
| --- | --- |
| `w e r t y u i` | Home, Desktop, Documents, Downloads, Music, Videos, Public |
| `b` | Bookmark the current folder (press again to remove) |
| (assigned letter) | Jump to that bookmark |
| `←` | Up one folder — the cursor lands on the folder you came from |

Bookmarks get single-letter shortcuts assigned automatically and persist across restarts in
`~/.config/ira/bookmarks`. Going up a folder is not a leap into the dark: the entry you descended
from is right under the cursor.

## Fuzzy Search with a Sticky Filter

| Key | Action |
| --- | --- |
| `/` | Fuzzy-search the current folder's listing |
| `Enter` | Confirm the query — the pane keeps showing the matches |
| `Esc` | Clear the filter and restore the full listing |

While typing, the list narrows live with case-insensitive subsequence matching (consecutive and
word-boundary hits score best). Confirming turns the search into a sticky filter view. Every
action works on the filtered results: navigate, select, copy, move, delete, rename — as if the
filtered view were the folder. `Esc` brings the full listing back at any time.

## Multi-Select and Batch Operations

| Key | Action |
| --- | --- |
| `Space` | Toggle selection on the entry under the cursor |
| `Ctrl+A` (or `Alt+A`) | Select all / clear all |
| `Alt+I` | Invert the selection |
| `c` | Copy the selection to the other pane |
| `m` | Move the selection to the other pane |
| `Del` | Delete the selection (with confirmation) |

Selections are marked inline, and every operation accepts a run of entries or a single cursor
entry. Nothing happens without asking: deletes, and folder-into-itself transfers, are guarded by
an explicit confirmation.

## Background Transfers with the Copy Board

Copies and moves run on background threads — the UI never freezes, no matter how large the tree.

| Key (Copy Board focused) | Action |
| --- | --- |
| `` ` `` | Toggle the Copy Board sidebar |
| `↑` / `↓` | Select a job |
| `Space` / `p` | Pause / resume the selected job |
| `x` | Cancel the job (removes the partial destination) |
| `Esc` | Close the board |

Each job shows live progress. Cancelled transfers clean up after themselves; finished folders
arrive complete.

## Folder Sizes, Measured in the Background

| Key | Action |
| --- | --- |
| `?` | Measure the selected folder, or the whole selection |

Single folders stream their totals into the info dialog while the walk runs in the background —
`x` stops the walk, `r` restarts it. With a multi-selection, ira measures everything at once and
reports **"N folders / M files selected"** with the summed data size and on-disk size. Completed
measurements are cached and persisted, so reopening a folder shows its size instantly — even
after a restart.

## Image Preview

| Key | Action |
| --- | --- |
| `v` | Cycle the active pane's image preview: off → column → grid (per pane, persisted) |
| `Tab` | Focus the text editor when a text file is previewed |
| `Ctrl+S` | Save (editor focused); plain letters type |
| `Esc` | Exit the editor, back to the pane |

**Edit text in the preview:** with a text file previewed in column mode, `Tab` focuses the
editor — type to edit, `Ctrl+S` saves atomically, `Esc` exits — plain letters type. `q` types a `q`; nothing
quits while editing. Read-only, binary, non-UTF-8, and >5 MB files open read-only instead, and
unsaved changes are discarded on `Esc`/`Tab` (a `*` in the border marks them).

**Column** renders the selected entry as an image in a side column. **Grid** replaces the file
list with a thumbnail grid — image thumbnails, glyphs for folders and unsupported formats, the
cursor highlighted, filtered (`/`) views included. Select an image (PNG, JPEG, GIF, BMP, WebP)
and ira renders it with full graphics in terminals that support the kitty, iTerm2, or Sixel
protocols, Unicode half-blocks everywhere else. Decoding happens on a bounded background worker
pool and is cached in memory and on disk, so scrolling through a folder of photos stays instant
and the UI never blocks.

## Create, Go To, and Clipboard

| Key | Action |
| --- | --- |
| `n` | Create a file or folder — the extension decides: `notes.txt` is a file, `notes` a folder |
| `[` | Open the go-to-path dialog |
| `]` | Copy the active pane's folder path to the clipboard |

Creating is not limited to the current level: `sub/notes.txt` creates missing parent folders
automatically and drops you next to what you made. The go-to dialog accepts pasted or typed
paths, expands `~`, and navigates to the target — creating the whole chain when it does not
exist yet, for files and folders alike. `]` hands the current folder path to the clipboard for
pasting anywhere else.

## Everyday Editing

| Key | Action |
| --- | --- |
| `Enter` | Rename the selected entry in place |
| `Del` | Delete the selection after a `y`/`n` confirmation (on macOS, Backspace joins — Finder-style) |
| `→` | Enter the selected folder, or open the selected file with the system default app |

Renames reject empty names and collisions — never an accidental overwrite. Deletion is
recursive for folders, always confirmed, and runs in the background with a dismissable progress
dialog.

## Comfort

- **Native terminal on demand** — `0` spawns your own terminal emulator directly in the
  active pane's folder, preferring the one ira runs in. Linux tries foot, Alacritty, kitty,
  GNOME Terminal, Konsole, xfce4-terminal, and xterm; macOS launches Terminal.app, iTerm2,
  WezTerm, Ghostty, kitty, or Alacritty (via `open`, since GUI apps aren't on `PATH`);
  Windows opens Windows Terminal, falling back to a `cmd` window.
- **Hidden files** — `.` toggles dot-file visibility; the setting persists across restarts.
- **Sortable listings** — `,` cycles the listing through name, size, modified, and kind.
- **Scroll acceleration** — hold `↑`/`↓` and the cursor ramps up through long listings; change
  direction and the ramp resets.
- **Jump to top / bottom** — `z` / `x`, or `Alt+↑` / `Alt+↓`.

## Clear Feedback

Transient notices appear in a bottom bar and expire on their own; failures open dismissable
error dialogs instead of vanishing. Deletion and size walks show progress while they run, and
every dialog is closed by the same key you reached for anyway.

## Session Persistence

Layout, active pane, both folders, the hidden-files setting, and the folder-size cache are
persisted and restored — quit anywhere, come back to everything where you left it.

## Cross-Platform by Construction

Linux, macOS, and Windows from a single static binary with no runtime dependencies. Drive
detection adapts per platform (lsblk, `/Volumes`, drive-letter probing), and every glyph in the
UI is single-width Unicode that renders identically across terminal fonts.
