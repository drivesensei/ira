# Copy / Move between panes — plan

Status: **IMPLEMENTED** (kept as the design record; see `docs/features.md` for the
user-facing key reference).

## Goal

Copy or move the selected file/folder from the **active** pane to the **other**
pane with a single key — no separate paste step. The operation must run
asynchronously so the TUI stays responsive while large files or directory trees
are being copied.

## Shortcuts

| Key | Action |
| --- | --- |
| `c` | Copy the selected entry to the other pane |
| `m` | Move (cut) the selected entry to the other pane |

The target is always the **other pane's current folder**. There is no
"paste" key; the destination is implicit.

### Availability check

Both keys are free, but each requires one small adjustment:

1. **`c`** — today it is shadowed by the `Ctrl-C` quit arm:
   `KeyCode::Char('c') | KeyCode::Char('C')` (a no-op for plain `c`).
   Refactor it to a guarded pattern so plain `c` falls through:
   ```rust
   KeyCode::Char(c) if key_event.modifiers == KeyModifiers::CONTROL
       && (c == 'c' || c == 'C') => app.quit(),
   ```
   `c` is already in `RESERVED_KEYS`, so it is not a bookmark letter — no
   bookmark conflict.

2. **`m`** — today it is a *free bookmark letter* (it appears in the
   auto-assigned sequence `o p a s d f g h j k l v n m`). Reserve it by adding
   `'m'` to `RESERVED_KEYS` in `src/services/bookmarks.rs`. The bookmark
   sequence then becomes `o p a s d f g h j k l v n` (13 letters).

Neither key collides with drives (`1`–`9`), common folders (`w e r t y u i`),
`q`, `z`, `x`, `/`, `b`, `+`, or `Tab`.

## Design

### Async model

File I/O never runs on the main (render/input) thread.

- `App` gains an `mpsc::Receiver<OpResult>` and a status field (e.g.
  `op_status: Option<String>`).
- `copy_to_other_pane()` / `move_to_other_pane()` validate inputs, then
  `std::thread::spawn` a worker that performs the operation and sends an
  `OpResult` (success + affected panes, or an error string) over the channel.
- On the existing `tick()` (2 s cadence), `App` drains the channel, clears the
  status on completion, and re-lists the affected pane(s) so the file lists
  reflect the result.
- The status (e.g. "Copying `foo/` …", "Done", "Error: permission denied") is
  rendered in the Actions box or a small status row.

### Copy semantics

- Recursive copy for directories (hand-rolled with `std::fs`, no new
  dependency), preserving files and nested structure.
- Symlinks: copy the symlink itself (do not follow) for v1.
- Destination name collision → **skip + report** for v1 (safest); a later
  version can offer overwrite / auto-rename (`name (1)`).

### Move semantics

- Try `std::fs::rename` first (instant when source and destination are on the
  same filesystem).
- On `ErrorKind::CrossesDevices` (`EXDEV`), fall back to copy + delete source.
- Keep the source entry's name in the destination.

### Operation inputs

- Source: `pane().state.selected()` → the selected entry's full path
  (reuse `visible_entry` so fuzzy-search selection maps correctly).
- Destination directory: the **other** pane's `folder.path`.
- Refuse if: nothing selected, the other pane has no folder, source is missing,
  or source == destination directory (or destination is inside source, i.e.
  copying a directory into its own subtree).

## Data flow

1. User selects an entry in the active pane, presses `c` (copy) or `m` (move).
2. `handler.rs` dispatches to `App::copy_to_other_pane()` / `move_to_other_pane()`.
3. `App` resolves source + destination, validates, sets `op_status`, spawns the
   worker thread, and moves the `Sender` (the `Receiver` lives in `App`).
4. Worker does the I/O and sends `OpResult` on completion/error.
5. On `tick()`, `App` drains `OpResult`s, updates `op_status`, and on success
   re-lists the destination pane (and source pane for a move).

## Edge cases to handle

