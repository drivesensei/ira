# IRA — Integrated Retro Archives

A terminal file manager built with Rust, [Ratatui](https://ratatui.rs), and crossterm.

## Features

- **Drive panel** — auto-detects storage drives, mounted or not, and mounts them on demand. The list is re-scanned every ~2 s, so plugging in a drive shows it without restarting the app — like a file manager. On Linux this enumerates filesystem partitions with `lsblk` and mounts via `udisksctl` (the udisks2 backend the desktop file manager uses). See [docs/features.md](docs/features.md) for per-OS behavior.
- **Common folders** — one-key jump to Home, Desktop, Documents, Downloads, Music, Videos, Public.
- **Bookmarks** — mark any folder with `b`; each gets a single-letter shortcut auto-assigned in QWERTY keyboard order, persisted to `~/.config/ira/bookmarks`.
- **File browser** — alphabetically sorted listing, cursor navigation, opens files with the system default application.

## Build

Requires Rust (tested with 1.98).

```sh
cargo build --release
./target/release/ira
```

## Usage

| Key | Action |
| --- | --- |
| `1`–`9` | Open the Nth drive (mounts it first if unmounted) |
| `w` `e` `r` `t` `y` `u` `i` | Jump to Home / Desktop / Documents / Downloads / Music / Videos / Public |
| `b` | Bookmark / unbookmark the current folder |
| `c` | Copy the selected entry(ies) to the other pane |
| `m` | Move the selected entry(ies) to the other pane |
| `Space` | Multi-select the entry under the cursor |
| `Ctrl+A` (or `Alt+A`) | Select all / clear all |
| `Alt+I` | Invert the selection |
| `.` | Toggle hidden (dot) files |
| `Del` | Delete the selection (with confirmation) |
| `` ` `` | Toggle the Copy Board sidebar (progress, pause, cancel) |
| `→` | Enter the selected folder, or open the selected file |
| `←` | Go up one directory |
| `↑` / `↓` | Move selection |
| `Alt+↑` / `Alt+↓`, `z`, `x` | Jump to top / bottom of the list |
| `/`, then type | Fuzzy-search the current folder's files (`Enter` confirm, `Esc` cancel, `Backspace` delete) |
| `+` | Split / unsplit the files pane |
| `Tab` | Switch focus (panes / Copy Board) |
| `q` / `Ctrl+C` | Quit |

See [docs/features.md](docs/features.md) for a full breakdown.

## Project structure

```
src/
├── main.rs            # entry point + event loop
├── app.rs             # application state + navigation logic
├── event.rs           # terminal event handler (tick/key/mouse/resize)
├── handler.rs         # key → action dispatch
├── tui.rs             # terminal setup/teardown (raw mode, alt screen)
├── ui.rs              # layout composition
├── domain/            # core types (Folder)
├── services/          # drives, folders, bookmarks, file listing
├── components/        # per-panel widgets
└── utils/             # path helpers
```

## Known limitations

- Linux drive detection uses `lsblk` (util-linux) and mounting uses `udisksctl` (udisks2); both must be installed. Filesystems already mounted outside the removable-media roots (`/run/media`, `/media`, `/mnt`) are treated as system mounts and not listed.
- The second file-panel tab (`tab2`) is declared in the model but not implemented.
- Mouse input is captured but not handled.
