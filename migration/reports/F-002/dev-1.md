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

The two manual SIGINT interruptions of long-running test processes bypassed Rust destructors and left four temporary Git worktrees in `/dev/shm`; I removed those with `git worktree remove --force` and pruned the Git worktree registry. Abrupt parent-process termination cleanup is therefore explicitly unproven and remains a lifecycle risk; ordinary return/error paths still need the dedicated assertions in G-F002-ADV-27/28.

## Follow-up verification

- `cargo test ... --test f002_contract --no-run`: passed after schema API alignment.
- Trace round-trip focused test: 1 passed.
- Normalizer focused tests: 2 passed.
- Mismatch diagnostic: 1 passed.
- Observation mutation comparison: 1 passed.
- `cargo tree --manifest-path tools/ira-parity/Cargo.toml --locked -e normal`: completed. Closure includes portable-pty 0.9.0 (`anyhow`, `downcast-rs`, `filedescriptor`, `libc`, `log`, `nix`, `serial2`, `shell-words`) and direct `serde`, `sha2`, `tempfile`, `thiserror`, `toml`, `hex`; this is not a license audit. F-150 must audit full closure.
- First live-run diagnosis: retaining the parent-side portable-pty slave handle prevented reader EOF and made the reader join hang. The runner now drops that handle immediately after spawn, collects observations rather than waiting for an interactive TUI to exit, clears inherited child environment, applies validated scenario overrides, and terminates/reaps under a cleanup deadline. `tagged_oracle_trace_replays_on_linux_macos_windows` passed on this Linux host (10.91s); `pty_adapter_maps_supported_key_press_text_paste_and_resize_on_linux` passed (14.24s); repeat/release rejection passed. The child is launched in a generated fixture directory with isolated HOME/XDG/TMP roots, and `env_clear()` is used before adding the generated allowlist.
- Additional focused tests passed after the initial red run: round-trip; unknown version; missing required fields; malformed tagged event/unknown field; unsupported platform; target event/observation contracts; unsupported observation; UI-neutral domain name; separate process streams; readiness and readiness timeout/no-input; environment override and symlink containment; poisoned Unix environment; normalizer (2); exact-byte and screen mismatch/mutation; golden write/stage/metadata. These are not all 40 tests and do not close their GAPs without a commit and reviewer verification.
- After converting individually passing contract cases to `GAP-FIXED` in commit `151e121`, `TMPDIR=/dev/shm CARGO_TARGET_DIR=/dev/shm/ira-parity-crate-target cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_contract -- --test-threads=1 --skip pty_adapter_maps_supported_key_press_text_paste_and_resize_on_linux --skip tagged_oracle_trace_replays_on_linux_macos_windows` passed **25, failed 0, ignored 13**. The two Linux replay cases were filtered because each was verified individually; both passed standalone as noted above.

## S7–S9 hardening and current Linux verification

