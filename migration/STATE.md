# Migration state

Updated: 2026-10-01 | Branch: main | HEAD: 5a99091
Phase: 1 Deep discovery | Current wave: none | Gate: FAIL (discovery and matrix are incomplete)

## Project coordinates
- TUI crate/bin/path: `ira` package at repository root; `src/main.rs` and `src/lib.rs`; Rust 2021.
- GPUI app crate/bin/path: `ira-desktop` at `desktop/`; `desktop/src/main.rs`; Rust 2024.
- Core crate: none yet; the TUI library currently couples Ratatui types into its application and feature logic.
- gpui dependency: crates.io `gpui = 0.2.2`, locked in `desktop/Cargo.lock`.
- Build/test commands: `cargo build --locked` passed; `cargo build --manifest-path desktop/Cargo.toml --locked` passed; `TMPDIR="$PWD/target/tmp" cargo test --locked` passed (all tests). `/tmp` is a constrained tmpfs; default-temp runs previously hit resource pressure.
- Oracle baseline tag: `tui-oracle-baseline` -> `1cad4ce`; tag pushed to origin. Baseline binary path: `target/debug/ira` built from this commit. No TUI source edits permitted.
- CI: `.github/workflows/desktop-build.yml` builds packaged macOS arm64 and Windows x86_64 app binaries; repo-wide migration CI not yet verified or present.

## Counts
Rows=0; 99 inventory entries; 419 unique surface items. Discovery reconciliation and matrix construction remain. Blind completion-audit clean rounds=0.

## In flight
| Agent | Role | Row(s) | Branch/worktree | Launched | Expect |
|---|---|---|---|---|---|
| - | - | - | - | - | No subagents in flight; discovery and blind reports await manager reconciliation |

## Blockers
- Fast parity gate currently finds zero rows; this is expected until discovery and planning produce the matrix.
- Root-wide `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` expose pre-existing TUI findings, including an untracked user example. The tagged TUI baseline remains unchanged; gate scope needs an explicit migration decision before implementation.
- Discovery inventories record 99 behavior entries across nine slices; all 42 `src/**/*.rs` files have an explicit inventory owner or module-barrel classification. Three blind discovery cross-checks found no material source-derived contradiction; runtime evidence strength varies and open risks remain documented.
- The merged `migration/oracle/surface.txt` contains 419 unique items. The fast gate therefore currently reports 420 violations: no matrix rows plus every surface item uncovered. Manager must reconcile overlaps and map all items into appropriately sized feature rows before Phase 2 is complete.

## Next 5 actions
1. Reconcile all nine inventories, verify capture links/baseline commit ids, and resolve overlaps/discrepancies from the blind reports.
2. Close high-risk runtime gaps safely (notably delete/cancel, recursive merge, dialogs, terminal size and platform-only behavior); record any unavailable host checks.
3. Map the 419 surface items into small, independently verifiable matrix rows and define their dependencies.
4. Reconcile advisor recommendations with complete discovery and record adopt/adapt/reject decisions.
5. Build and review the parity matrix, common-ground foundation, dependencies, and conflict-free feature waves before implementation.
