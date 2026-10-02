# Migration state

Updated: 2026-10-01 | Branch: main | HEAD: df91471 (F-002 reviewer transitions reconciled in feature branch)
Phase: 3 Foundation | Current wave: 0 / F-002 redesign | Gate: FAIL (150 feature rows remain open; 86 open GAP annotations)

## Project coordinates
- TUI crate/bin/path: `ira` package at repository root; `src/main.rs` and `src/lib.rs`; Rust 2021.
- GPUI app crate/bin/path: `ira-desktop` at `desktop/`; `desktop/src/main.rs`; Rust 2024.
- Core crate: `ira-core` at `crates/core`, local path dependency from root TUI and desktop; UI-neutral and currently empty.
- GPUI: crates.io `gpui = 0.2.2`, locked in `desktop/Cargo.lock`; verify APIs against this exact source and lock.
- Build/test commands: `cargo build --locked` passed; `cargo build --manifest-path desktop/Cargo.toml --locked` passed; `TMPDIR="$PWD/target/tmp" cargo test --locked` passed all tests. Root fmt/clippy still show pre-existing TUI and user-example findings.
- Oracle: tag `tui-oracle-baseline` -> `1cad4ce`; tag pushed. Do not edit TUI behavior without baseline characterization and an approved decision.
- Desktop CI: `.github/workflows/desktop-build.yml` is artifact compile/package only, not migration parity CI. No macOS/Windows parity run evidence yet.

## Counts
Rows=151; 143 NOT_STARTED, 1 RED_TESTS, 1 VERIFIED, 6 BLOCKED; 99 inventory entries; 418 canonical surface items (literal comma key alias normalized to `key:normal:comma`); 418 surface-owner rows; blind completion-audit clean rounds=0.

## In flight
| Agent | Role | Row(s) | Branch/worktree | Launched | Expect |
|---|---|---|---|---|---|
| f002_lifecycle_adv_review | adversarial-reviewer | F-002 S12 | review/F-002-lifecycle-adversarial @ d945cd9 | 2026-10-01 | verify ADV-29/30 and attack lifecycle teardown; native OS gaps remain open |
| f002_lifecycle_logic_review | logic-reviewer | F-002 S12 | review/F-002-lifecycle-logic @ d945cd9 | 2026-10-01 | compare owned session and runner against frozen oracle lifecycle semantics |

## Blockers
- Fast gate on this checkout reports 236 violations: 150 unverified feature rows plus 86 open GAP annotations/ignored gap tests. Surface presence is complete; that does not prove semantic equivalence.
- The 151-row matrix includes behavior-sized splits for input contexts/dialogs and a `migration/surface-ownership.tsv` ledger. Advisor re-review conditions were reconciled in D-0010: F-034 is split into F-034/F-151, F-013 now follows F-002, and F-014 excludes terminal overlay code.
- F-001 remains VERIFIED at e346263. F-002's D-0013/D-0014 redesign is in progress on `migrate/F-002-review-fixes` at `d945cd9`: the owned PTY session is wired into the oracle runner. Manager independently reproduced lifecycle 6/6, helper PTY 4/4, contract 29 passed/11 ignored, adversarial 6/6, and the tagged Linux replay; fmt and library clippy pass. ADV-29/30 are GAP-FIXED pending both reviewers. S13 capture and S14 paste remain intentionally red. Native macOS/Windows lifecycle, ConPTY close ordering, and ADV-40 remain unverified; F-150 still owns native CI and license evidence. Feature branch has other open ADV-06..09,12..13,15..16,24..28,34,37,40,42,51..58 and LOG-05/06. F-150 workflow red tests remain on `review/F-150-adversarial` at `c371f10`; 9/9 fail on main because the workflow is absent.
- Architecture and platform second-round reviews are recorded in `reports/F-002/architecture-spec-review-2.md` and `platform-spec-review-2.md`; their constraints have been reconciled in D-0014 and the F-002 spec. Shutdown is phase-specific with fixture retention on unjoined-reader failure; golden payload/path encoding is reversible and atomically staged; Unix paste rejects the exact closing marker and Windows requires native oracle evidence or explicit unsupported behavior. No feature implementation is cleared until lifecycle red tests exist.
- Root-wide `cargo fmt --all -- --check` and strict workspace clippy have pre-existing findings from the frozen TUI and an untracked user example. Record a scoped/legacy-baseline policy before changing TUI code or claiming gate success.
- Rows F-068..F-070, F-072..F-073 and F-088 are BLOCKED until terminal-host/OS behavior gets a signed parity mapping; no waiver/equivalence has UX, logic, and adversarial sign-offs yet. F-150 now tracks the missing migration CI.

## Next 5 actions
1. Keep the branch-level verified GAP transitions reviewer-owned; reconcile ADV marker states and do not close G27/29/30 without session-level proof.
2. Independently verify S12 developer work; launch adversarial and logic reviewers immediately after its commit.
3. After S12 reviewer gaps close, assign the typed golden bundle (S13), then bracketed-paste fidelity (S14), one slice at a time.
4. Integrate F-002 after reviewer GAP transitions are closed; then merge F-150 red tests and implement its native CI/license workflow.
5. Advance remaining Wave-0 rows only after their dependency barriers and per-row evidence/review gates clear.
