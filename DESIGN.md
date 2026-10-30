# DESIGN.md — IRA Desktop (Zed Workbench Edition)

> Source: Zed Editor GPUI Workbench Design System + `impeccable` v4.7.1 Operate Mode & Craft Floor (`craft-floor.md`, `operate.md`).
> Owner: UI lead. Every UI agent reads this before touching UI. Tokens in code map from `ira_core::theme::Theme`.

## 1. Visual Theme & Direction (Zed Workbench)
IRA Desktop adopts the **Zed Editor** GPUI workbench aesthetic under `impeccable` **Operate** mode:
- **Edge-to-edge structural grid**: Workbench regions (top workspace bar, places/bookmarks bar, dual-pane editor/file canvas, bottom-up terminal dock, and bottom status bar) fit together as a cohesive IDE shell separated by `1px` structural hairlines (`theme.border`), eliminating floating double-bordered boxes.
- **Craft Floor Compliance (`impeccable`)**:
  - **Refuse**: No gradient text, no decorative blurs/glows, no >1px side stripes, no kicker/eyebrow labels above headings, no nested card-in-card containers.
  - **Precision**: High-contrast primary text (`theme.text`), restrained chrome (`theme.surface_alt` for headers/docks/toolbars, `theme.bg` for file canvas), accent color (`theme.accent`) reserved strictly for active pane tab indicator, active cursor row, selection marks, and live status signals.

## 2. Colour Palette & Surface Hierarchy
Mapped dynamically from `ira_core::theme::Theme` (all 6 built-in presets + light counterpart):

| Zed Workbench Role | Theme Token | Usage |
|---|---|---|
| Editor / File Canvas | `theme.bg` | File list rows, grid background, terminal output viewport |
| Panel / Dock Surface | `theme.surface` | Modal dialogs, inactive tab strip, inline input background |
| Titlebar / Tab Bar / Status Bar | `theme.surface_alt` | Top drive/places bar, pane tab header, details column header, bottom terminal dock header, bottom status bar |
| Structural Hairline | `theme.border` | `1px` dividers between toolbar, panes, preview split, bottom terminal dock, and status bar |
| Active Focus Border | `theme.border_active` | Active pane border/indicator, focused modal border, active terminal dock border |
| Active Row / Selection | `theme.cursor_bg` / `theme.selection` | Active cursor row in focused pane (`cursor_bg` + `cursor_fg`), multi-selected rows & hover (`selection`) |
| Primary & Muted Ink | `theme.text` / `theme.text_muted` | Filenames & active tab (`text`); breadcrumb parent path, file sizes, timestamps, inactive tabs (`text_muted`) |
| Semantic Signals | `theme.accent`, `theme.info`, `theme.success`, `theme.warning`, `theme.error` | Active tab top line, terminal running dot, dirty editor `*`, job status, error notices |

## 3. Typography & Scale (`impeccable` Operate Mode)
- Fixed, tight pixel scale (1.12–1.18 step ratio):
  - **Dialog Title**: `13px`, `FontWeight::BOLD`.
  - **File Row / Terminal Input & Output**: `12px–13px`, regular for files, `FontWeight::BOLD` for directories.
  - **Tab Bar / Breadcrumb / Toolbar**: `12px` (`FontWeight::BOLD` on active folder tab, regular `theme.text_muted` on parent path).
  - **Details Column Header / Status Bar / Key Chips**: `11px` (`theme.text_muted`), right-aligned for `Size` (`90px`) and `Modified` (`132px`) columns.

## 4. Zed Workbench Layout & Component Architecture
1. **Top Workbench Bars (`ira-drives-row`, `ira-common-folders-row`, `ira-bookmarks-and-actions-row`)**:
   - Compact Zed toolbar strips (`py(px(3.))`, `px(px(8.))`, `bg(theme.surface_alt)`) with inline section label pill on the left (`Drives`, `Folders`, `Bookmarks`) and horizontally aligned shortcut chips on the right, eliminating stacked two-line header boxes so the file canvas gets maximum vertical room.
