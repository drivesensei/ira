# Advisory: revised migration plan by platform-advisor round 4

Scope reviewed: current `migration/PARITY_MATRIX.md`, `migration/surface-ownership.tsv`, `migration/DECISIONS.md`, `migration/waves/GRAPH.md`, and `WAVE-0.md` through `WAVE-3.md`.

## Verdict: PROCEED to F-001 spec and red-test preparation

The latest revision resolves the platform-plan blockers from round 3 sufficiently for preparation of the narrowly scoped core/GPUI boundary. Terminal-host rows F-068–F-070/F-072/F-073 and Linux mount/eject F-088 are explicitly BLOCKED and held outside active implementation batches pending equivalent-mapping signoffs. F-150 provides an owned migration-CI row. The Wave 0 dependency order is substantially corrected, and F-001's write-set is now minimal desktop wiring rather than all of `desktop/**`.

This is clearance to prepare and review the F-001 spec/tests, not approval to implement unrelated filesystem/platform behavior. Preserve `Path`/`OsString` internally and do not decide legacy path serialization in F-001; D-0003 correctly leaves encoding/round-trip policy for the filesystem/persistence contracts. Before those dependent features start, specify macOS case/normalization and Windows drive/UNC/Unicode/long-path, locking/ACL, and link behavior with target-specific tests.

## Residual recommendations

### R1 [should] Align the F-013 dependency edge and batch
The matrix and WAVE-0 say F-013 depends on F-002, but `GRAPH.md` only shows F-001 -> F-013. Also WAVE-0 puts F-002 and F-013 together in batch B even though the matrix records F-002 as a prerequisite of F-013. Either add the graph edge and put F-013 after F-002, or remove the dependency if the performance baseline can be captured independently. This does not block F-001 spec preparation.

### R2 [should] Keep F-014 limited to platform contracts
F-014's TUI refs still say `src/services/overlay/**`; those files implement terminal-host image overlays that D-0005 and the blocked rows explicitly exclude from GPUI implementation. Point the row/spec to the actual platform service contracts and process/path adapters, so an F-014 implementation cannot accidentally pull in terminal-window geometry.

### R3 [should] Preserve semantic verification despite ledger consistency
The surface ledger matches the matrix exactly: 418 unique surfaces, no missing/extra entries, and no owner-row mismatches. This is a strong structural check, not proof of platform parity. Retain per-OS tests/evidence for F-071 Windows volume labels, F-120 native chooser, F-122 open/reveal/clipboard, F-115 lifecycle/reopen, and F-118 text/IME. The current CI workflow remains artifact-only until F-150 lands; do not use its existing build artifacts as migration evidence.

## Platform blockers before implementation

None specific to F-001 spec/red-test preparation. The unresolved path-encoding contract blocks filesystem/config/transfer feature specs that persist or round-trip paths, not a minimal crate boundary. Do not unblock F-068–F-070/F-072/F-073 or F-088 for implementation without the matrix-required row-specific equivalent mapping and UX, logic, and adversarial signoffs. macOS and Windows native runtime checks remain required before any OS-dependent row is VERIFIED.

# Advisory: F-001 spec pre-review by platform-advisor

Scope reviewed: `migration/specs/F-001.md`, F-001 row in `migration/PARITY_MATRIX.md`, Wave 0 membership, and D-0010 in `migration/DECISIONS.md`.

## Verdict: APPROVE WITH MINOR CLARIFICATION

No macOS/Windows portability blocker to F-001. The separate root and desktop manifests/lockfiles are correctly preserved; a local path dependency on `ira-core` does not require a shared workspace or lockfile. The boundary is appropriately structural, leaves feature behavior and path serialization to later rows, and keeps the existing GPUI launch/reopen path and button behavior. D-0010 correctly limits approval to spec/red-test preparation and requires architecture/platform/UX review plus committed adversarial red checks before implementation.

### R1 [should] Make supported-target build evidence explicit in acceptance
S2/S4/S5 currently name locked builds and `cargo metadata`/path-dependency checks, but the explicit build commands run on whichever host executes them. State that F-150 must run locked builds for both manifests on the supported Linux, macOS, and Windows runners (including macOS arm64 and Windows MSVC), and retain separate lockfiles in each runner. This is not a reason to block F-001 prep; cross-platform runs can be supplied by F-150 before Wave 0 exit.

