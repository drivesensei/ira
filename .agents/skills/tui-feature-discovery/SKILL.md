---
name: tui-feature-discovery
description: How to exhaustively learn every feature of the terminal file manager before migrating it. Static code analysis plus live runtime capture, slice list, file manager feature probe checklist, inventory format, and the surface.txt contract. Use for tui-discovery-explorer agents and when the manager plans discovery.
---

# TUI feature discovery

Goal: build a complete, evidence-backed picture of everything the TUI does, so that "1:1" is measurable.
You do not know this codebase in advance. Do not guess; read and run it.

## 1. Two evidence channels (use both for every slice)

**Static**: read the source. For each behavior, record `path:line` refs and the function names.
**Dynamic**: run the TUI and observe. Code lies by omission (defaults, config, terminal quirks, timing).

### Driving the TUI headlessly (recommended: tmux)

```bash
SESSION=oracle-$$
tmux new-session -d -s $SESSION -x 120 -y 40 "cd $FIXTURE && $ORACLE_BIN"
tmux send-keys -t $SESSION 'j' 'j' 'Enter'            # keys; use C-x, M-x, Space, Escape, etc.
sleep 0.3
tmux capture-pane -t $SESSION -p        > migration/oracle/captures/<area>/<scenario>.txt      # plain
tmux capture-pane -t $SESSION -p -e     > migration/oracle/captures/<area>/<scenario>.ansi.txt # with colors
tmux kill-session -t $SESSION
```

Rules:
- Run against a **throwaway fixture tree** created by a script stored in `migration/oracle/fixtures/make_fixture.sh`
  (and a Windows-friendly Python variant). Never run destructive features on real data.
- Use a fresh `HOME`/config dir per scenario (`HOME=$(mktemp -d)`, and XDG or platform config env overrides) to see
  first-run behavior, then again with a populated config.
- Record: keys sent, terminal size, resulting screen, resulting filesystem diff (`diff -r` of fixture before and
  after), process exit code, anything printed to stdout/stderr after exit, files written outside the fixture.
- Resize the pane (`tmux resize-window -x 60 -y 15`) to see layout fallbacks and truncation rules.
- If a scenario is timing-dependent (animations, debounce, repeat), note intervals in the capture header.
- If tmux is unavailable, use a pty harness (Python `pty`, or Rust `portable-pty`/`expectrl`) with a `vt100`
  parser. Store the harness under `migration/oracle/tools/`.

## 2. Slices (one explorer per slice; the manager adjusts to the real source layout)

1. Entry points: `main`, CLI parsing (every flag, subcommand, positional, exit codes, stdout contract), env vars,
   startup sequence, shell integration (cd-on-exit, chooser file, stdin handling), logging, panics.
2. Configuration and persisted state: file locations per OS, formats, defaults, schema, validation, error
   messages, migrations, bookmarks/history/session/trash metadata, theme and keymap files.
3. Input system: event loop, key decoding, key binding tables, modes (normal/insert/visual/command/search),
   multi-key sequences, counts/prefixes, timeouts, mouse handling, paste, resize, focus events.
4. Core state model: app struct, tabs/panes, cursor/selection model, sort/filter/hidden, view modes, history stack.
5. Filesystem layer: listing, metadata, symlinks, permissions, special files, watchers/refresh, caching, errors.
6. File operations: copy, move, delete, trash, rename, bulk rename, create, link, chmod/chown, archive ops, undo,
   conflict resolution policy, progress, cancellation, background job queue.
7. Search and filtering: name, glob, regex, fuzzy, incremental, content search, jump (zoxide/fzf-like), results UI.
8. Preview and viewers: text, syntax highlighting, images, archives, binary/hex, markdown, PDF, external previewers.
9. Rendering and widgets: layout, panes, columns, status/header/footer lines, popups, dialogs, prompts, help screen,
   icons, colors/themes, truncation and alignment rules, number/date/size formatting.
10. External integration: open-with, editor/pager/shell spawn, clipboard, drag/drop equivalents, mounts/devices,
    git status, remote/VFS (ssh/ftp/s3?), plugins, scripting hooks, command mode (`:commands`).
11. Errors and messaging: every error/notification/confirmation string and when it appears; status line behavior.
12. Tests and docs already in the repo: existing unit/integration tests, README, man pages, CHANGELOG, help text.
    These are claims; verify them against runtime.
