# State/search/jump TUI runtime capture notes

Oracle: `/home/vlad/Projects/ira/target/debug/ira` (built from frozen commit `1cad4ce43cc72d52d4cc4eef920e0da22cb69568`, tag `tui-oracle-baseline`). Runs used `functions.exec_command` with `tty:true`, `stty cols 120 rows 40`, an isolated `/tmp/ira-state-search.*` or `/tmp/ira-state-search-nav.*` fixture as cwd, and separate isolated `HOME` and `XDG_CONFIG_HOME`. The direct PTY answered the terminal capability query and displayed the initial UI. Home is shortcut `w` from Drives. All operations were navigation/search/selection/sort; no fixture files were modified. Quit with Ctrl+C; process returned terminal to normal mode.

## Fixture

First run root: `/tmp/ira-state-search.niYJx2/home`; contents:

```
Archive/data.bin (5 bytes)
alpha.txt (6 bytes)
alphabet.log (9 bytes)
bravo.md (6 bytes)
docs/readme.md (10 bytes)
documents/
gamma.txt (6 bytes)
.hidden.txt (7 bytes; hidden)
```

Second navigation fixture root: `/tmp/ira-state-search-nav.gVab9i/home`; entries `Archive/a.txt` (1 byte), `alpha.txt` (1 byte), `beta.txt` (9 bytes), `c.txt` (3 bytes), `docs/`.

## Initial listing and cursor/navigation (`120×40`)

From the initial `Drives` page, press `w`. The Home listing appeared with directories/files alphabetically: `Archive`, `alpha.txt`, `alphabet.log`, `bravo.md`, `docs`, `documents`, `gamma.txt`; `.hidden.txt` was absent. Highlight began on `Archive`.

On the second fixture, `x` moved highlight to last row `docs`; `z` moved it back to first row `Archive`; Down moved highlight to `alpha.txt`. From `Archive`, Right entered `/tmp/ira-state-search-nav.gVab9i/home/Archive` and showed `a.txt`. Left returned to Home and restored highlight to the `Archive` directory just left. No wrap was observed in the tested top/bottom movements; saturating behavior is also explicit in `src/app.rs` navigation code.

## Selection and size sort

On first fixture, Space on highlighted `Archive` changed its marker from `[ ]` to `[*]` and advanced the cursor to `alpha.txt`. Ctrl+A (PTY byte `0x01`) marked every visible row; pressing Ctrl+A again cleared all visible markers. On second fixture, `/alp`, Enter, Ctrl+A marked only visible `alpha.txt`; Esc cleared the confirmed filter and restored the full listing with `alpha.txt` still selected.

On first fixture, `,` displayed exact status `Sorted by size (largest first)`. The 9-byte `alphabet.log` rose ahead of 6-byte files, equal-size files were alphabetical, and directories followed the files. The other sort comparators are recorded from source (see inventory); mtime values in this fixture were too close to serve as a useful independent ranking probe.

## Fuzzy search: live/no-match/confirmed-filter

Starting on the first fixture’s Home directory, `/alp` immediately displayed only `alpha.txt` and `alphabet.log`; `alpha.txt` had the cursor. Enter promoted the query to a confirmed filter; while confirmed, Ctrl+A selected only the matching rows. Esc cleared the confirmed filter, displayed the full list, and retained the selected `alpha.txt` marker.

A separate `/zzzz` live query cleared all file rows and produced no empty-state message. Esc during search canceled and restored the full list. In a later reproduction, `/zzzz` then Enter (confirm) unexpectedly displayed the full unfiltered file list. Sending Down left the full list visible without a cursor highlight. Source explains the mismatch: action visibility is empty because `filter_query=Some("zzzz")` and `filter_indices=[]`, but the renderer treats a filter as active only if `filter_indices` is nonempty, then takes its full-list branch. This is a reproducible oracle defect, not an expected feature.

## Direct-path jump

Using second fixture, normal `[` then typing `/tmp/ira-state-search-nav.gVab9i/home/Archive/a.txt` and Enter closed the path prompt and navigated to the parent folder. Pressing `?` opened Info showing `Name: a.txt` and `Path: /tmp/ira-state-search-nav.gVab9i/home/Archive/a.txt`, confirming that the file was selected after navigation. The fixture file was not changed. The entry-input explorer owns text-editor edge cases; this capture confirms target resolution and cursor reveal.

## History/no-op/absence

Source sweep over `src/app.rs`, `src/handler.rs`, `src` for `history|back|forward|last.dir|last_dir|jump` found no recent folder history stack or back/forward action. Live runtime exercised Left parent navigation and restoration to the child just left; this is parent navigation, not arbitrary visit history. `[` is explicit path entry, not a fuzzy jump list. Bookmark key `o`/shortcut behavior is in config/bookmarks slice. No destructive/non-fixture action was attempted.

Capture limitations: direct terminal output is ANSI-driven, so this file is a human-readable transcription of the final screens/changed rows, with actions and fixture evidence. All dynamic interactions above used a real 120×40 exec PTY. This capture does not independently establish Modified/Kind sort ties, repeat-ramp timings, Alt/Meta modifier decoding, OS search normalization, empty-directory edge cases, or root/path permission errors.
