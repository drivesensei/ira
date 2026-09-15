# Themes, fonts & icons

Everything IRA can look like, and how each knob is chosen. Quick check of what your
terminal actually got: `ira --check-terminal`.

## Built-in themes

Press `\` (backslash) to switch to the next theme. The bottom banner shows
`Switched to <name>` for a few seconds, the `Actions` box title shows the active one
(`Actions · Nord`), and the choice is saved immediately to `~/.config/ira/state`
(`theme=<id>`), so the next launch starts where you left off. Consecutive presses walk
this list in order and wrap around:

| # | `id` (for `preset =` / `theme=`) | Label in the UI | Look | bg | text | accent |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `mocha` | Catppuccin Mocha | Soft pastel dark; the default | `#1e1e2e` | `#cdd6f4` | `#89b4fa` |
| 2 | `cyberpunk2077` | Cyberpunk 2077 | Night City: yellow HUD, cyan, trauma-team red | `#0a0a0c` | `#f5f0c8` | `#fcee0a` |
| 3 | `gruvbox-dark` | Gruvbox Dark | Warm retro browns and yellows | `#282828` | `#ebdbb2` | `#fabd2f` |
| 4 | `nord` | Nord | Arctic, bluish, low contrast | `#2e3440` | `#eceff4` | `#88c0d0` |
| 5 | `dracula` | Dracula | Purple / pink on deep indigo | `#282a36` | `#f8f8f2` | `#bd93f9` |
| 6 | `tokyo-night` | Tokyo Night | Cool night-city blues and violets | `#1a1b26` | `#c0caf5` | `#7aa2f7` |

Accepted aliases (case-insensitive, `_` = `-`): `default`, `catppuccin` → mocha;
`cyberpunk`, `2077`, `nightcity` → cyberpunk2077; `gruvbox` → gruvbox-dark;
`tokyonight`, `tokyo` → tokyo-night.

All six palettes are truecolor. On terminals without truecolor (Terminal.app, plain
`xterm`, old `screen`) every color is quantized to the nearest xterm-256 index, so the
theme still reads the same instead of collapsing to eight ANSI colors.

### Which theme starts

1. `theme=<id>` in `~/.config/ira/state` — written every time you press `\`.
2. `preset = "<id>"` in `~/.config/ira/theme.toml`.
3. `mocha`.

So `theme.toml` sets the theme for a fresh profile; `\` overrides it from then on.
To pin a theme permanently, delete the `theme=` line from the state file (or simply cycle
back to the one you want, since the last press is what gets saved).

### Per-key overrides

`~/.config/ira/theme.toml` can also override individual colors. Overrides are applied
**on top of whichever preset is active**, including after cycling with `\`, so a custom
`accent` follows you across every theme. Partial files are fine; unknown keys and
invalid colors are ignored. Values are `#rrggbb` hex or ratatui color names (`red`,
`light_blue`, …).

```toml
preset = "tokyo-night"   # starting theme for a fresh profile
icons = "nerd"           # or "unicode" / "auto"
chips = "outline"        # or "square" / "rounded"; default: outline when icons are nerd

# top-level keys
# bg surface surface_alt border border_active text text_muted accent
# key_fg key_bg success warning error info cursor_bg cursor_fg selection
# dir hidden shadow
accent = "#ff9e64"

[files]
# image video audio archive code document executable data
image = "#e0af68"
```

A ready-made file is in [`theme.cyberpunk2077.toml`](theme.cyberpunk2077.toml).

### Key chips

Shortcut keys are one row tall, so they cannot carry a real box border; instead
`chips = "..."` in `theme.toml` picks one of three one-row framings:

| `chips` | Look | Notes |
| --- | --- | --- |
| `outline` | `+` — bold `accent` key between thin rounded caps in `border_active` | Default with Nerd icons. Echoes the panels' rounded borders; no fill. Falls back to `(+)` without a Nerd Font. |
| `square` | ` + ` in `key_fg` on a filled `key_bg` rectangle | Default with Unicode icons; works on any font. |
| `rounded` | ` + ` — the filled body between thick half-circle caps | Stadium shape; two cells wider, the Actions box grows to match. |

