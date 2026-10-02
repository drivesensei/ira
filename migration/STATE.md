# Migration state

Updated: 2026-10-01 | Branch: main | HEAD: 082e85e (F-002 re-review; F-150 red tests prepared)
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
| f002_adversarial_review | G50 verification | F-002 | review/F-002-adversarial | 2026-10-01 | independently verify qualified-tag fix in 5ae8bf3 |
| f002_arch_redesign | escalation review | F-002 | reports-only | 2026-10-01 | assess third-cycle lifecycle/fidelity findings and recommend redesign |

## Blockers
- The fast gate reports 151 violations, all because feature rows remain open; surface presence is complete but does not prove semantic equivalence. The surface-presence check has no uncovered items; presence is not equivalent to semantically correct ownership.
- The 151-row matrix includes behavior-sized splits for input contexts/dialogs and a `migration/surface-ownership.tsv` ledger. Advisor re-review conditions were reconciled in D-0010: F-034 is split into F-034/F-151, F-013 now follows F-002, and F-014 excludes terminal overlay code.
- F-001 is VERIFIED at e346263 after 7 boundary tests, 278 root tests, both local locked builds, EQUIVALENT logic review, resolved adversarial GAPs, and macOS/Windows desktop CI run 36937979352. No GPUI window was launched on those runners; this row makes no native runtime claim. F-002's approved spec covers standalone `tools/ira-parity`, versioned TOML traces, explicit v1 input/observation contracts, observation-based readiness before input, TUI press-only phase support, portable-pty 0.9.0, exact-baseline detached builds/cache metadata, cleanup on every exit path, protected environment roots, and live Linux/macOS/Windows replay. Architecture and platform approved. Frozen-baseline startup, resize, and non-TTY flag behavior were observed; initial PTY key injection was inconclusive. Remediation commit `62db2ce` and qualified-tag fix `5ae8bf3` are pushed on `migrate/F-002-review-fixes`; manager verified 37 active and 13 ignored Linux tests after the fix, plus fmt/clippy/scope. Adversarial review resolved ADV-31/38/44/45/46/49; logic review resolved LOG-01..04 and added passing checks for repeated expectations and process readiness. G50 passed the namespace-collision probe locally and awaits adversarial re-verification. New logic probes LOG-05 (plain payload fails bracketed paste) and LOG-06 (staged capture lacks observed output) now fail against 5ae8bf3. G27/29/30 remain open pending real child/reader cleanup evidence. Native macOS/Windows evidence remains F-150. F-002 is not integrated or verified. Architecture redesign review is active after three review cycles. F-150 is pre-reviewed by architecture/platform, and adversarial red tests are pushed at `review/F-150-adversarial` commit `c371f10`; manager confirmed 9/9 fail on main because the workflow is absent, as intended. F-150 remains NOT_STARTED until F-002 is integrated and the red tests are merged.
- Root-wide `cargo fmt --all -- --check` and strict workspace clippy have pre-existing findings from the frozen TUI and an untracked user example. Record a scoped/legacy-baseline policy before changing TUI code or claiming gate success.
- Rows F-068..F-070, F-072..F-073 and F-088 are BLOCKED until terminal-host/OS behavior gets a signed parity mapping; no waiver/equivalence has UX, logic, and adversarial sign-offs yet. F-150 now tracks the missing migration CI.

## Next 5 actions
1. Obtain adversarial re-verification of G50 at `5ae8bf3`; keep G27/29/30 open until real lifecycle paths are proven.
2. Integrate logic-review red GAPs LOG-05/06 for paste fidelity and captured golden output; address the architecture redesign recommendation.
3. Integrate the reviewed F-002 harness and F-150 red CI tests, then implement the CI workflow.
4. Prove F-150 native replay, lockfile license audit, and desktop window smoke experiment.
5. Continue Wave-0 features only after their dependency barriers and per-row test/review gates clear.