### R2 [should] Verify the core boundary at source as well as manifest level
S3's proposed check rejects direct GPUI/Ratatui/Crossterm dependencies, but the invariant also excludes imports of their types. Have the red boundary checks inspect `crates/core/**` for host-UI imports/dependencies and validate each app depends only on the core package path. Keep the root TUI's existing Ratatui/Crossterm dependencies valid; the restriction applies only inside `ira-core`.

Platform-specific paths, filesystem APIs, config roots, process launch, and native behavior are explicitly out of F-001 scope. D-0003's typed-path and serialization decisions remain prerequisites for their later owning rows, not for this crate skeleton.

# Advisory: F-002 cross-platform harness pre-review by platform-advisor

Scope reviewed: `migration/specs/F-002.md`, F-002/F-150 matrix rows, current Wave 0, existing artifact workflow, and the tagged-oracle requirement.

## Verdict: CHANGES REQUIRED before red tests

The spec correctly recognizes platform-specific input encoding, explicit isolation, failure cleanup, and that stored captures are not live replay. It leaves the core platform contract unresolved, however; implementers cannot write portable red tests until the PTY, supported runtime matrix, and baseline artifact lifecycle are fixed.

### R1 [must] Select one cross-platform PTY implementation and define live execution on all targets
Use a pinned `portable-pty` release (current documented release 0.9.0) behind a small harness adapter: Unix PTY on Linux/macOS and ConPTY on Windows. Its API exposes native PTY selection, child wait/termination, resize and master I/O, and its package is small (~114 kB source) with MIT license; it is part of WezTerm. It requires no tmux, shell framework, Python, external terminal application, or separately installed PTY DLL for this harness. The [crate docs](https://docs.rs/portable-pty/0.9.0/portable_pty/) and [published manifest/license](https://docs.rs/crate/portable-pty/0.9.0/source/Cargo.toml) document the cross-platform API and MIT terms. Audit the resolved dependency closure in the lockfile before merge.

F-150 must run actual tagged-oracle scenarios on `ubuntu-latest`, `macos-14`, and `windows-latest` (MSVC). Do not label Windows schema/golden-only checks as live behavior coverage. ConPTY's `CreatePseudoConsole` requires Windows 10 version 1809 or later ([Microsoft API requirements](https://learn.microsoft.com/en-us/windows/console/createpseudoconsole)); state this as the harness live-test minimum. Keep app OS support policy separate. If a runner cannot execute a live scenario, its report must explicitly say capture-only/unavailable and the owning parity row cannot claim live OS evidence.

### R2 [must] Define a platform-aware input encoding and scenario support contract
Keep one scenario schema, but encode logical event intent separately from adapter bytes/VT sequences and terminal resize events. The Unix PTY and ConPTY adapters must preserve the exact ordered key/input sequence presented to the frozen TUI. Each scenario must declare `live` support per OS or a concrete platform profile; unsupported live input is an explicit test result, never a silent skip. Only create OS-specific expectations where the frozen oracle demonstrably differs. Include ordinary character, Enter/Esc/arrows, resize and EOF/quit coverage on each native runner so ConPTY behavior is actually exercised.

### R3 [must] Build only the pinned baseline, reproducibly, with a cache that cannot select a different binary
In CI fetch the exact `tui-oracle-baseline` commit (full-history checkout or explicit tag fetch), verify its peeled SHA against `1cad4ce43cc72d52d4cc4eef920e0da22cb69568`, and build it in a detached clean checkout/archive using that commit's root `Cargo.toml` and `Cargo.lock`. Never fall back to `target/debug/ira` from the current checkout. Build once per OS/target/toolchain and reuse the exact artifact for all scenarios in that job. Cache key must include baseline SHA, OS, target triple, Rust toolchain/compiler version, and baseline lockfile hash; store/verify an artifact digest, rebuild on a cache miss, and fail if the exact baseline cannot be built or verified. `actions/checkout` default shallow history is insufficient without an explicit fetch of the baseline tag.

### R4 [must] Make timeout, child cleanup and environment isolation testable invariants
Apply a per-scenario deadline independent of test-suite timeout. On timeout or assertion error, terminate the PTY child, wait/reap it with a bounded cleanup deadline, close PTY handles, join/drain the output reader, and remove the unique fixture even if cleanup itself reports failure; report both primary and cleanup errors. Do not use sleeps: drive readiness from PTY output or a bounded poll/deadline. Spawn the exact executable with an argv array (no shell) and explicit cwd.

Start from cleared env and set only documented values. On Unix isolate `HOME`, XDG config/cache/data roots, `TMPDIR`, `TERM` and locale. On Windows isolate `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `TEMP`/`TMP`, and set the minimum OS variables required to launch the child (`SystemRoot`/`WINDIR`, `PATH`, and where needed `HOMEDRIVE`/`HOMEPATH`); preserve no inherited IRA or terminal-host settings. Create all profile dirs under the per-run temp root. Test that poisoned parent `IRA_*`, `TERM_PROGRAM`, `WT_*`, HOME/XDG/APPDATA variables cannot escape fixture/config roots.

### R5 [should] Separate deterministic harness tests from live PTY tests in CI, without false green claims
Run schema, fixture, normalization, diagnostics, and mutation tests on all three OSes. Run the same declared live scenarios against the baseline per OS on all three native runners. Keep PTY-dependent tests as required jobs, not `#[ignore]` or OS `cfg` exclusions. If the GPUI native window is unavailable in hosted CI, record that as a separate F-150 UI smoke limitation; it does not prevent terminal oracle replay and must not be conflated with it. Use each runner's own temporary storage and target cache; do not share PTY sessions or mutable fixtures.

No license/tool blocker is identified for the recommended PTY crate. The exact Rust dependency closure and `portable-pty` 0.9.0 MSVC build/ConPTY replay still need to be confirmed in F-150 before F-002 can be marked verified.

# Advisory: F-002 final cross-platform pre-review by platform-advisor

Scope reviewed: revised `migration/specs/F-002.md`, D-0011, F-002/F-150 matrix rows, Wave 0 and dependency graph.

## Verdict: APPROVE WITH TWO SPEC CLARIFICATIONS — red-test preparation may begin

The revised spec adopts the required native Unix PTY/Windows ConPTY strategy, all-three-OS live replay, exact frozen-baseline SHA and detached build, OS/target/toolchain/lockfile cache key with digest check and no current-tree fallback, cleared per-OS environment, absolute deadlines, child reaping/handle and output-reader cleanup, fixture cleanup, and Windows 10 1809 harness minimum. D-0011 records the same policy. F-150 owns CI/live-replay and license-closure verification; it does not claim cached build artifacts or stored goldens alone as live evidence. No unacceptable license or external tool requirement is apparent for the proposed `portable-pty` dependency.

The remaining items are spec precision, not a reason to delay adversarial red-test authoring. Ensure the red tests cover these requirements before implementation begins.

### R1 [must] Constrain scenario environment overrides so they cannot defeat isolation
The `Config influence` paragraph says each scenario supplies explicit environment overrides, while S9 says to start from a cleared environment and set only the OS minimum allowlist plus isolated roots. State that scenario overrides are separately allowlisted; harness-owned variables (`HOME`, XDG roots, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `TEMP`/`TMP`, and `TMPDIR`) cannot be redirected outside the run root. Permit IRA feature-specific env values only when their path targets are contained under that run root. The poisoned-parent test alone does not cover a trace that explicitly overrides a protected path.

### R2 [should] Specify detached baseline worktree cleanup
S7 requires a detached baseline worktree and cache, while S8 explicitly guarantees only PTY handles, readers and scenario fixture cleanup. Define that the temporary baseline checkout/worktree is removed on normal completion and failed setup/build, with Windows handle-safe cleanup; cache entries remain separately keyed/reusable. CI runner destruction reduces leakage but does not define local harness behavior. Add a cleanup failure diagnostic/test if practical.

Other reviewed details are adequate. Keep F-150's actual live tagged-oracle replay required on Linux, macOS 14, and Windows MSVC; if any live runner is unavailable, report it as unavailable rather than treating captures/schema tests as equivalent. The Windows 1809 minimum applies only to this ConPTY harness path and must remain separate from IRA's product support policy.
