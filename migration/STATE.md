# Migration state

Updated: 2026-10-01 | Branch: main | HEAD: bcc69bc (F-002 reviews identified fixes before integration)
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
| f002_adversarial_review | review follow-up | F-002 | review/F-002-adversarial | 2026-10-01 | reopen G31/G38 and tighten tests for those contract gaps |
| f002_implementer | review fixes | F-002 | migrate/F-002-review-fixes | 2026-10-01 | fix 8 demonstrated logic/adversarial gaps; retain unproven lifecycle cases |

## Blockers
- The fast gate reports 151 violations, all because feature rows remain open; surface presence is complete but does not prove semantic equivalence. The surface-presence check has no uncovered items; presence is not equivalent to semantically correct ownership.
- The 151-row matrix includes behavior-sized splits for input contexts/dialogs and a `migration/surface-ownership.tsv` ledger. Advisor re-review conditions were reconciled in D-0010: F-034 is split into F-034/F-151, F-013 now follows F-002, and F-014 excludes terminal overlay code.
- F-001 is VERIFIED at e346263 after 7 boundary tests, 278 root tests, both local locked builds, EQUIVALENT logic review, resolved adversarial GAPs, and macOS/Windows desktop CI run 36937979352. No GPUI window was launched on those runners; this row makes no native runtime claim. F-002's approved spec covers standalone `tools/ira-parity`, versioned TOML traces, explicit v1 input/observation contracts, observation-based readiness before input, TUI press-only phase support, portable-pty 0.9.0, exact-baseline detached builds/cache metadata, cleanup on every exit path, protected environment roots, and live Linux/macOS/Windows replay. Architecture and platform approved. Frozen-baseline startup, resize, and non-TTY flag behavior were observed; initial PTY key injection was inconclusive. Developer commits `176d6de`, `151e121`, `7ee2d54`, `88dcf9c` on `migrate/F-002-parity-harness` pass 27 active Linux tests and 13 ignored tests when explicitly run; fmt/clippy pass. Independent reviewers found four logic divergences (observation comparisons, ANSI normalization, symlink containment side effect, missing-tag fetch) and four adversarial divergences (expected observations, platform profile, readiness kind, OSC normalization); all are being fixed on `migrate/F-002-review-fixes`. They also reopened weak readiness/golden tests and retained lifecycle gaps. Native macOS/Windows replay and dependency license audit remain unverified. F-002 is not integrated or verified.
- Root-wide `cargo fmt --all -- --check` and strict workspace clippy have pre-existing findings from the frozen TUI and an untracked user example. Record a scoped/legacy-baseline policy before changing TUI code or claiming gate success.
- Rows F-068..F-070, F-072..F-073 and F-088 are BLOCKED until terminal-host/OS behavior gets a signed parity mapping; no waiver/equivalence has UX, logic, and adversarial sign-offs yet. F-150 now tracks the missing migration CI.

## Next 5 actions
1. Fix the demonstrated logic and adversarial gaps on `migrate/F-002-review-fixes` without weakening tests.
2. Complete adversarial re-review; resolve/reopen GAPs with exact evidence.
3. Add native-child timeout and reader cleanup coverage or retain documented GAPs.
4. Start F-150 CI integration for live macOS/Windows replays and full dependency license audit.
5. Continue Wave-0 state, builds, tests, and CI in small pushed commits.