13. Orphan sweep: any source file not claimed by slices 1-12.

## 3. Feature probe checklist (find these or prove they do not exist; record N/A with evidence)

Navigation: up/down/left/right, enter dir, parent, home/end/page, jump to path, go to home/root/last dir, history
back/forward, jump list, bookmarks/marks, tabs, panes, miller columns, breadcrumbs, mouse.
Listing: sort keys and directions, natural sort, case rules, dirs-first, hidden toggle, filter, columns, size/date
formats, symlink display, permissions display, file type colors, icons, git markers, empty dir message.
Selection: single, multi, range/visual, invert, select by glob/regex, select all, clear, persistence across refresh,
selection count/size display.
File operations: yank/cut/paste, copy, move, delete (permanent), trash, rename (inline and editor), bulk rename,
mkdir, touch, symlink/hardlink, chmod, chown, archive/extract, duplicate, undo/redo, conflict dialog (overwrite,
skip, rename, apply-to-all), cross-device move, progress, cancel, error aggregation.
Search: find, filter-as-you-type, next/prev match, content grep, fuzzy finder integration, saved searches.
Preview: pane toggle, scroll, supported types, size limits, external commands, caching.
Commands: command palette or `:` mode, aliases, history, completion, shell escape (`!`), user-defined commands.
Config: file format, hot reload, theme, keymap overrides, per-directory settings, env var overrides.
System: open with default app, open with chosen app, terminal spawn, editor spawn, clipboard copy path/name,
mount/unmount, disk usage/free space, process/job list, notifications, bell.
Meta: help screen, about/version, first-run behavior, crash/panic handling, exit confirmation, quit codes,
session restore, multiple instances, locking.

For each probe: either create an inventory entry or record `NOT PRESENT (evidence: searched X, ran Y)`.

## 4. Inventory file format (`migration/inventory/<slice>.md`)

```markdown
# Inventory: <slice name>
Explorer run: <date> | TUI commit: <sha> | Files claimed: <list or glob>

## Features
### INV-<slice>-001 <short name>
- Trigger: key `x` in normal mode | command `:foo` | CLI `--bar` | automatic on ...
- Behavior: <precise, testable description. Include ordering, defaults, limits, edge cases>
- State read/written: <fields, files>
- Source refs: path:line, path:line
- Runtime evidence: migration/oracle/captures/<area>/<scenario>.txt (+ fixture diff)
- Edge cases observed: <list>
- Error behavior: <exact message or UI reaction, exit codes>
- Config knobs: <keys that alter it>
- Platform notes: <anything unix-only or terminal-only>
- Depends on: INV-... (other inventory ids)
- Confidence: high | medium | low (+ what would raise it)

## Unclaimed or uncertain
<anything you could not classify; open questions with how to resolve them>
```

Rules: one INV entry per distinct behavior; do not merge "rename" and "bulk rename". Quote exact strings. Prefer
captured evidence over inference. Mark uncertainty explicitly, never smooth it over.

## 5. `migration/oracle/surface.txt` contract

One item per line: `kind:name`. This file is the **coverage checklist** the final gate checks against the matrix.
Kinds: `key`, `cmd`, `cli`, `env`, `cfg`, `file`, `msg`, `mode`, `ui`, `exit`, `hook`.
Examples:
```
key:normal:j
key:normal:ctrl-x
key:normal:g g
cmd::rename
cli:--chooser-file
env:EDITOR
cfg:ui.sort_dirs_first
file:bookmarks.toml
msg:Destination already exists
mode:visual
ui:help-screen
exit:code:2
```
Extract mechanically where possible (parse the keymap table, clap definitions, serde config structs, error enums)
and complete manually. Over-inclusion is fine; omission is not.

## 6. Cross-check

For the three largest slices, the manager launches a second explorer **blind to the first inventory** and diffs the
results. Differences become either inventory corrections or new entries. Disagreement means the oracle is
unclear: run the TUI to settle it.

## 7. Explorer return format (to the manager; keep under 60 lines)

- Files written, counts of INV entries, counts of surface items added
- Top 10 riskiest or trickiest behaviors found
- Open questions and uncertainties
- Candidate dependencies spotted (which INV entries need which others)
- Any behavior that looks terminal-only, with your proposed desktop equivalent
