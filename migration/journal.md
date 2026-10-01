# Journal (append only)

Format: `YYYY-MM-DD HH:MM | action | result | sha`
2026-10-01 15:10 | Phase 0: installed migration kit, armed full migration goal, tagged and pushed current TUI as tui-oracle-baseline | root cargo build passed; cargo test reported 3 operations failures during concurrent GPUI build/quota pressure; desktop build pending | 1cad4ce
2026-10-01 15:32 | Classified Phase 0 tests and established direct-PTY oracle workflow | both builds passed; root tests pass with `TMPDIR="$PWD/target/tmp"`; root-wide fmt/clippy have legacy findings and remain unmodified; TUI startup needs terminal capability probing to finish before capture | 1f2f5a3
2026-10-01 15:34 | Reproduced safe interactive TUI operation and documented oracle driver | direct exec PTY at 120x40: Home (`w`) then `n`, typed `notes.txt`, Enter created file inside isolated HOME; `q` exited 0; raw failed pty probe removed | 1f2f5a3
2026-10-01 15:45 | Completed four discovery slices with live PTY evidence | 52 inventory entries; startup/session/config/bookmark/navigation/filter/help/error, create/rename/copy/move behavior captured; state/search and preview/external follow-ups active; no feature source code changed | 1f2f5a3