The caps are Nerd Font Powerline glyphs (U+E0B4–E0B7), single-width in every Mono
Nerd Font. Unknown values fall back to the automatic choice.

## Fonts

IRA is a TUI: it draws characters and colors into cells the terminal owns. It **cannot
ship, embed, or select a font** — the typeface, size, ligatures and line height all come
from your terminal emulator's settings. What IRA controls is *which* characters it emits,
which is where the icon sets come in.

Recommended: any **Mono Nerd Font** patched build. The ones that render every glyph IRA
uses cleanly at width 1:

- JetBrainsMono Nerd Font Mono
- FiraCode Nerd Font Mono
- Hack Nerd Font Mono
- CaskaydiaCove Nerd Font Mono (Cascadia Code)
- MesloLGS Nerd Font Mono
- Iosevka Nerd Font Mono

Prefer the `Mono` variant of each family: the non-Mono variants use double-width glyphs
that can push columns out of alignment on some terminals (notably on Windows).

## Icon sets

There are three sets.

| Set | `icons =` / `IRA_ICONS` | Needs | What you see |
| --- | --- | --- | --- |
| Nerd | `nerd` | A Nerd Font reachable by the terminal | Per-extension glyphs (Rust gear, Python, PDF, archive, …), drive / bookmark / common-folder icons, Powerline key chips |
| Emoji | `emoji` | A terminal that renders color emoji two cells wide (its own fallback font does the drawing — nothing to install) | 📁 folders (🏠 💻 📚 📥 🎵 🎬 for the common ones, 🌿 `.git`, 📦 `node_modules`), 🦀 Rust, 🐍 Python, 🟨 JS, 🟦 TS, 🌐 HTML, 🎨 CSS, 🐹 Go, ☕ Java, 💎 Ruby, 🔩 C/C++, 📝 Markdown, 🧾 JSON, 🔧 config, 📄 text, 📷 image, 🎬 video, 🎵 audio, 📦 archive, 📕 PDF, 📘 document, 📊 spreadsheet, 🚀 executable, 📇 data, 💿 disk image, 📃 other |
| Unicode | `unicode` | Nothing — stock fonts | `□` folder, `≡` text, `λ` code, `▦` image, `▶` video, `♪` audio, `▣` archive, `▤` document/PDF, `▥` spreadsheet, `▸` executable, `◈` data, `◉` disk image, `·` other; every glyph is asserted single-width |

The emoji set is restricted to codepoints Unicode classes as wide (`East_Asian_Width=W`,
single scalars, no variation selectors or ZWJ sequences), so every terminal that follows
the standard gives them exactly two cells — the same two cells the Nerd set reserves
(glyph + pad space) — and the name column never shifts. Ambiguous-width symbols such as
⚙ 🖼 🗄 🖥 are deliberately excluded because terminals disagree on their width; a test
asserts the whole set.

### Which icon set is used

1. `IRA_ICONS=nerd|emoji|unicode` environment variable (one-shot override).
2. `icons = "..."` in `theme.toml`.
3. Auto-detect, re-run on every launch, richest set the terminal can draw:
   - **Nerd** when a font that actually contains the glyphs is reachable by the terminal
     (see below), or when `NERD_FONT=1` is set;
   - else **Emoji** when the terminal is known to render wide color emoji: Windows
     Terminal, every macOS terminal, VS Code / Cursor, GNOME Terminal and other VTE
     hosts (`VTE_VERSION`), Konsole, kitty, WezTerm, Ghostty, Alacritty, foot, Warp;
   - else **Unicode** (legacy Windows console, Linux console, unrecognized hosts).

`auto` in either `IRA_ICONS` or `icons =` skips that step and falls through. The icon
set is never saved to the state file; change fonts or terminals and the next launch
follows.

### How auto-detect finds a font

A terminal app cannot pick or ship a font, so IRA checks where the terminal's glyph
lookup will end up, in this order:

1. **Terminals that bundle Symbols Nerd Font** — kitty (0.36+), WezTerm, Ghostty, Warp —
   always render the glyphs, whatever font you selected.
