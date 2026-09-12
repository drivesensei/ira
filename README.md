# IRA — Integrated Retro Archives

A terminal file manager built with Rust, [Ratatui](https://ratatui.rs), and crossterm.

## Features

- **Drive panel** — auto-detects storage drives, mounted or not, and mounts them on demand. The list is re-scanned every ~2 s, so plugging in a drive shows it without restarting the app — like a file manager. On Linux this enumerates filesystem partitions with `lsblk` and mounts via `udisksctl` (the udisks2 backend the desktop file manager uses). See [docs/features.md](docs/features.md) for per-OS behavior.
- **Common folders** — one-key jump to Home, Desktop, Documents, Downloads, Music, Videos, Public.
- **Bookmarks** — mark any folder with `b`; each gets a single-letter shortcut auto-assigned in QWERTY keyboard order, persisted to `~/.config/ira/bookmarks`.
- **File browser** — alphabetically sorted listing, cursor navigation, opens files with the system default application.

## Install

Releases are published automatically for every version tag — see
[GitHub Releases](https://github.com/drivesensei/ira/releases) for all assets and checksums.

- **macOS** (Homebrew): `brew install drivesensei/ira/ira`
- **Windows** (winget): `winget install drivesensei.IRA`
- **Ubuntu/Debian**: download the `.deb` from a release and run
  `sudo dpkg -i ira_<version>_amd64.deb` (or `apt install ./ira_...deb`)
- **Arch Linux**: install the `ira-bin` package from the AUR
- **Anywhere with Rust**: `cargo install ira`

Linux drive-mounting support uses `udisks2` (installed automatically on most desktop
distributions; listed as a recommended dependency in the `.deb`).


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
| `Enter` | Rename the selected entry |
| `?` | Show metadata (size, type, dates) for the selected entry |
| `` ` `` | Toggle the Copy Board sidebar (progress, pause, cancel) |
| `→` | Enter the selected folder, or open the selected file |
| `←` | Go up one directory |
| `↑` / `↓` | Move selection |
| `Alt+↑` / `Alt+↓`, `z`, `x` | Jump to top / bottom of the list |
| `/`, then type | Fuzzy-search the current folder's files (`Enter` confirm, `Esc` cancel, `Backspace` delete) |
| `+` | Split / unsplit the files pane |
| `\` | Switch to the next theme (Mocha → Cyberpunk 2077 → Gruvbox Dark → Nord → Dracula → Tokyo Night) |
| `Tab` | Switch focus (panes / Copy Board) |
| `q` / `Ctrl+C` | Quit |

See [docs/features.md](docs/features.md) for a full breakdown.

## Fonts, icons & theme

IRA is a TUI: it cannot ship or select a font. The terminal you run it in owns the typeface.

- **Icons.** Three sets: **Nerd Font** glyphs (per-extension: Rust gear, Python, PDF, …), **Emoji** (📁 📄 🦀 🐍 🎬 📦 …, per-extension too, drawn by the terminal's own color-emoji font so nothing needs installing) and a **Unicode** fallback (`□` folders, `≡` text, `▶` video, …) for stock fonts. The emoji set only uses codepoints Unicode defines as two cells wide, so columns stay aligned exactly as with Nerd glyphs. Auto-detection runs on every launch: Nerd when a font that actually has the glyphs is reachable (terminals that bundle them — kitty, WezTerm, Ghostty, Warp — the profile font in Windows Terminal / VS Code settings, or on Linux and macOS any installed Nerd Font the OS can fall back to); otherwise Emoji on terminals known to render wide color emoji (Windows Terminal, every macOS terminal, VS Code, GNOME Terminal/VTE, Konsole, kitty, Alacritty, foot, …); otherwise Unicode. Pin it with `icons = "nerd" | "emoji" | "unicode"` in the theme file, or `IRA_ICONS=…` for one shot. Config lives in `~/.config/ira/` on Linux, `~/Library/Application Support/ira/` on macOS, `%APPDATA%\ira\` on Windows.
- **Colors.** Truecolor (`Color::Rgb`) is used when the terminal advertises it (`COLORTERM=truecolor`, iTerm2, WezTerm, ghostty, Windows Terminal, modern conhost). **Terminal.app is 256-color only**; IRA quantizes the palette to xterm-256 there so colors do not collapse to named ANSI.
- **Image previews.** Kitty, iTerm2 or Sixel when the terminal answers the startup probe — except Sixel on native Windows, which leaves the preview blank in our TUI, so Windows gets 2×4 braille blocks instead. Terminal.app and other protocol-less hosts use the same braille renderer (256-color on Terminal.app). `IRA_IMAGES=auto|sixel|kitty|iterm2|blocks` overrides. On macOS, real images need iTerm2, Ghostty, kitty or WezTerm.
- **Theme.** Six built-in presets — `mocha` (default, Catppuccin), `cyberpunk2077`, `gruvbox-dark`, `nord`, `dracula`, `tokyo-night`. Press `\` to cycle through them in that order; the bottom banner says `Switched to <name>` and the choice is remembered in `~/.config/ira/state`. Pick a starting preset and optional per-key overrides in `~/.config/ira/theme.toml` — partial files are fine, unknown keys and invalid colors are ignored, and overrides stay applied on every preset you cycle to:

```toml
preset = "cyberpunk2077"
icons = "nerd"

# optional per-key overrides on top of the preset
# accent = "#fcee0a"
[files]
image = "#ff9f1c"
```

- **`--check-terminal`** prints the detected truecolor flag, icon set, where the Nerd glyphs would come from (bundled, profile font, installed font, or none), active theme (and where it came from), image protocol (and why it was chosen), cell size, and whether a theme file was found. Use it when icons render as boxes or colors look washed out.

Full list of themes with swatches, recommended Nerd Fonts, icon sets, and the exact precedence rules: [docs/themes.md](docs/themes.md).

## Project structure

```
src/
├── main.rs            # entry point + event loop
├── app.rs             # application state + navigation logic
├── event.rs           # terminal event handler (tick/key/mouse/resize)
├── handler.rs         # key → action dispatch
├── tui.rs             # terminal setup/teardown (raw mode, alt screen)
├── ui/                # layout composition + shared chrome (chips, glass dialogs)
├── theme/             # palette, terminal caps, file-type icons
├── domain/            # core types (Folder)
├── services/          # drives, folders, bookmarks, file listing
├── components/        # per-panel widgets
└── utils/             # path helpers
```

## Known limitations

- Linux drive detection uses `lsblk` (util-linux) and mounting uses `udisksctl` (udisks2); both must be installed. Filesystems already mounted outside the removable-media roots (`/run/media`, `/media`, `/mnt`) are treated as system mounts and not listed.
- The second file-panel tab (`tab2`) is declared in the model but not implemented.
- Mouse input is captured but not handled.
