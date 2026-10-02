# Logic report: F-002 round 1

Commit reviewed: `88dcf9c38185c3a10ee1938050c9dd095e456a2b`  
Oracle: `tui-oracle-baseline` peeled to `1cad4ce43cc72d52d4cc4eef920e0da22cb69568`  
Oracle files read: `src/event.rs:40-65`, `src/main.rs:55-72`, `src/tui.rs:57-67`; F-002 traces in `migration/oracle/traces/harness/`; approved F-002 spec.

## Contract equivalence table

| Clause | Requirement | Implementation and evidence | Equivalent? / gap |
|---|---|---|---|
| S1 | Versioned TOML, strict validation, readiness, ordered events, platforms and expected observations | `trace.rs:162-201`; parser tests in `f002_contract.rs`; Linux tests pass | Partial. Platform names parse, but execution never checks trace platform against `RunOptions.platform` (`runner.rs:156-167`). Expected observation schemas are unvalidated and arbitrary fixture kinds parse. |
| S2 | Run exact frozen oracle from verified detached checkout; no current-tree fallback | `baseline.rs:191-209,245-281`; isolated temp git remote/tag tests pass | Partial. Exact local peeled SHA is verified and build uses detached SHA, but missing tag returns immediately; no fetch is attempted. Reproduced by `baseline_resolution_fetches_missing_pinned_tag` (G-F002-LOG-04). |
| S3 | UI-neutral target contract, ordered typed events, declared observation support and observations | `trace.rs:9-88`, `runner.rs:79-99,156-167,245-255`; target order test passes | No. `TraceTarget` has no supported-kind declaration; TUI support is hard-coded and reports process/filesystem/persistence as supported. Assertions for those observations are ignored; `process_observation_assertions_are_compared` fails (G-F002-LOG-01). |
| S4 | Native PTY/ConPTY, semantic event encoding, unsupported combinations rejected; live all platforms | `runner.rs:323-423,579-612`; Linux key/text/paste/resize and live oracle tests pass | Partial. No macOS/Windows native evidence in this Linux worktree. TUI oracle enables bracketed paste (`src/tui.rs:62-67`) and routes parsed `Event::Paste` to `handle_paste` (`src/main.rs:65-68`), while adapter sends paste payload as plain bytes (`runner.rs:582`), so paste is not preserved as a paste event. |
| S5 | Normalize control sequences/declared volatile roots; preserve text/order; exact filesystem and persistence comparisons | `normalize.rs:10-31,50-59`; normalizer and standalone byte comparator tests pass | No. CSI final `~` drops following `M` (`normalizer_preserves_text_after_csi_tilde_final`, G-F002-LOG-02). The live oracle matcher only checks screen `contains` and dimensions (`runner.rs:477-505`); filesystem/persisted/process expected values are not compared (G-F002-LOG-01). |
| S6 | Actionable expected/actual diffs; candidate oracle observations go to staging only | `normalize.rs:33-48`; `golden.rs:64-83`; staging-preservation tests pass | Partial. `compare_screen` can format a diff but live comparison returns bool and the runner emits a generic timeout, not scenario/observation/expected/actual. Golden capture writes the input trace as `initial_screen.toml`, not captured observations; metadata hard-codes OS/date to `unknown` (`golden.rs:67-80`). |
| S7 | Fetch/verify baseline, detached build, complete cache identity and digest, cleanup on every exit | `baseline.rs:14-109,191-233,245-380,425-437`; 13 ignored baseline/cleanup tests pass when explicitly run | Partial. Cache metadata binds SHA/OS/target/rustc string/lock hash/profile and executable digest. Fetch is missing (G-LOG-04). `Drop` discards cleanup errors (`baseline.rs:434-437`), though `run_oracle_trace` explicitly surfaces cleanup failure (`runner.rs:283-304`). No Windows cleanup evidence here. |
| S8 | Readiness observation before input; absolute bounded deadline; terminate/reap, close handles, drain/join readers, clean fixtures | `runner.rs:187-243,332-465,506-577`; Linux tests including ignored timeout tests pass | Partial. Runner reads raw PTY output and ignores `readiness.observation`; comparison semantics only process terminal bytes. Reader cleanup reports deadline failure but detaches a still-running reader (`runner.rs:554-565`). Native Windows handle-safe behavior unverified. |
| S9 | Cleared, allowlisted child env; protected roots stay inside scenario root; no external symlink traversal | `environment.rs:47-164`; poison-env and existing-symlink tests pass | No. Missing-leaf under an in-root symlink passes lexical containment, `create_dir_all` creates it externally, then later validation rejects. Reproduced by `symlink_parent_escape_has_no_external_side_effect` (G-F002-LOG-03). |
| S10 | Committed golden metadata includes exact oracle, scenario, fixture, dimensions, logical events, OS profile, capture date; human review | `golden.rs:7-49,67-80`; metadata field tests pass | No. Capture hard-codes OS and date as `unknown`; event serialization is a debug string. No committed output is self-approved, which is correct. |
| S11 | Schema, isolation, normalization, native live replay, diagnostics, timeout, poisoned env, parallelism and mutation on native CI | Contract tests: default 27 passed / 13 ignored; ignored suite 13 passed on Linux; live Linux tag replay passes | Partial. Four new logic probes fail as recorded. Current evidence is Linux only; the macOS and Windows jobs/ConPTY replay are an F-150 obligation and remain open. |

## Derived oracle model

`src/event.rs:50-64` polls and reads Crossterm events; it forwards only key `Press`, but forwards resize and paste as distinct events. `src/main.rs:63-68` dispatches key, paste, and resize separately; paste reaches `app.handle_paste`. `src/tui.rs:61-67` enables raw mode, alternate screen, mouse capture, and bracketed paste. The F-002 adapter preserves list order and applies PTY resize in order, but plain payload bytes cannot reproduce Crossterm's bracketed-paste event boundary. The screen oracle is a byte stream; its observation still must be parsed and asserted according to the declared kind rather than treating every expected observation as an optional screen substring.

## Executed evidence

- `TMPDIR=/dev/shm CARGO_TARGET_DIR=/dev/shm/ira-parity-logic-target cargo test --manifest-path tools/ira-parity/Cargo.toml --locked`: **27 passed, 13 ignored**, including live tagged-oracle replay on Linux.
- Same command with `-- --ignored --test-threads=1`: **13 passed** on Linux. This does not establish macOS PTY or Windows ConPTY behavior.
- New reviewer probes: **4 failed as expected**, reproducing G-F002-LOG-01 through G-F002-LOG-04.
- No GAP-FIXED notes were raised by this logic review, so none were promoted.

## Findings and verdict

Open: G-F002-LOG-01 (high, assertion coverage), G-F002-LOG-02 (medium, screen text loss), G-F002-LOG-03 (high, external filesystem side effect), G-F002-LOG-04 (high, missing baseline fetch). Additional contract rows remain partial for paste fidelity, actionable diffs/golden metadata, and native-platform evidence. No fixes verified.

Verdict: **DIVERGENT (4 demonstrated open GAPs; additional contract/platform requirements unproven)**. Confidence: high for the four failing probes and direct source discrepancies; medium for cross-platform behavior because only Linux was available.
