# Migration state

Updated: 2026-10-01 | Branch: main | HEAD: 1cad4ce
Phase: 0 Orientation / 1 Discovery | Current wave: none | Gate: FAIL (baseline and migration state are incomplete)

## Project coordinates
- TUI crate/bin/path: `ira` package at repository root; `src/main.rs` and `src/lib.rs`; Rust 2021.
- GPUI app crate/bin/path: `ira-desktop` at `desktop/`; `desktop/src/main.rs`; Rust 2024.
- Core crate: none yet; the TUI library currently couples Ratatui types into its application and feature logic.
- gpui dependency: crates.io `gpui = 0.2.2`, locked in `desktop/Cargo.lock`.
- Build/test commands: `cargo build --locked` passed; `cargo build --manifest-path desktop/Cargo.toml --locked` passed; baseline `cargo test --locked` failed 3 `operations_test` cases (two transfer timing assertions and a temp-file quota error; cause not yet classified).
- Oracle baseline tag: `tui-oracle-baseline` -> `1cad4ce`; tag pushed to origin. Working tree will be used for captures only; no TUI source edits permitted.
- CI: `.github/workflows/desktop-build.yml` builds packaged macOS arm64 and Windows x86_64 app binaries; repo-wide migration CI not yet verified or present.

## Counts
Rows=0; discovery not yet complete. Blind audit clean rounds=0.

## In flight
| Agent | Role | Row(s) | Branch/worktree | Launched | Expect |
|---|---|---|---|---|---|
| discover_entry_input | TUI explorer | discovery | shared checkout, disjoint inventory/oracle files | 2026-10-01 | entry points + input system inventory and runtime captures |
| shell_architecture | Architecture advisor | Phase 0 | shared checkout, reports only | 2026-10-01 | shell audit + pinned GPUI notes |
| platform_risks | Platform advisor | Phase 0 | shared checkout, reports only | 2026-10-01 | macOS/Windows risk map and CI review |

## Blockers
- Full baseline test command had three failing transfer tests, one with `QuotaExceeded`; rerun sequentially after build contention clears to classify resource pressure versus existing regression.
- Fast parity gate currently fails because the feature matrix has no rows; expected until discovery and planning are complete.
- Discovery coverage and feature matrix are not yet established.

## Next 5 actions
1. Finish Phase 0 builds; rerun tests after concurrent builds finish and record results.
2. Complete orientation and verify a reproducible TUI command and runtime fixture flow.
3. Read explorer and advisor reports; map all TUI modules to discovery slices.
4. Launch disjoint TUI explorers and capture complete runtime surface/key/config/CLI evidence.
5. Build and review the parity matrix, common-ground foundation, dependencies, and conflict-free feature waves before implementation.