- Baseline cache metadata now binds the frozen SHA, OS, rustc host target, compiler version, frozen-tag `Cargo.lock` hash, fixed debug profile, and cached executable SHA-256. Cache hits parse and compare the sidecar and verify the executable digest; cache misses build from the detached worktree root manifest/lockfile and copy the binary outside the worktree. Build-once is covered using an isolated test cache so prior local cache state cannot make the test vacuous.
- Baseline resolution queries detached HEAD and clean status. Failed worktree-add, simulated post-add exits, build/cache failures, cache hits, and explicit normal cleanup paths attempt both Git and filesystem removal; cleanup errors are composed with the primary error. `Baseline::Drop` is best-effort, with explicit cleanup errors returned on normal runner exits. A hard process kill still cannot run Rust destructors and is not claimed as covered.
- Scenario environment construction starts from cleared child variables, supplies the platform allowlist and unique HOME/XDG/profile/temp roots, rejects unknown scenario keys, and validates protected overrides lexically and canonically against the unique fixture root. Live PTY startup uses the generated environment and `env_clear()` before applying it. Only the declared fixture root is normalized from screen observations.
- `cargo fmt --manifest-path tools/ira-parity/Cargo.toml -- --check`: passed.
- `TMPDIR=/dev/shm CARGO_TARGET_DIR=/dev/shm/ira-parity-crate-target cargo clippy --manifest-path tools/ira-parity/Cargo.toml --all-targets --locked -- -D warnings`: passed.
- Full unfiltered Linux crate run `cargo test --manifest-path tools/ira-parity/Cargo.toml --locked -- --test-threads=1`: **27 passed, 0 failed, 13 GAP-annotated ignored**, in 18.65s. Both live Linux PTY replay scenarios ran in this set.
- Explicit invocation of every ignored case with `cargo test ... --test f002_contract -- --ignored --test-threads=1`: **13 passed, 0 failed**, in 38.47s. These include scripted lifecycle tests G-F002-ADV-29/30; they remain open because scripted self-report is not evidence of terminating a real OS child under a PTY deadline.
- Baseline-specific ignored cases: **6 passed, 0 failed**, in 26.15s, including exact SHA/detached clean checkout, build-once, cache metadata, and cleanup paths. `git worktree list` has no leaked `ira-parity-baseline-*` checkout.
- `cargo tree --manifest-path tools/ira-parity/Cargo.toml --locked -e normal` and full `cargo metadata --locked` resolved the complete lock closure. Packages/licenses: anyhow (MIT OR Apache-2.0); bitflags 1.3.2 (MIT/Apache-2.0), 2.13.2 (MIT OR Apache-2.0); block-buffer (MIT OR Apache-2.0); cfg-if (MIT OR Apache-2.0); cfg_aliases (MIT); cpufeatures (MIT OR Apache-2.0); crypto-common (MIT OR Apache-2.0); digest (MIT OR Apache-2.0); downcast-rs (MIT/Apache-2.0); equivalent (Apache-2.0 OR MIT); errno (MIT OR Apache-2.0); fastrand (Apache-2.0 OR MIT); filedescriptor (MIT); generic-array (MIT); getrandom (MIT OR Apache-2.0); hashbrown (MIT OR Apache-2.0); hex (MIT OR Apache-2.0); indexmap (Apache-2.0 OR MIT); lazy_static (MIT OR Apache-2.0); libc (MIT OR Apache-2.0); linux-raw-sys (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT); log (MIT OR Apache-2.0); memchr (Unlicense OR MIT); nix (MIT); once_cell (MIT OR Apache-2.0); portable-pty (MIT); proc-macro2/quote/syn 2/syn 3/serde/serde_core/serde_derive/serde_spanned/sha2/tempfile/thiserror 1/thiserror 2/thiserror-impl 1/thiserror-impl 2/toml/toml_datetime (all MIT OR Apache-2.0); r-efi (MIT OR Apache-2.0 OR LGPL-2.1-or-later); rustix (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT); serial2 (BSD-2-Clause OR Apache-2.0); shared_library (Apache-2.0/MIT); shell-words (MIT/Apache-2.0); toml_edit (MIT OR Apache-2.0); toml_write/winnow (MIT); typenum (MIT OR Apache-2.0); unicode-ident ((MIT OR Apache-2.0) AND Unicode-3.0); version_check (MIT/Apache-2.0); winapi (MIT/Apache-2.0), winapi-i686-pc-windows-gnu (MIT/Apache-2.0), winapi-x86_64-pc-windows-gnu (MIT/Apache-2.0); windows-link (MIT OR Apache-2.0); windows-sys (MIT OR Apache-2.0); winreg (MIT). Root package `ira-parity` has no declared license metadata. This is an inventory, not a legal approval: F-150 must confirm license-file/notice obligations and target-specific closure, especially optional r-efi LGPL alternative, Unlicense, BSD-2-Clause, and Windows dependencies.

## Open blockers and risks

- Native macOS PTY and Windows MSVC ConPTY replay, dependency license closure audit, and all-platform poisoned environment behavior belong to F-150 and remain unverified here.
- G-F002-ADV-29/30 remain open: current test doubles verify the runner's orchestration but do not prove OS child termination/reaping or reader cleanup under native PTY failure. S8 synchronous PTY writes/resizes are deadline-checked before each event, but the OS calls themselves are not cancellable if an OS backend blocks inside a call; native pathological-timeout evidence is still required.
- The test GAP notes not closed by the three prior commits remain open pending reviewer verification; this report does not claim F-002 is verified.

NEXT ACTION: commit/push this hardening checkpoint; reviewers resolve implementation/test GAPs, while F-150 supplies native macOS/Windows replay, ConPTY lifecycle, and dependency-license evidence.
