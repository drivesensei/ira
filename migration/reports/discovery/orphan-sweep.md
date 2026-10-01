# Orphan sweep: source, tests, and documentation

Date: 2026-10-01
Oracle commit: `1cad4ce43cc72d52d4cc4eef920e0da22cb69568` (`tui-oracle-baseline^{}`)
Checkout HEAD observed: `5a99091743083b483de9999516329e492668cdad`; `git diff tui-oracle-baseline..HEAD -- src tests README.md docs` was empty.
Explorer: `discover_tests_docs`; no source/test files changed.

## Summary

- `src/**/*.rs` source files enumerated: **42**.
- Files directly claimed by existing discovery inventories or their explicit source references/wildcard claims: **35**.
- Concrete source implementation files without an explicit owning inventory: **7** (listed below). Two overlay platform files were marked uncertain in `preview-external.md`; that is not equivalent to an inventory of their implementation behavior.
- Module declaration files with no behavior of their own are not counted as concrete implementation orphans: `src/components/mod.rs`, `src/domain/mod.rs`, `src/services/mod.rs`, `src/utils/mod.rs`. `src/theme/mod.rs` is substantive and is claimed by config/theme and rendering inventories.
- The root package contains **278 tests** according to `cargo test --locked -- --list`: 218 unit tests, 60 integration tests, and 0 doc tests. The command lists tests and does not execute assertions. The parent manager separately reports the suite passed when run with `TMPDIR="$PWD/target/tmp"`.
- `README.md` quick keys are incomplete against the actual help overlay; docs/full_features.md is more complete. `--help` is not a CLI help mode.

## Source ownership reconciliation

`app.rs` and `handler.rs` are shared across feature slices; a “claimed” file means its relevant behaviors have inventory references, not that one inventory exhaustively documents the whole file.

| Source paths | Existing inventory / status |
| --- | --- |
| `src/main.rs`, `src/event.rs`, `src/tui.rs` | `entry-input.md` (entry, event, terminal lifecycle) |
| `src/app.rs`, `src/handler.rs` | Shared: `entry-input.md`, `config-state.md`, `files-ops.md`, `filesystem.md`, `preview-external.md`, `rendering-messages.md`, `state-search.md`; each owns separate behavior areas |
| `src/components/actions_ui.rs`, `bookmarks_ui.rs`, `common_folders_ui.rs`, `copy_board_ui.rs`, `drives_ui.rs`, `preview_ui.rs`, `tab1_files_ui.rs` | `rendering-messages.md` explicitly claims all `src/components/*_ui.rs`; feature ownership is shared with config, operations, filesystem, and preview inventories |
| `src/ui/chrome.rs`, `src/ui/mod.rs` | `rendering-messages.md` |
| `src/domain/data.rs` | `config-state.md`; `state-search.md` also cites `Folder` |
| `src/utils/fuzzy.rs`, `src/utils/is_dir.rs` | `state-search.md` |
| `src/services/blocks.rs`, `bookmarks.rs`, `clipboard.rs`, `drives.rs`, `file_info.rs`, `list_files.rs`, `overlay/mod.rs`, `picker_probe.rs`, `state.rs`, `thumbnails.rs`, `transfer.rs` | Respectively preview/render, config, external, platform/filesystem, metadata, filesystem, preview, preview, config, preview, and file-operations inventories; see the matching source refs in those inventories |
| `src/theme/caps.rs`, `icons.rs`, `mod.rs` | `config-state.md` and/or `rendering-messages.md`; substantive theme loader/capability/icon paths are inventoried there |
| `src/components/mod.rs`, `src/domain/mod.rs`, `src/services/mod.rs`, `src/utils/mod.rs` | Module-export declarations only; no independent runtime behavior. Their exported implementation modules are listed above or below. |
| `src/lib.rs` | **Unclaimed.** Public library re-exports all modules, but no slice inventoried the crate's public API surface. Low behavioral risk; assign it to the entry/core boundary and verify exported API compatibility during core split. |
| `src/services/folders.rs` | **Unclaimed.** `docs/features.md` describes common-folder resolution via `dirs-next`, but no inventory records this service's exact path resolution, missing-folder rules, ordering, or errors. Assign to common-folders/platform discovery. |
| `src/services/overlay/macos.rs`, `src/services/overlay/windows.rs` | **Unclaimed implementation detail.** `preview-external.md` flags these modules as uncertain, but native-window matching, geometry collection, overlay visibility, OS API failures, and teardown have no dedicated inventory/runtime record. Assign to platform/preview discovery. |
| `src/services/windows_drives_labels.rs` | **Unclaimed.** The filesystem/drive inventories describe drive listing at a higher level, but not volume-label API inputs, errors, fallback labels, or encoding. Assign to platform drive discovery. |
| `src/theme/font_probe.rs`, `src/theme/wt_font.rs` | **Unclaimed implementation detail.** Theme docs discuss font selection and tests exist, but current inventory headers/source refs do not cover Nerd Font filesystem scanning, terminal profile parsing/cache, VS Code settings, or Windows Terminal configuration precedence. Assign to theme/platform discovery. |

