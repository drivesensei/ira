# F-002 developer report (partial)

Status: PARTIAL; do not mark F-002 verified. Implementation started on `migrate/F-002-parity-harness`.

## Changes

- Added an isolated `tools/ira-parity` package and lockfile with trace schema, environment policy, normalizer, golden staging, baseline resolver, PTY runner, and scripted target modules.
- Kept production changes within `tools/ira-parity/**`; made one API-shape-only change in the accepted test, changing `KeyCode::Character('q')` to the schema's `{ character: 'q' }` shape. No behavior assertion or GAP note changed.
- Confirmed portable-pty 0.9.0 API from the downloaded registry source: `native_pty_system`, `PtySystem::openpty(PtySize)`, `SlavePty::spawn_command(CommandBuilder)`, master reader/writer/resize, and `Child::{try_wait,kill,wait}`.

## Test-first red run

Initial bootstrap command:

```text
cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_contract -- --ignored --test-threads=1
```

It reported **14 passed, 26 failed, 0 ignored**. Failure causes observed in the output:

- The test host's `/tmp` quota rejected Git's detached-worktree metadata write (`fatal: could not write to '/tmp/ira-parity-baseline-…/.git': Disk quota exceeded`), affecting baseline-backed cases.
- Parser rejected the fixture's `[[observations]]` entries as unknown `kind` due to the initial Serde flatten/unknown-field combination; fixed in the source afterward.
- Remaining failures included incomplete cache/test-double/runner behavior. The summarized initial output was truncated by the tool, so no per-test failure claims beyond the observed messages above.

Local quota workaround only: rerun commands set `TMPDIR=/dev/shm CARGO_TARGET_DIR=/dev/shm/ira-parity-crate-target`. Code uses Rust's platform temp directory and inherited `CARGO_TARGET_DIR` (or a target directory inside the temporary worktree); no `/dev/shm` path is hardcoded. The checkout is held by the returned `Baseline` and its `Drop` attempts `git worktree remove --force`, then directory cleanup. The cache is outside the repo and intentionally persists. The first observed checkout was removed after a failed build. All exits and cleanup errors are not yet proven.

## Follow-up verification

- `cargo test ... --test f002_contract --no-run`: passed after schema API alignment.
- Trace round-trip focused test: 1 passed.
- Normalizer focused tests: 2 passed.
- Mismatch diagnostic: 1 passed.
- Observation mutation comparison: 1 passed.
- `cargo tree --manifest-path tools/ira-parity/Cargo.toml --locked -e normal`: completed. Closure includes portable-pty 0.9.0 (`anyhow`, `downcast-rs`, `filedescriptor`, `libc`, `log`, `nix`, `serial2`, `shell-words`) and direct `serde`, `sha2`, `tempfile`, `thiserror`, `toml`, `hex`; this is not a license audit. F-150 must audit full closure.
- First live-run diagnosis: retaining the parent-side portable-pty slave handle prevented reader EOF and made the reader join hang. The runner now drops that handle immediately after spawn, collects observations rather than waiting for an interactive TUI to exit, clears inherited child environment, applies validated scenario overrides, and terminates/reaps under a cleanup deadline. `tagged_oracle_trace_replays_on_linux_macos_windows` passed on this Linux host (10.91s); `pty_adapter_maps_supported_key_press_text_paste_and_resize_on_linux` passed (14.24s); repeat/release rejection passed. The child is launched in a generated fixture directory with isolated HOME/XDG/TMP roots, and `env_clear()` is used before adding the generated allowlist.
- Additional focused tests passed after the initial red run: round-trip; unknown version; missing required fields; malformed tagged event/unknown field; unsupported platform; target event/observation contracts; unsupported observation; UI-neutral domain name; separate process streams; readiness and readiness timeout/no-input; environment override and symlink containment; poisoned Unix environment; normalizer (2); exact-byte and screen mismatch/mutation; golden write/stage/metadata. These are not all 40 tests and do not close their GAPs without a commit and reviewer verification.

## Open blockers and risks

- Baseline cache metadata/sidecar identity and digest validation, cache reuse/build-once semantics, injected cleanup failures, and cleanup on every baseline-resolver failure path remain incomplete.
- Reader joining after normal child termination is exercised by Linux live tests, but bounded reader-drain behavior on pathological OS/PTY failure is not yet proven. Readiness is observation-driven with a deadline, but screen normalization and comparison semantics need further hardening.
- Windows ConPTY and macOS PTY are not verified here. Several accepted test contracts remain failing or unrun; none of their GAP notes were closed.
- `cargo clippy --manifest-path tools/ira-parity/Cargo.toml --locked --all-targets -- -D warnings` now passes. Full suite is not green.

NEXT ACTION: fix PTY lifecycle/readiness first, then implement verified cache metadata and rerun all contracts without hiding GAPs; obtain native macOS/Windows verification via F-150.
