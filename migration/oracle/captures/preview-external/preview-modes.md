# Direct PTY capture: preview mode cycle

- Oracle binary: `/home/vlad/Projects/ira/target/debug/ira`, built from frozen TUI commit `1cad4ce43cc72d52d4cc4eef920e0da22cb69568`.
- Direct PTY command used 120 columns × 40 rows, `HOME=/tmp/ira-preview-ext-x425py9h/home`, `XDG_CONFIG_HOME=$HOME/.config`, `IRA_THUMBNAIL_CACHE_DIR=$HOME/cache`; clean throwaway home with six small entries.
- Startup emits terminal image-protocol and cell-size queries, so waited 13 seconds before keys. Initial visible screen was Drives / Common folders; pressing `w` selected Home and entered it. Home header reported `(details)`.
- With no selected row, pressed `v`: right side appeared with title `Preview` and body `select a file`; footer/status displayed `Preview: column`.
- Pressed Down arrow once to select `README`: preview column first showed `loading…`, then visibly rendered its two lines, `first line` and `second line`. No selection-side file change.
- Pressed `Tab`: title changed to `Editing — Ctrl+S save · Esc exit`; cursor appeared at first character of `first line`. Pressed Esc without typing; returned to preview and file list, fixture content unchanged.
- Sent `vvv`: visible intermediate Grid layout showed multiple small file tiles; status ended `Preview: off`, then `Preview: details`. The Grid test files include a syntactically minimal PNG and intentionally invalid media placeholders, so the capture confirms mode/layout and not successful image decode.
- Quit with `q`, process returned to shell. No fixture mutation from this scenario; in-memory preview choice may be persisted only on normal app shutdown in the isolated temporary HOME.

This is a concise behavioral transcript from returned PTY screen deltas, not a raw ANSI dump. Raw terminal escape output is noisy and does not make a useful review artifact.
