# Direct PTY capture: text preview and built-in editor

- Starting state: fresh temp HOME and XDG config, 120×40 PTY, wait for startup capability query to settle, `w` to Home. Directory contains `README` with `first line\nsecond line\n` plus other small files.
- Key sequence: `v`, Down arrow, wait for async text load, `Tab`, `Esc`, then quit without edits.
- Observed: Column mode displays `README` content verbatim as `first line` and `second line`; initial preview was `loading…`. Tab focuses the IRA text-area editor in the preview region, labeled `Editing — Ctrl+S save · Esc exit`, with cursor at first line. Esc exits back to preview. The file bytes remained unchanged.
- Evidence limitations: did not type/save, did not exercise dirty-discard, read-only, >5 MiB, invalid UTF-8, NUL, or concurrent-change branches dynamically; related branches are represented by source and unit-test evidence in inventory.