- Copy/move a directory into its own subtree → reject.
- Cross-filesystem move (`EXDEV`) → copy + delete fallback.
- Permission / not-found errors → report, leave state consistent.
- Empty destination pane (no folder) → report "no target pane".
- Very large trees → async already; optional per-file progress in the status.

## Long-running operations

Copy/move can run for minutes or hours. Each operation is modeled as a **job**
so many can run at once and each can be observed, paused, or cancelled. This
refines the single-op `OpResult` channel above into a job list + event stream.

### Job model

```rust
enum JobKind { Copy, Move }
enum JobStatus { Running, Paused, Cancelled, Done, Failed(String) }

struct Job {
    id: u64,
    kind: JobKind,
    source: PathBuf,
    dest_dir: PathBuf,
    total_bytes: Option<u64>,   // None = unknown -> indeterminate bar
    copied_bytes: u64,
    current_path: Option<PathBuf>,
    status: JobStatus,
    started_at: Instant,
    cancel: Arc<AtomicBool>,
    pause: Arc<(Mutex<bool>, Condvar)>,
}
```

- `App` holds `jobs: Vec<Job>` and an `mpsc::Receiver<JobEvent>`.
- Jobs run concurrently (one thread per job, or a small pool).

### Progress

- `total_bytes` is filled by a pre-scan (sum of file sizes). If the tree is
  huge, leave it `None` and show an indeterminate bar.
- The worker copies in chunks (e.g. 256 KiB-1 MiB) and emits throttled events
  (~10/s): `Progress { id, bytes, current_path }`, `Done { id }`,
  `Failed { id, error }`, `Cancelled { id }`.
- `tick()` drains events and updates jobs; the board shows `done/total`, a
  percentage bar, throughput, and ETA.

### Cancellation

- `cancel` is checked between chunks/entries. On cancel, the worker stops,
  removes the partial destination entry, and emits `Cancelled`.

### Pause / resume

- `pause` uses a `Condvar` so a paused worker blocks (no busy-wait). Resume
  signals it. (A v1 could poll `AtomicBool` with a short sleep.)

## Copy Board sidebar

A side panel listing every job with progress and controls.

### Actions entry

Add a second action to the Actions box:

```
[+] Split Pane
[`] Copy Board
```

Toggle key: the backtick key (`` ` ``). It is free — no collision with drives,
common folders, bookmarks, `c`/`m`, or the other keys. Alternatives: `\` or
`F2`.

### Layout

When open, the Copy Board is a sidebar to the right of the files area. One row
per job:

```
[>] Copy  foo/  ->  /mnt/backup   72%  3.2 GiB/4.4 GiB  41 MiB/s  ETA 00:29
[ ] Move  a.mp4 ->  /run/media/..  done
[x] Copy  logs/ ->  /tmp           35%  (cancelled)
```

A row shows: kind, source name, destination, a progress bar + percentage, bytes,
throughput, and ETA (or status for finished/cancelled/failed jobs).

### Interaction (contextual — only while the board has focus)

- `Tab` cycles focus: pane 0 -> pane 1 -> Copy Board -> pane 0.
- Up/Down select a job.
- `Space` (or `p`) pause/resume the selected running job.
- `x` cancel the selected running job.
- `Esc` (or backtick again) close the board.

These keys act on the board only when it is focused; the global bindings are
unchanged when a pane has focus.

### History

Keep a bounded history (last ~20) of finished/cancelled/failed jobs so results
can be verified; prune older entries.

## Acceptance criteria

- `c` copies the selected entry to the other pane; `m` moves it.
- The TUI stays responsive during a large copy/move (no freeze).
- Both panes' file lists refresh correctly after completion.
- Errors (missing source, no target, permission, name collision) are reported
  without crashing or corrupting state.
- `c`/`m` do not shadow `Ctrl-C`, and `m` is no longer offered as a bookmark
  letter.
- Long operations show live progress; the Copy Board lists all active jobs.
- Any running job can be paused/resumed and cancelled; cancellation leaves no
  partial destination entry.
- Multiple simultaneous copies are supported and all appear in the board.
