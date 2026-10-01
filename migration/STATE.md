# Migration state

Updated: 2026-10-01 | Branch: main | HEAD: cc857b3 (F-001 verified; F-002 spec in review)
Phase: 3 Foundation | Current wave: 0 / F-002 spec pre-review | Gate: FAIL (150 feature rows remain open before implementation/evidence)

## Project coordinates
- TUI crate/bin/path: `ira` package at repository root; `src/main.rs` and `src/lib.rs`; Rust 2021.
- GPUI app crate/bin/path: `ira-desktop` at `desktop/`; `desktop/src/main.rs`; Rust 2024.
- Core crate: `ira-core` at `crates/core`, local path dependency from root TUI and desktop; UI-neutral and currently empty.
- GPUI: crates.io `gpui = 0.2.2`, locked in `desktop/Cargo.lock`; verify APIs against this exact source and lock.
- Build/test commands: `cargo build --locked` passed; `cargo build --manifest-path desktop/Cargo.toml --locked` passed; `TMPDIR="$PWD/target/tmp" cargo test --locked` passed all tests. Root fmt/clippy still show pre-existing TUI and user-example findings.
- Oracle: tag `tui-oracle-baseline` -> `1cad4ce`; tag pushed. Do not edit TUI behavior without baseline characterization and an approved decision.
- Desktop CI: `.github/workflows/desktop-build.yml` is artifact compile/package only, not migration parity CI. No macOS/Windows parity run evidence yet.

## Counts
Rows=151; 144 NOT_STARTED, 1 VERIFIED, 6 BLOCKED; 99 inventory entries; 418 canonical surface items (literal comma key alias normalized to `key:normal:comma`); 418 surface-owner rows; blind completion-audit clean rounds=0.

## In flight
| Agent | Role | Row(s) | Branch/worktree | Launched | Expect |
|---|---|---|---|---|---|
| architecture_plan_review | spec pre-review | F-002 | shared report only | 2026-10-01 | harness architecture/write-set recommendation |
| platform_plan_review | spec pre-review | F-002 | shared report only | 2026-10-01 | PTY/ConPTY and cross-platform execution strategy |

## Blockers
- The fast gate reports 151 violations, all because feature rows remain open; surface presence is complete but does not prove semantic equivalence. The surface-presence check has no uncovered items; presence is not equivalent to semantically correct ownership.
- The 151-row matrix includes behavior-sized splits for input contexts/dialogs and a `migration/surface-ownership.tsv` ledger. Advisor re-review conditions were reconciled in D-0010: F-034 is split into F-034/F-151, F-013 now follows F-002, and F-014 excludes terminal overlay code.
- F-001 is VERIFIED at e346263 after 7 boundary tests, 278 root tests, both local locked builds, EQUIVALENT logic review, resolved adversarial GAPs, and macOS/Windows desktop CI run 36937979352. No GPUI window was launched on those runners; this row makes no native runtime claim. F-002 spec is drafted and undergoing architecture/platform pre-review; no F-002 code has started. Migration-wide CI (F-150) and native-window smoke evidence remain outstanding.
- Root-wide `cargo fmt --all -- --check` and strict workspace clippy have pre-existing findings from the frozen TUI and an untracked user example. Record a scoped/legacy-baseline policy before changing TUI code or claiming gate success.
- Rows F-068..F-070, F-072..F-073 and F-088 are BLOCKED until terminal-host/OS behavior gets a signed parity mapping; no waiver/equivalence has UX, logic, and adversarial sign-offs yet. F-150 now tracks the missing migration CI.

## Next 5 actions
1. Reconcile F-002 architecture/platform recommendations into its scenario and runner contract.
2. Launch adversarial red-test preparation for the approved F-002 scope.
3. Implement the harness only after red tests and a conflict-free integrator write-set are set.
4. Verify the harness replays the frozen oracle and reports deterministic diffs across supported runners.
5. Continue Wave-0 state, builds, tests, and CI in small pushed commits.