2. **Profile font from the terminal's own settings.** Windows Terminal
   (`settings.json`, the profile named by `WT_PROFILE_ID`, else `profiles.defaults`)
   and the VS Code / Cursor integrated terminal (`terminal.integrated.fontFamily`, else
   `editor.fontFamily`). A face named like a Nerd Font (`… Nerd Font`, `… NF` / `NFM`
   / `NFP`, `Caskaydia`, `Delugia`, or such a font in a comma-separated fallback list)
   counts. Windows Terminal renders with DirectWrite, which does not reach other
   installed fonts for these glyphs, so a plain profile font there — the stock
   `Cascadia Mono` included — is a definitive "no".
3. **Installed fonts (Linux, macOS).** These terminals fall back per glyph to any
   installed font, so an installed `Symbols Nerd Font Mono` or any patched font is
   enough even when the profile font is plain. Linux asks fontconfig for fonts covering
   the sample glyphs (`fc-list ':charset=f07b e718 e0b6'`), falling back to a scan of
   the font directories by file name when `fc-list` is missing; macOS scans
   `~/Library/Fonts`, `/Library/Fonts` and `/System/Library/Fonts`.

If icons still show as boxes (`▯`) or stray symbols, `ira --check-terminal` prints which
of the three found (or did not find) a font; `IRA_ICONS=unicode` or `icons = "unicode"`
forces the fallback set, `icons = "emoji"` forces emoji on a host the list above misses.

## Terminal notes

- **Terminal.app (macOS)** is 256-color only and has no Nerd Font by default: expect the
  quantized palette and Unicode icons unless a Nerd Font is installed (CoreText falls
  back to it per glyph, so it does not have to be the selected font).
- **Windows Terminal** ships Cascadia Mono, which has no Nerd glyphs, and does not fall
  back to other installed fonts for them, so the default is the emoji set (Segoe UI
  Emoji, in color). It switches to Nerd glyphs as soon as the profile font is one that
  has them (e.g. `Cascadia Mono NF`, or a fallback list such as
  `Cascadia Mono, Symbols Nerd Font Mono`). Image previews use a transparent
  Win32 overlay (silent probe + `WT_SESSION`); `IRA_IMAGES=sixel` forces Sixel.
  VS Code's integrated terminal on Windows is not a console HWND — overlay
  placement is skipped and previews stay on the braille underlay.
- **Terminal.app** has no image protocol. Previews use a transparent AppKit
  overlay over the thumbnail cells (2×4 braille underlay if the window cannot
  be located). iTerm2, Ghostty, kitty and WezTerm keep their in-terminal
  protocols.
- **iTerm2, WezTerm, kitty, ghostty, Alacritty, Windows Terminal** are truecolor; set
  `COLORTERM=truecolor` if your shell profile clears it.
- **tmux / screen** pass truecolor only when configured (`set -g default-terminal
  "tmux-256color"` plus `set -ga terminal-overrides ",*:Tc"`); without that, IRA
  quantizes.

## `ira --check-terminal`

Prints what was resolved without starting the UI:

```
ira 0.1.15
truecolor: yes
icons: nerd (auto: a font with Nerd glyphs is available)
nerd glyphs: yes (installed font "JetBrainsMono Nerd Font")
theme: dracula (from session state, last `\` press)
image protocol: Kitty (from terminal query)
cell size: 9x18 px
theme file: /home/you/.config/ira/theme.toml (present)
```

The `nerd glyphs` line names the source: `yes (kitty bundles them)`, `yes (profile font
"Cascadia Mono NF")`, `yes (installed font "…")`, `no (profile font "Cascadia Mono")`
or `no (no covering font found)`.

## Config locations

| OS | `theme.toml` and `state` live in |
| --- | --- |
| Linux | `~/.config/ira/` (`$XDG_CONFIG_HOME/ira/`) |
| macOS | `~/Library/Application Support/ira/` |
| Windows | `%APPDATA%\ira\` (`C:\Users\<you>\AppData\Roaming\ira\`) |

The rest of this page writes `~/.config/ira/` for short.
