# Filesystem and operation captures

Oracle: `/home/vlad/Projects/ira/target/debug/ira`, frozen tag `tui-oracle-baseline` (`1cad4ce43cc72d52d4cc4eef920e0da22cb69568`). Fixture creation: `migration/oracle/fixtures/make_files_ops_fixture.py`. All mutation probes used disposable fixture trees and isolated HOME/XDG config.

The live TUI was driven with a 120×40 direct PTY (`stty cols 120 rows 40`) after a 12-second startup wait. Home (`w`) was used when starting from Drives. Initial listing and create/rename/copy/move flows were observed; capture files preserve dialogs/results and exact filesystem evidence. Copy and move captures use `copy-case`; fixture manifest identifies initial contents.

Create: `n`, type a name, Enter. Both folder and zero-byte file creation were confirmed on disk. Rename: Enter on selected file opens the prefilled Rename dialog; Enter commits. Renaming `a.txt` to existing `b.txt` displayed `Cannot rename: 'a.txt' already exists.` and left both files unchanged. Copy (`c`) and move (`m`) prompted `Copy 'a.txt' → right?` / `Move 'b.txt' → right?`; `y` accepted. Default conflict policy was `auto-rename`; copy preserved destination `a.txt` and created `a (2).txt` with source bytes. Move removed `left/b.txt` and created `right/b.txt`. Both completed jobs appeared in Copy Board.

Permanent-delete confirmation/progress, cancellation/pause, overwrite/skip conflict-policy cycling, recursive directory transfer, and error paths remain source/test-derived rather than dynamically verified. Trash/recycle and undo were not found in the TUI operation handlers.
