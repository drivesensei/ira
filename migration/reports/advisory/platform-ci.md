# Advisory: platform CI by platform-advisor round 1

Scope reviewed: `.github/workflows/desktop-build.yml`, `.github/workflows/release.yml`, `desktop/Cargo.toml`, `desktop/README.md`, `.cargo/config.toml`, root `Cargo.toml`, and available Actions run history. Latest observed desktop-build runs: `36926800102` and `36919657605` completed successfully on 2026-10-01 (`gh run list --workflow desktop-build.yml --limit 5`). This establishes build/package success only, not runtime parity.

Summary verdict: **PROCEED WITH CHANGES**. Current artifact CI is useful for manual shell testing but is not migration CI. It does not build/test the TUI on its macOS/Windows jobs, exercise GPUI UI, or run formatting/lint/tests. Build targets are macOS arm64 and Windows x86_64 only.

## Current pipeline facts

- `desktop-build.yml` triggers on manual dispatch and changes to itself or `desktop/**`; macOS runner is `macos-14`, Windows runner is `windows-latest`.
- macOS runs `cargo build --manifest-path desktop/Cargo.toml --release --locked`, creates `IRA.app`, ad-hoc signs and verifies it, then uploads a 14-day arm64 tarball artifact. No notarization or Intel/macOS x86_64 artifact is built.
- Windows builds a release GUI-subsystem `.exe` with static CRT and uploads the standalone x86_64 binary for 14 days.
- There are no `cargo test`, `cargo fmt --check`, or clippy steps in this workflow. No visual smoke test is present. There is no dedicated macOS/Windows parity workflow yet.
- `desktop/Cargo.toml` is separate from root `Cargo.toml`; it currently depends on GPUI only. Thus green desktop jobs do not compile the TUI crate or validate its platform-sensitive code.
- `.github/workflows/release.yml` is a separate tag-triggered release workflow for the TUI and includes a GitHub Release job. The desktop workflow itself does not publish releases. Confirm with the manager whether the old TUI release workflow remains in the requested migration policy; user-facing request in prior history said test binaries only/no releases.
- macOS artifact is only ad-hoc signed. `desktop/README.md` already notes Gatekeeper approval. Clean-machine launch and reopen behavior must be manually tested; `codesign --verify` does not prove notarization or Gatekeeper acceptance.

## Ranked recommendations

### R1 [must] Add migration CI for each supported desktop target and the TUI/core

Matrix: `macos-14` arm64, `windows-latest` x86_64 MSVC; include `ubuntu-latest` while root crate continues targeting Linux and as a fast core test lane. If shipping Intel macOS is an explicit deliverable, add `macos-13`/x86_64 or cross-target build and smoke test; current download artifact is arm64 only.

Each OS job should run locked `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`, plus build both TUI and desktop manifests (or workspace when unified). Add GUI app bundle packaging on macOS and GUI subsystem executable packaging on Windows. Current TUI release matrix proves its own build per target only if its historical run is green; it does not prove migration behavior.

### R2 [must] Add runtime smoke coverage separate from compile jobs

Use headless GPUI unit/component tests when supported by pinned GPUI. For tests that need display/GPU, either a real macOS/Windows runner GUI session or a dedicated display-backed job with an explicit `env: needs display` ignore and CI job that actually runs it. Add smoke assertions for: window opens, focus/action routing, close/minimize/reopen, native file open, app config path, and a basic listing/transfer fixture. A successful cargo build is not evidence that an app window appears or restores.

### R3 [must] Make filesystem fixtures deterministic across runners

- Add `.gitattributes` for text golden files with explicit LF and binary fixtures `-text`; do not normalize byte-exact session/config fixtures unexpectedly.
- Avoid relying on symlinks unless fixture setup checks capability and has an explicit alternate path; Windows symlink creation can depend on privilege/Developer Mode. Exercise junction/reparse points separately.
- Include temporary-directory tests for spaces, Unicode, non-ASCII, case-equivalent names, long paths, read-only/ACL denied files, cross-volume transfer, and locked files. Real removable volumes are not reliable hosted-runner fixtures; inject drive-list data or use focused platform adapter tests.
- Avoid asserting directory enumeration order; sort test inputs/expected outputs explicitly. macOS APFS and Windows filesystem case behavior differs from Linux.

### R4 [should] Keep artifact distribution and migration verification visibly distinct

The desktop artifact workflow currently uploads test binaries, and the release workflow creates TUI GitHub releases from tags. Keep user-test artifacts out of release/publish steps unless explicitly chosen; add a workflow summary that labels artifact OS/architecture, commit, signing status, and manual launch limitations. Record CI run IDs against parity rows only for what they actually cover.

### R5 [should] Prevent a green check from hiding desktop build coverage gaps

`desktop-build.yml` path filter does not run when root `src/**`, root `Cargo.toml`/`Cargo.lock`, docs/specs, or migration scripts change. Expand trigger paths to all code/dependency inputs it builds, or add a dedicated required workflow with normal PR/push triggers. Keep caches scoped per target/workspace (`desktop -> target` now is reasonable for artifacts); avoid shared target collisions across host/target jobs.

## Flake and runner notes

- GPUI can be sensitive to GPU/driver/window-server availability; separate deterministic core tests from native-window smoke tests. Do not mark visual-only work verified through compile-only CI.
- Hosted macOS runner version and GPU backend can change; pin/test the supported runner family and collect logs/screenshots on failure. Test app bundle in a clean user session, not only CI's build account.
- Windows path length and checkout path depth can make Cargo builds fail before product behavior is reached; enable long paths for fixture coverage and keep checkout/build paths short. Use MSVC target as current workflow does.
- Avoid testing clipboard through external command tools in a no-console desktop process. Use GPUI/platform clipboard adapters and fake/capture tests where possible.

## Not verified

- No green migration CI exists yet; successful run IDs above are for build artifacts only.
- This review did not inspect artifact downloads or launch them on actual macOS/Windows hosts. Existing desktop README asks for manual test.
- No `.gitattributes` file currently exists. No parity inventory or `STATE.md` currently records the required matrix or CI evidence.
