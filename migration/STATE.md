# Migration state

Updated: 2026-10-01 | Branch: main | HEAD: 54dd6b1 (working migration plan and F-001 spec not committed)
Phase: 3 Foundation | Current wave: 0 / F-001 red-test preparation | Gate: FAIL (151 feature rows remain open before implementation/evidence)

## Project coordinates
- TUI crate/bin/path: `ira` package at repository root; `src/main.rs` and `src/lib.rs`; Rust 2021.
- GPUI app crate/bin/path: `ira-desktop` at `desktop/`; `desktop/src/main.rs`; Rust 2024.
- Core crate: none yet; extraction is foundation F-001 and must keep UI-free behavior separate from GPUI and TUI dependencies.
- GPUI: crates.io `gpui = 0.2.2`, locked in `desktop/Cargo.lock`; verify APIs against this exact source and lock.
- Build/test commands: `cargo build --locked` passed; `cargo build --manifest-path desktop/Cargo.toml --locked` passed; `TMPDIR="$PWD/target/tmp" cargo test --locked` passed all tests. Root fmt/clippy still show pre-existing TUI and user-example findings.
- Oracle: tag `tui-oracle-baseline` -> `1cad4ce`; tag pushed. Do not edit TUI behavior without baseline characterization and an approved decision.
- Desktop CI: `.github/workflows/desktop-build.yml` is artifact compile/package only, not migration parity CI. No macOS/Windows parity run evidence yet.

## Counts
Rows=151; 145 NOT_STARTED, 6 BLOCKED; 99 inventory entries; 418 canonical surface items (literal comma key alias normalized to `key:normal:comma`); 418 surface-owner rows; blind completion-audit clean rounds=0.

## In flight
| Agent | Role | Row(s) | Branch/worktree | Launched | Expect |
|---|---|---|---|---|---|
| adversarial_F001 | red-test reviewer | F-001 | shared tree | 2026-10-01 | failing boundary checks and GAP notes committed |

## Blockers
- The fast gate reports 151 violations, all because feature rows remain open; surface presence is complete but does not prove semantic equivalence. The surface-presence check has no uncovered items; presence is not equivalent to semantically correct ownership.
- The 151-row matrix includes behavior-sized splits for input contexts/dialogs and a `migration/surface-ownership.tsv` ledger. Advisor re-review conditions were reconciled in D-0010: F-034 is split into F-034/F-151, F-013 now follows F-002, and F-014 excludes terminal overlay code.
- F-001 spec is approved by architecture, UX, and platform; adversarial boundary red tests remain pending. Other foundation implementation, migration CI, and native OS smoke evidence have not started. No feature source code changed; F-001 spec preparation is now active.
- Root-wide `cargo fmt --all -- --check` and strict workspace clippy have pre-existing findings from the frozen TUI and an untracked user example. Record a scoped/legacy-baseline policy before changing TUI code or claiming gate success.
- Rows F-068..F-070, F-072..F-073 and F-088 are BLOCKED until terminal-host/OS behavior gets a signed parity mapping; no waiver/equivalence has UX, logic, and adversarial sign-offs yet. F-150 now tracks the missing migration CI.

## Next 5 actions
1. Collect F-001 adversarial red boundary tests and GAP notes; verify they fail against the current tree.
2. Reconcile the adversarial tests to F-001 spec clauses and update the exact test map.
3. Start F-001 implementation only after executable red tests are committed; preserve root TUI and independent lockfiles.
4. Complete developer implementation and both logic/adversarial review loops inside the F-001 write-set.
5. Run dual-manifest builds and required root tests; update evidence/status and the gate before F-002.
