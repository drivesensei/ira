# Inventory: tests, documentation, and orphan sweep
Explorer run: 2026-10-01 | TUI commit: `1cad4ce43cc72d52d4cc4eef920e0da22cb69568` (`tui-oracle-baseline^{}`) | Files claimed: `tests/*.rs`, `src/**` coverage reconciliation, `README.md`, `docs/**`, `desktop/README.md`, `Cargo.toml`

## Features

### INV-DOCS-001 CLI help and package entry contracts
- Trigger: launch `ira` with a CLI argument.
- Behavior: the program recognizes only `--version`, `-V`, and `--check-terminal`, scanning every argument after argv[0]. `--version`/`-V` takes precedence and prints `ira 0.1.21` plus newline to stdout, then returns 0; `--check-terminal` prints the terminal diagnostics and returns 0. There is no `--help` implementation or argument parser. Unknown options, including `--help`, fall through into ordinary terminal probing and start the normal TUI; arbitrary arguments are not rejected or treated as paths/subcommands.
- State read/written: version paths read/write none; check-terminal reads config/theme/font/probe inputs; ordinary startup loads normal app state.
- Source refs: `src/main.rs:11-47`; package name/version in `Cargo.toml:1-3`.
- Runtime evidence: `target/debug/ira --version` and `target/debug/ira -V` each printed exactly `ira 0.1.21`, exit 0. `--check-terminal` with isolated HOME/XDG_CONFIG_HOME printed version, truecolor, icon/source, Nerd glyph source, theme/source, image protocol/source, cell size and missing temp theme path; exit 0. A direct PTY launch with `--help` (120x40, isolated `HOME=/tmp/ira-discover-help-home`, `XDG_CONFIG_HOME=/tmp/ira-discover-help-config`) displayed the ordinary Drives/Common folders/Files UI and normal key hint bar, rather than help text; Ctrl+C exited 0. Output was inspected live and is not a persisted capture.
- Edge cases observed: `--version` takes precedence if both recognized flags are present (source and existing entry inventory); argument order is otherwise irrelevant. A no-TTY runtime error for unknown args was not repeated here.
- Error behavior: normal terminal setup errors propagate from `main`; exact text depends on OS/TTY.
- Config knobs: `IRA_IMAGES`, `IRA_ICONS`, `IRA_WT_SETTINGS`, `IRA_THUMBNAIL_CACHE_DIR`, terminal/font environment and theme configuration; see relevant feature inventories. CLI command does not parse these values as arguments.
- Platform notes: ordinary terminal probing/startup varies by OS; this slice's live behavior was Linux PTY only.
- Depends on: INV-ENTRY-001/002/003/005 in `entry-input.md`; theme/config and terminal-probe inventories.
- Confidence: high for Linux runtime and source dispatch; macOS/Windows runtime not exercised.

### INV-DOCS-002 Existing test suite and its evidence boundary
- Trigger: `cargo test` or `cargo test -- --list` at the frozen TUI baseline.
- Behavior: tests are split between 218 unit tests in `src/**` and 60 integration tests: 2 drive-refresh tests, 52 operations tests, and 6 sort tests. `cargo test --locked -- --list` completed successfully and enumerated those counts (278 total, 0 benchmarks, 0 doc tests). Coverage exercises state methods/services, view geometry/rendered text, filesystem and transfer operations, sorting/filtering, platform probe parsers and theme/config logic. Tests predominantly assert in-process model/service behavior; they do not constitute full TUI feature parity or macOS/Windows GUI/runtime smoke coverage.
- State read/written: many tests construct temporary paths below `std::env::temp_dir()`; a subset explicitly redirects app state paths. Test files use PID-derived temporary names, and operations tests generally remove their fixture at the end. One drive refresh test constructs `App::new` and invokes host drive enumeration in the background.
- Source refs: `tests/drives_refresh_test.rs:1-33`; `tests/operations_test.rs:1-2386`; `tests/sort_test.rs:1-297`; unit `#[test]` modules throughout `src/**` (218 tests as enumerated by Cargo).
- Runtime evidence: `cargo test --locked -- --list` exit 0; it lists test names without executing test bodies. The manager's phase-0 test run is recorded in `migration/STATE.md` / `migration/journal.md` and reports that the root suite passed with `TMPDIR="$PWD/target/tmp"` (the constrained `/tmp` environment had caused earlier transient failures).
- Edge cases observed: integration tests explicitly cover hidden files and persistence, collisions, cancellation, selection/filtering, nested creation/goto, race-safe create, batch responsiveness, async refresh and cursor behavior. Some tests are platform-gated (for example macOS Backspace behavior and Unix permission behavior); test discovery alone does not prove tests ran on other platforms.
- Error behavior: Cargo reports compiler/test warnings during `--list`; those are baseline hygiene findings, not test failures. `cargo test` is needed for assertion evidence; the list command alone is not a pass of test bodies.
- Config knobs: test behavior uses env/config inputs where dedicated tests provide snapshots or isolated paths; see configuration/theme inventories.
- Platform notes: only the current Linux host's test enumeration/run has been observed; Windows/macOS CI parity remains unproven by this slice.
- Depends on: all behavior inventories referenced by the individual test names; root test command and platform CI.
- Confidence: high for test count/layout and listed test names; medium for completeness of behavioral coverage because numerous feature edges have no direct tests.

