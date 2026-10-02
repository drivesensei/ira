# Migration state

Updated: 2026-10-01 | Branch: main | HEAD: dc2ab8b (F-002 redesign red-test gates recorded)
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
| f002_capture_paste_red | adversarial red-test author | F-002 | review/F-002-capture-paste-red | 2026-10-01 | write S13 byte-safe bundle and S14 platform-specific paste red tests; no production changes |

## Blockers
- Fast gate on this checkout reports 236 violations: 150 unverified feature rows plus 86 open GAP annotations/ignored gap tests. Surface presence is complete; that does not prove semantic equivalence.
- The 151-row matrix includes behavior-sized splits for input contexts/dialogs and a `migration/surface-ownership.tsv` ledger. Advisor re-review conditions were reconciled in D-0010: F-034 is split into F-034/F-151, F-013 now follows F-002, and F-014 excludes terminal overlay code.
- F-001 is VERIFIED at e346263 after 7 boundary tests, 278 root tests, both local locked builds, EQUIVALENT logic review, resolved adversarial GAPs, and macOS/Windows desktop CI run 36937979352. No GPUI window was launched on those runners; this row makes no native runtime claim. F-002's approved spec covers standalone `tools/ira-parity`, versioned TOML traces, explicit v1 input/observation contracts, observation-based readiness before input, TUI press-only phase support, portable-pty 0.9.0, exact-baseline detached builds/cache metadata, cleanup on every exit path, protected environment roots, and live Linux/macOS/Windows replay. Architecture and platform approved. Frozen-baseline startup, resize, and non-TTY flag behavior were observed; initial PTY key injection was inconclusive. Remediation commit `62db2ce` and qualified-tag fix `5ae8bf3` are pushed on `migrate/F-002-review-fixes`; manager verified 37 active and 13 ignored Linux tests after the fix, plus fmt/clippy/scope. Adversarial review resolved ADV-31/38/44/45/46/49; logic review resolved LOG-01..04 and added passing checks for repeated expectations and process readiness. G50 was independently re-verified by the reviewer against `5ae8bf3`: collision probe passed and reviewer commit `cd96d91` records `GAP-RESOLVED`; G27/29/30 remain open. Logic probes LOG-05 (plain payload fails bracketed paste) and LOG-06 (staged capture lacks observed output) fail against `5ae8bf3` and are being integrated with reviewer markers. Native macOS/Windows evidence remains F-150. F-002 is not integrated or verified. Architecture escalation returned RETHINK; manager adopted restart of remaining F-002 work from `5ae8bf3`, preserving existing fixes and amending the spec with owned PTY lifecycle/helper-child proofs (S12), typed actual-observation golden bundles (S13), and Crossterm bracketed-paste fidelity (S14); disposition is D-0013. Adversarial lifecycle-red commits `4a97ae7`/`46811ea` are integrated and pushed to `migrate/F-002-review-fixes` (branch head `46811ea`): real portable-pty helper-child tests pass 4/4; the `PtySession` contract fails to compile specifically because the extraction is absent. Manager independently reproduced both outcomes. The branch also contains reviewer-resolved G50 and LOG-01..04, plus open LOG-05/06 and G27/29/30. Custom scope check passes with only reviewer test/report files changed. A fresh adversarial author is writing red tests for S13 byte-safe bundles and S14 Unix/Windows paste behavior before implementation can start. F-150 is pre-reviewed by architecture/platform, and adversarial red tests are pushed at `review/F-150-adversarial` commit `c371f10`; manager confirmed 9/9 fail on main because the workflow is absent, as intended. F-150 remains NOT_STARTED until F-002 is integrated and the red tests are merged.
- Architecture and platform second-round reviews are recorded in `reports/F-002/architecture-spec-review-2.md` and `platform-spec-review-2.md`; their constraints have been reconciled in D-0014 and the F-002 spec. Shutdown is phase-specific with fixture retention on unjoined-reader failure; golden payload/path encoding is reversible and atomically staged; Unix paste rejects the exact closing marker and Windows requires native oracle evidence or explicit unsupported behavior. No feature implementation is cleared until lifecycle red tests exist.
- Root-wide `cargo fmt --all -- --check` and strict workspace clippy have pre-existing findings from the frozen TUI and an untracked user example. Record a scoped/legacy-baseline policy before changing TUI code or claiming gate success.
- Rows F-068..F-070, F-072..F-073 and F-088 are BLOCKED until terminal-host/OS behavior gets a signed parity mapping; no waiver/equivalence has UX, logic, and adversarial sign-offs yet. F-150 now tracks the missing migration CI.

## Next 5 actions
1. Keep the branch-level verified GAP transitions reviewer-owned; reconcile ADV marker states and do not close G27/29/30 without session-level proof.
2. Complete and integrate S13/S14 red tests with the lifecycle and LOG-05/06 probes; keep the missing-session compile failure red; then launch the replacement developer for the serialized redesign slices.
3. Implement and independently review the extracted PTY owner/teardown, typed golden bundle, and bracketed-paste behavior in serialized overlapping write sets.
4. Integrate F-002 after reviewer GAP transitions are closed; then merge F-150 red tests and implement its native CI/license workflow.
5. Advance remaining Wave-0 rows only after their dependency barriers and per-row evidence/review gates clear.
