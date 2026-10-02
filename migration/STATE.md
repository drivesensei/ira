# Migration state

Updated: 2026-10-01 | Branch: main | HEAD: 84e04ff (F-002 remediation pushed; independent re-review active)
Phase: 3 Foundation | Current wave: 0 / F-002 review fixes | Gate: FAIL (150 feature rows remain open before implementation/evidence)

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
| f002_adversarial_review | adversarial re-review | F-002 | review/F-002-adversarial | 2026-10-01 | verify remediation commit 62db2ce; reassess lifecycle and baseline ref gaps |
| f002_logic_review | logic re-review | F-002 | review/F-002-logic | 2026-10-01 | verify LOG-01..04 against remediation commit 62db2ce |

## Blockers
- The fast gate reports 151 violations, all because feature rows remain open; surface presence is complete but does not prove semantic equivalence. The surface-presence check has no uncovered items; presence is not equivalent to semantically correct ownership.
- The 151-row matrix includes behavior-sized splits for input contexts/dialogs and a `migration/surface-ownership.tsv` ledger. Advisor re-review conditions were reconciled in D-0010: F-034 is split into F-034/F-151, F-013 now follows F-002, and F-014 excludes terminal overlay code.
- F-001 is VERIFIED at e346263 after 7 boundary tests, 278 root tests, both local locked builds, EQUIVALENT logic review, resolved adversarial GAPs, and macOS/Windows desktop CI run 36937979352. No GPUI window was launched on those runners; this row makes no native runtime claim. F-002's approved spec covers standalone `tools/ira-parity`, versioned TOML traces, explicit v1 input/observation contracts, observation-based readiness before input, TUI press-only phase support, portable-pty 0.9.0, exact-baseline detached builds/cache metadata, cleanup on every exit path, protected environment roots, and live Linux/macOS/Windows replay. Architecture and platform approved. Frozen-baseline startup, resize, and non-TTY flag behavior were observed; initial PTY key injection was inconclusive. Remediation commit `62db2ce` is pushed on `migrate/F-002-review-fixes`. Manager independently verified 5 adversarial, 27 active contract, 4 logic, and 13 explicitly ignored Linux tests; scoped fmt, clippy (`-D warnings`), diff and dirty-tree scope checks pass. G27/29/30 remain open pending real lifecycle evidence; the developer report also identifies incomplete golden output and paste protocol fidelity. Logic and adversarial reviewers are rechecking their findings. Native macOS/Windows replay and dependency license audit remain F-150 obligations. A possible ambiguous Git revision lookup (`refs/tags/<name>` validated, `<name>^{commit}` resolved) is flagged for review. F-002 is not integrated or verified.
- Root-wide `cargo fmt --all -- --check` and strict workspace clippy have pre-existing findings from the frozen TUI and an untracked user example. Record a scoped/legacy-baseline policy before changing TUI code or claiming gate success.
- Rows F-068..F-070, F-072..F-073 and F-088 are BLOCKED until terminal-host/OS behavior gets a signed parity mapping; no waiver/equivalence has UX, logic, and adversarial sign-offs yet. F-150 now tracks the missing migration CI.

## Next 5 actions
1. Complete adversarial and logic re-review of remediation commit 62db2ce; resolve/reopen GAPs with evidence.
2. Address any re-review findings, including Git ref qualification and repeated observation behavior.
3. Keep G27/29/30 open until native-child timeout and reader cleanup behavior has adequate proof.
4. Start F-150 CI integration for live macOS/Windows replays and full dependency license audit.
5. Continue Wave-0 features only after F-002 review/integration barriers clear.
