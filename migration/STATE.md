# Migration state

Updated: 2026-10-01 | Branch: main | HEAD: 1f2f5a3
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
Rows=0; discovery not yet complete. Blind audit clean rounds=0.

## In flight
| Agent | Role | Row(s) | Branch/worktree | Launched | Expect |
|---|---|---|---|---|---|
| discover_state_search | TUI explorer | discovery | shared checkout, disjoint inventory/oracle files | 2026-10-01 | state model and search/filter runtime inventory |
| discover_preview_external | TUI explorer | discovery | shared checkout, disjoint inventory/oracle files | 2026-10-01 | preview/viewers and external integration inventory |

## Blockers
- Fast parity gate currently finds zero rows; this is expected until discovery and planning produce the matrix.
- Root-wide `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` expose pre-existing TUI findings, including an untracked user example. The tagged TUI baseline remains unchanged; gate scope needs an explicit migration decision before implementation.
- Discovery is partial: 52 inventory entries are recorded across entry/input (18), config/state (12), file operations (13), and rendering/messages (9). State/search and preview/external explorers are active.
- No matrix rows or merged central surface items exist yet; filesystem services, tests/docs, platform slice details, and orphan sweep remain unclaimed.

## Next 5 actions
1. Verify completed entry/input, config/state, file-operation, and rendering/message inventories/captures; check all evidence paths and commit ids.
2. Complete state/search and preview/external, then cover filesystem services, tests/docs, platform risks, and orphan modules.
3. Enumerate all `src/**` modules, perform blind cross-checks, reconcile live behavior, and merge the full surface.
4. Reconcile advisor recommendations with complete discovery and record adopt/adapt/reject decisions.
5. Build and review the parity matrix, common-ground foundation, dependencies, and conflict-free feature waves before implementation.