2. **Dual-Pane File & Preview Split (`pane`)**:
   - **Zed Tab / Breadcrumb Header (`h(px(28.))`)**:
     - Left: Active pane indicator dot/bar (`theme.accent` when focused), folder icon, bold folder name tab, and muted parent path breadcrumb.
     - Right: Active filter chip (`/{filter}`), hidden files badge (`. hidden`), sort mode badge (`sort: name|size|modified`), and view mode badge (`list` / `preview` / `details` / `grid`).
   - **Column Preview Mode (`PreviewMode::Column`)**:
     - Interior container MUST use `.flex().flex_row().flex_1().w_full().h_full().min_h_0().min_w_0()`.
     - Left file list wrapper MUST use `.flex().flex_col().flex_1().h_full().min_h_0().min_w(px(200.))` (with `flex_basis(relative(0.58))` for images and `flex_basis(relative(0.38))` for text preview, plus `.flex_grow()`), and the inner `uniform_list` MUST have `.w_full().h_full().flex_1().min_h_0()` so the file list NEVER disappears!
     - Right preview/editor column uses `.flex().flex_col().h_full().min_h_0().min_w(px(220.))` separated by a `1px` `border_l_1` hairline.
   - **Details Mode (`PreviewMode::Details`) & List Rows (`file_row`)**:
     - In `PreviewMode::Details`, render a Zed-style column header bar (`h(px(22.))`, `px_3()`, `bg(theme.surface_alt)`, `border_b_1()`, `text_size(px(11.))`, `text_color(theme.text_muted)`) with `"Name"` (`flex_1()`), `"Size"` (`w(px(90.)).text_right()`), and `"Modified"` (`w(px(132.)).text_right()`).
     - `uniform_list` MUST have `.w_full().h_full().flex_1().min_h_0()` and `file_row` (when `!grid`) MUST have `.w_full().h(px(30.))` so every row spans 100% of the pane width and right-aligned columns sit flush against the right edge.
3. **Zed-Style Embedded Bottom Terminal Panel (`#ira-terminal-dock`)**:
   - Triggered by pressing `0` (or clicking the `0 Terminal` chip in Actions / status bar).
   - Docked at the bottom of the workspace above `#ira-status-footer` (`h(px(240.))`, `w_full()`, `border_t_1()`, `border_color(theme.border_active when focused else theme.border)`, `bg(theme.bg)`).
   - **Dock Header Bar (`h(px(26.))`, `bg(theme.surface_alt)`, `border_b_1()`)**:
     - Left: Terminal tab badge (`"Terminal"` with `●` status dot: `theme.success` when shell/command idle, `theme.accent` when running, `theme.error` on non-zero exit), current working directory (`cwd`), and shell name (`$SHELL` / `sh` / `cmd`).
     - Right: Interactive controls (`Clear`, `Focus / Pane (Tab)`, `Close (0 / Esc)`).
   - **Output Viewport**: Scrollable monospace output buffer (`flex_1()`, `min_h_0()`, `overflow_y_scroll()`, `px_3()`, `py_2()`, `text_size(px(12.))`) displaying command transcript and live stdout/stderr lines.
   - **Command Input Row (`h(px(28.))`, `border_t_1()`, `bg(theme.surface)`)**:
     - Prompt prefix (`❯` in `theme.accent`) + interactive `TextInput` entity (`InputMode` or dedicated terminal input entity) where pressing `Enter` executes the command in the embedded shell session (supporting `cd <dir>` to update `cwd`, `clear` to clear scrollback, `exit` to close the panel, and background non-blocking `Command::new(shell)` execution streaming lines into the buffer) and `Esc` returns focus to the active file pane (or closes when empty).
4. **Zed-Style Status Bar (`#ira-status-footer`)**:
   - Compact `24px` footer bar along the bottom edge with mode badge (`NORMAL` / `SEARCH` / `EDIT` / `TERMINAL`), active status/feedback notice or contextual key hints, and right-hand dock toggles (`0 Terminal`, `* Keybindings`).
