# F-002 developer remediation report (round 3)

Status: FIXED, pending independent reviewer resolution of `G-F002-ADV-50`. F-002 remains unverified; prior lifecycle and native-platform gaps remain open.

## Review finding and red/green evidence

Cherry-picked adversarial reviewer commit `ab9163f` (source commit `a1c5d0a4f97356faa12c2066fda0975c3aef416c`). It added the G-F002-ADV-50 collision test and reviewer report/markers. No production code was included in that commit. The cherry-pick had one expected overlap in the adversarial test file; retained the current remediation's expected-observation test and added the G50 probe unchanged.

Before changing production code, ran:

```text
TMPDIR=/home/vlad/.cache/ira-parity-f002-fix-tmp CARGO_TARGET_DIR=/home/vlad/.cache/ira-parity-f002-fix-target cargo test --manifest-path tools/ira-parity/Cargo.toml --locked --test f002_adversarial baseline_resolution_is_qualified_to_tag_namespace -- --exact --nocapture
```

The test failed as expected. The result was `SHA mismatch: expected 1b21fc6b4989065d3de14619004077a800cb3cae, found ddb693d7d1748f5fb30a80be03c2cce688ac6b9d`, confirming that unqualified revision lookup selected the conflicting `refs/oracle` commit.

Changed `BaselineResolver::resolve` to use the fully qualified `refs/tags/<tag>^{commit}` revision for both local lookup and peeled-commit verification after fetch. Fetch already targets the fully qualified `refs/tags/<tag>` destination. Updated G50 from `GAP` to `GAP-FIXED`, awaiting reviewer verification.

The same focused command after the fix passed: **1 passed, 0 failed**.

## Verification

- Full active package suite: **37 passed, 13 ignored** (6 adversarial, 27 contract, 4 logic), including Linux PTY replays.
- All ignored contract probes, explicitly run with `--ignored --test-threads=1`: **13 passed, 0 failed**.
- `cargo fmt --manifest-path tools/ira-parity/Cargo.toml -- --check`: passed.
- `TMPDIR=/home/vlad/.cache/ira-parity-f002-fix-tmp CARGO_TARGET_DIR=/home/vlad/.cache/ira-parity-f002-fix-target cargo clippy --manifest-path tools/ira-parity/Cargo.toml --locked --all-targets -- -D warnings`: passed.
- `git diff --check`: passed.
- `python3 scripts/scope_check.py developer --allow 'tools/ira-parity/**' --allow 'migration/reports/F-002/**'`: passed for the uncommitted remediation diff.

All build/test commands used the disk-backed cache paths above because `/dev/shm` exhausted its quota during the previous verification attempt.

## Remaining gaps

- `G-F002-ADV-27`, `G-F002-ADV-29`, and `G-F002-ADV-30` remain open for lifecycle and child/reader cleanup evidence.
- Native macOS/Windows PTY/ConPTY runs and license closure remain F-150 evidence.
- F-002 remains unverified pending independent reviewer resolution and the outstanding lifecycle/platform work.