### INV-DOCS-003 Documentation claims and known contract gaps
- Trigger: user reads installation, build, usage, feature, theme, or distribution documentation.
- Behavior: `README.md` is a user-facing overview/install/quick key reference; `docs/features.md` and `docs/full_features.md` give broader behavior descriptions; `docs/themes.md` documents presets, font/icon probing, image protocols, config paths; `docs/copy-move-plan.md` and `docs/distribution-plan.md` are explicitly design/rollout plans; `desktop/README.md` describes the separate GPUI starter. Documentation is not itself an additional runtime capability.
- State read/written: none.
- Source refs: `README.md:1-143`; `docs/features.md:1-160`; `docs/full_features.md:1-204`; `docs/themes.md:1-212`; `docs/copy-move-plan.md:1-211`; `docs/distribution-plan.md:1-86`; `desktop/README.md:1-33`.
- Runtime evidence: `README.md` quick-reference keys differ in completeness from the live `*` keybindings overlay (captured at `migration/oracle/captures/entry-input/keybindings.txt`): quick-reference omits implemented actions including `n`, `[`, `]`, `0`, `-`, `Ctrl+-`, `,`, `v`, `+`, and `*` help. The more complete `docs/full_features.md` covers several omitted entries. The `--help` behavior is runtime-checked above and demonstrates no CLI help screen exists.
- Edge cases observed: README's “Install” release and package instructions represent packaging state and may drift independently; `docs/distribution-plan.md` is dated 2026-09-03 and includes plans/claims that must not be construed as feature requirements. `docs/copy-move-plan.md` documents planned v1 conflict handling (“skip + report”), while current behavior is defined by `files-ops.md` and its overwrite-policy implementation/runtime evidence; resolve any discrepancy against the oracle rather than using the plan as specification.
- Error behavior: docs do not define runtime errors; feature inventories own exact messages.
- Config knobs: the README and themes/full-features pages mention `IRA_IMAGES`, `IRA_ICONS`, `IRA_THUMBNAIL_CACHE_DIR`, `IRA_WT_SETTINGS`, theme TOML, and persistent state. Exact names/defaults are owned by config/theme/preview inventories.
- Platform notes: claims about drive enumeration, file browser/terminal launch, image overlays, package availability, and font detection are OS-specific and require their feature inventory evidence.
- Depends on: entry/input, config/state, filesystem, file-ops, rendering, preview/external and platform inventories.
- Confidence: high for documentary content and observed `--help` mismatch; individual long-form platform claims require their owning inventories' runtime evidence.

## Unclaimed or uncertain
- See `migration/reports/discovery/orphan-sweep.md` for the module-by-module reconciliation. Seven concrete implementation files remain unclaimed by existing slice headers/inventories: `src/lib.rs`, `src/services/folders.rs`, `src/services/overlay/macos.rs`, `src/services/overlay/windows.rs`, `src/services/windows_drives_labels.rs`, `src/theme/font_probe.rs`, and `src/theme/wt_font.rs`. The five Rust module declaration files (`components/mod.rs`, `domain/mod.rs`, `services/mod.rs`, `theme/mod.rs`, `utils/mod.rs`) are scaffolding; the four whose source behavior is not itself a barrel are mapped to their owning slices. The platform overlay implementations were explicitly left uncertain in `preview-external.md`; they are not considered covered merely because their modules are declared.
- `docs/features.md` lists the first file panel as the only interactive list, while `docs/full_features.md` and live screens describe split panes. Treat older prose carefully; source/runtime inventory is authoritative.
- Many inline unit tests live inside production `.rs` files, contrary to the kit's preferred reviewer-annotation locations (`tests/` or sibling `tests.rs`). They are frozen baseline tests and must not be moved as part of this discovery slice.
