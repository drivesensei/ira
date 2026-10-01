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
