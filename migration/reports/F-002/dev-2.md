# F-002 developer remediation report (round 2)

Status: PARTIAL. This remediation addresses the assigned review findings; F-002 remains unverified pending the lifecycle and native-platform gaps listed below.

## Test-first evidence

Before implementation, the four logic probes in `tools/ira-parity/tests/f002_logic.rs` all failed:

```text
cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_logic -- --test-threads=1
4 failed: missing-tag fetch, CSI `~` text loss, process-observation mismatch, symlink side effect
```

After implementation, the same probe suite passed **4/4**. The added adversarial suite passed **5/5**, including the incompatible-platform pre-start check and a real frozen-baseline build/cache-hit worktree-removal test.

## Changes

- Observation readiness now samples the declared kind and evaluates returned data before sending input. Unsupported PTY readiness kinds fail before baseline build/start. Pending events are rejected if readiness data does not match; no-event traces report the readiness mismatch in their result.
- Expected observations are compared by kind: terminal text and dimensions, process exit/stdout/stderr, byte-exact filesystem/persisted data, and domain values. Mismatches include scenario/observation/field plus expected and actual values. The PTY adapter rejects process/domain observations it cannot faithfully expose and reads declared fixture files only after path containment checks.
- Runner preflight rejects scenarios that do not list the selected platform before target startup; `windows` aliases the trace profile `windows-msvc`.
- Terminal normalization parses CSI final bytes (including `~`) and OSC terminated by BEL or ST, avoiding the prior dropped-text/leaked-title behavior.
- Protected environment roots validate lexical containment and the deepest existing ancestor’s canonical path before creating leaf directories, preventing symlink escape side effects.
- Baseline resolution fetches only the missing exact tag ref from `origin`, then verifies the peeled commit SHA. Existing tags with the wrong SHA still fail closed.
- Reopened G31 now tests the actual captured filesystem observation bytes before input; G38 stages and parses `metadata.toml`, checking serialized fields and values including event sequence, platform, and generated capture date.

## Verification

- Full active suite after the final G31 assertion: **36 passed, 13 ignored**, including both live Linux PTY replays. Independently rerun with `TMPDIR=/home/vlad/.cache/ira-parity-f002-fix-tmp CARGO_TARGET_DIR=/home/vlad/.cache/ira-parity-f002-fix-target cargo test --manifest-path tools/ira-parity/Cargo.toml --locked -- --test-threads=1`; results were adversarial **5/5**, contract **27 passed / 13 ignored**, logic **4/4**.
- All ignored tests invoked explicitly with `TMPDIR=/home/vlad/.cache/ira-parity-f002-fix-tmp CARGO_TARGET_DIR=/home/vlad/.cache/ira-parity-f002-fix-target cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_contract -- --ignored --test-threads=1`: **13 passed / 0 failed**. G27/29/30 remain open despite their scripted test bodies passing; they do not prove real cleanup failure or native OS child/reader lifecycle behavior.
- Reopened G38 serialized-metadata test passed individually.
- Formatting check and Clippy `-D warnings` passed on the final remediation tree (manager verification). `git diff --check` and `python3 scripts/scope_check.py developer --allow 'tools/ira-parity/**' --allow 'migration/reports/F-002/**'` also passed on the final uncommitted diff.
- To avoid the host’s `/dev/shm`/`/tmp` quota while copying the large oracle executable, these runs used `TMPDIR=/home/vlad/.cache/ira-parity-f002-fix-tmp` and `CARGO_TARGET_DIR=/home/vlad/.cache/ira-parity-f002-fix-target`; no machine-specific path is embedded in code.

## Remaining gaps and risks

- Native macOS/Windows PTY/ConPTY replay and license closure review remain F-150 evidence.
- G27, G29, and G30 remain open pending real lifecycle exit-path proof; no reviewer notes were deleted or closed by assertion suppression.
- Golden capture still stages the scenario trace with metadata rather than a complete independently captured observation record; that pre-existing S6/S10 gap was not part of this remediation.
- The live TUI PTY target does not claim unsupported process/domain observations. Paste protocol fidelity and the other findings in the logic report remain unaddressed. Baseline resolution verifies the peeled commit after resolving the supplied tag name; using an explicitly qualified `refs/tags/...` revision for the initial lookup may further guard against Git ref-name ambiguity and should be considered during independent review.

NEXT ACTION: finish full-suite, fmt/clippy, and scope checks; commit/push the remediation, then request independent reviewer re-verification.