### Follow-up resolution

`migration/inventory/platform-helpers.md` now explicitly owns `src/lib.rs`, `src/services/folders.rs`, both platform overlay implementations and their shared module, Windows volume labels, common-folder presentation, and font/terminal-profile probing. This closes the concrete source ownership gap for all 42 enumerated Rust files. The behaviors are source-inventoried; macOS/Windows live UI behavior remains unverified and must not be marked parity-verified without target-platform evidence.

## Tests/docs audit and runtime checks

Test targets from `cargo test --locked -- --list`:

| Target | Count | Representative tested behavior |
| --- | ---: | --- |
| Library unit tests in `src/**` | 218 | State/theme, file listing, transfer, metadata walks, image/picker/overlay, font parsing, view geometry and formatting |
| `tests/drives_refresh_test.rs` | 2 | Async drive poll publication; nonblocking explicit refresh |
| `tests/operations_test.rs` | 52 | Selection, copy/move/delete, confirmations, rename/create/goto, filtering, async listing/transfer, info, persistence |
| `tests/sort_test.rs` | 6 | Sort cycling, cursor preservation, empty view and unknown timestamps |
| Rust doc tests | 0 | None |

CLI runtime checks used the prebuilt `target/debug/ira` baseline binary. `--version` and `-V` each printed exactly `ira 0.1.21` and exited 0. `--check-terminal` exited 0 and printed the expected diagnostic fields using an isolated temporary HOME/config root. A 120x40 PTY launch with `--help` displayed the regular TUI key-hint bar and normal file-manager panels; after Ctrl+C the process exited 0. This confirms the help string is ignored by the current argument dispatch rather than rendered as CLI help. The active `*` in-app help overlay remains the separate user-visible help mechanism; see `entry-input.md` and `migration/oracle/captures/entry-input/keybindings.txt`.

Documentation reviewed: `README.md`, `docs/features.md`, `docs/full_features.md`, `docs/themes.md`, `docs/copy-move-plan.md`, `docs/distribution-plan.md`, and `desktop/README.md`. The quick key table in README omits several working actions (including `n`, `[`, `]`, `0`, `-`, `Ctrl+-`, `,`, `v`, `+`, and `*` help), while the longer feature docs include most. The copy/move and distribution plans are design/historical documents; they must not override current oracle behavior or be treated as extra implemented capabilities.

## Remaining discovery gaps

## Manager follow-up: ownership closed

On 2026-10-01, `platform-helpers.md` was added to claim all seven concrete files listed above and the related common-folder renderer/service. The slice covers the library export surface, common-folder resolution/presentation, macOS/Windows overlay implementations, Windows volume labels, and font/Windows Terminal probes. This closes the source-file ownership gap across the enumerated 42 files. The original orphan findings are retained above as an audit trail.

Remaining discovery / verification gaps:

1. Native overlay and font probing are inventoried but source-derived for macOS/Windows; obtain target-platform runtime evidence before their parity rows are verified.
2. Confirm platform-specific integration test execution (macOS and Windows) and add migration CI evidence at manager level; local enumeration proves test registration, not target results.
3. Reconcile README quick-reference omissions against the authoritative live keybindings surface. This is documentation drift, not evidence that the omitted actions are absent.
4. There is no implemented CLI `--help`; only the TUI keybindings modal. If a future parity spec adds CLI help, record that additive behavior without changing the existing unknown-argument fallthrough contract.
