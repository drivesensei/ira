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

There are exactly two sets. Emoji were deliberately not added: their cell width differs
between Linux, macOS and Windows terminals and would break the file list columns.

| Set | `icons =` / `IRA_ICONS` | Needs | What you see |
| --- | --- | --- | --- |
| Nerd | `nerd` | A Nerd Font in the terminal | Per-extension glyphs (Rust gear, Python, PDF, archive, …), drive / bookmark / common-folder icons |
| Unicode | `unicode` | Nothing — stock fonts | `□` folder, `≡` text, `λ` code, `▦` image, `▶` video, `♪` audio, `▣` archive, `▤` document/PDF, `▥` spreadsheet, `▸` executable, `◈` data, `◉` disk image, `·` other; every glyph is asserted single-width |

### Which icon set is used

1. `IRA_ICONS=nerd|unicode` environment variable (one-shot override).
2. `icons = "..."` in `theme.toml`.
3. `icons=` in `~/.config/ira/state` — saved on every run, so once Nerd icons are on
   they stay on without the env var.
4. Auto-detect: Nerd when the terminal is one that ships or commonly pairs with a Nerd
   Font — kitty, WezTerm, ghostty, Alacritty, foot, Warp, VS Code's terminal, Windows
   Terminal — or when `NERD_FONT=1` is set. Otherwise Unicode.

`auto` in either `IRA_ICONS` or `icons =` skips that step and falls through.

If icons show as boxes (`▯`) you are in a terminal that auto-detected as Nerd-capable
but has no Nerd Font selected: either pick one in the terminal's settings or run once
with `IRA_ICONS=unicode` (which is then remembered).

## Terminal notes

- **Terminal.app (macOS)** is 256-color only and has no Nerd Font by default: expect the
  quantized palette and Unicode icons unless you install a Nerd Font and select it.
- **iTerm2, WezTerm, kitty, ghostty, Alacritty, Windows Terminal** are truecolor; set
  `COLORTERM=truecolor` if your shell profile clears it.
- **tmux / screen** pass truecolor only when configured (`set -g default-terminal
  "tmux-256color"` plus `set -ga terminal-overrides ",*:Tc"`); without that, IRA
  quantizes.

## `ira --check-terminal`

Prints what was resolved without starting the UI:

```
ira 0.1.6
truecolor: yes
icons: nerd (auto: nerd-capable terminal)
theme: dracula (from session state, last `\` press)
image protocol: Kitty
theme file: /home/you/.config/ira/theme.toml (present)
```
