# Logic report: F-001 round 1

Commit reviewed: `296c8927b57e526b2144f431397d2343e3aeb414` (developer); checked at `b06c919`.

Oracle files read: `Cargo.toml`, `Cargo.lock`, `src/**` at `tui-oracle-baseline` (`1cad4ce`); `desktop/Cargo.toml`, `desktop/Cargo.lock`, `desktop/src/main.rs`; `crates/core/Cargo.toml`, `crates/core/src/lib.rs`; `migration/specs/F-001.md`; decisions D-0001, D-0004, D-0008, D-0009, D-0010; boundary test and developer evidence.

## Equivalence table

| Spec | Oracle / contract and observed step | New implementation and evidence | Verdict / limit |
|---|---|---|---|
| S1 | Frozen root manifest identifies `ira`; root TUI source and lockfile are the baseline. | Root manifest remains `ira`, retains terminal deps and adds only local `ira-core`; `src/**` is unchanged against `tui-oracle-baseline`; root lock adds only `ira-core` to the ira dependency list. Locked root build passed. | Equivalent structurally. Build evidence does not independently characterize CLI/output behavior; unchanged source supports the no-change claim. |
| S2 | Desktop contract is the existing `ira-desktop` GPUI 0.2.2 shell: reopen, open-window, button label/click state. | Manifest still `ira-desktop`, GPUI 0.2.2; source is unchanged from base and contains the contract markers; local core dep and lock entry added. Locked desktop build passed. | Equivalent to the existing shell contract. This is source/build evidence only; no native launch or visual smoke was performed. |
| S3 | UI-neutral package boundary: no host framework dependencies/imports, no cross-host imports. | New `ira-core` has no dependencies and only crate docs plus `forbid(unsafe_code)`; S3 scans its Rust source and checks both host trees for opposite-host imports. | Equivalent for the introduced source. The empty core has no feature behavior to compare. |
| S4 | Root and desktop remain independent manifests/locks; no umbrella workspace. | Both manifests use relative local paths resolving to `crates/core`; separate committed lockfiles each contain `ira-core`; neither app manifest declares a workspace. Both `--locked` builds pass. | Equivalent. Locked builds establish both dependency graphs resolve independently. |
| S5 | Existing root test suite and both package builds remain green. | Boundary test S5 runs the two locked builds and root tests; developer reports 278 root tests passed. Review run of boundary suite passed, including S5. | Equivalent for the tested build/test contract. The suite provides no native window runtime evidence. |
| S6 | No feature extraction or behavior change; only minimum adapter wiring, with lifecycle/actions/etc. reserved. | Root `src/**` unchanged from baseline; core contains only `lib.rs`; desktop source remains only `main.rs` and unchanged; implementation diff adds package/manifests/locks only. | Equivalent; no unrelated feature behavior slipped in. |

The row is structural and adds no user-visible surface. No user-visible parity or visual-success claim is made. `G-F001-ADV-03` and `G-F001-ADV-04` are supported by the passing S3/S4 checks and are ready for their owner to resolve; I did not edit their GAP markers.

## Verification

- `python3 -m unittest discover -s tests/boundary -p 'test_*.py' -v` — 6 tests passed, including root/desktop locked builds and root tests invoked by S5.
- `git diff --quiet tui-oracle-baseline 296c892 -- src` — passed; root TUI source unchanged.
- `git diff --check 9b08eb9..HEAD` — passed.
- Requested `python scripts/scope_check.py reviewer --range 9b08eb9..HEAD` with the six specified noise allowances reports out-of-scope `Cargo.toml`, `Cargo.lock`, `desktop/{Cargo.toml,Cargo.lock}`, and `crates/core/**`. These are committed F-001 developer changes in the required comparison range, not reviewer edits. The checker has no option to subtract pre-existing range changes; reviewer edits are confined to the allowed report (this file), and the boundary test already existed in the range.

Open GAPs: none found by logic review. The two adversarial `GAP-FIXED` notes await adversarial reviewer resolution. Native launch/visual smoke remains unverified and is outside this structural equivalence claim.

Verdict: **EQUIVALENT** (high confidence for the specified structural/build contract; no runtime visual claim).
