# Adversarial report: F-002 (red-test mode) S13/S14

Commit reviewed: `46811ea` (`migrate/F-002-review-fixes`)

## Attempts and outcomes

- Read the current F-002 S13/S14 contract and D-0014 on `main`, plus architecture/platform spec-review-2. This review branch remains based on `46811ea` as assigned.
- Read the frozen TUI event path at `tui-oracle-baseline`: `src/event.rs` maps Crossterm `Paste` to `Event::Paste`; `src/main.rs` dispatches to `App::handle_paste`; the handler consumes pasted text as one string. The TUI enables bracketed paste in `src/tui.rs`.
- Ran an isolated Linux tmux session of the local TUI with a temporary HOME and fixture. Pressed `n`, sent a multiline/escape-looking tmux paste buffer, and captured the screen. The pane showed payload characters mixed into the footer/status display; this did not provide a reliable, directly observable `Event::Paste` versus ordinary-key distinction, so it is recorded as an inconclusive live probe, not oracle evidence. The input probe did not touch normal user config.
- `cargo test --manifest-path tools/ira-parity/Cargo.toml --test f002_paste_edges -- --nocapture`: 3 tests; ordinary Text passed, Unix framing and exact closing-marker rejection failed as expected.
- The original S13 compile-contract run failed as expected on missing `ObservationBundle`, `ObservationRecord`, `ObservationValue`, and `capture_bundle_to_staging`. After tightening the contract, the updated compile attempt was blocked before rustc reached the integration target by `Disk quota exceeded`. Code inspection of the reviewed base shows the additional exact missing seam: `runner::RunCapture`, `ScenarioRunner::run_capture_with_target`, and `GoldenStore::capture_run_to_staging`; existing `RunResult` discards observed values. The revised test drives a custom target whose observed `status` differs from the trace expectation, then stages the run capture. It will fail if the capture copies expectations instead of `TraceTarget::observe` results. The S13 byte/path/order tests remain compile-contract tests until the bundle surface exists.
- Path identity is explicitly tested independently from `ObservationKind::Filesystem.relative_path`: the test gives that `String` a `to_string_lossy()` value while the record carries the original `PathBuf`, including invalid Unix bytes. The serialized/reloaded record path must equal the original `PathBuf`; the kind string must never be used as the authoritative serialized path.
- Windows native oracle probe was not available on this Linux host. A `#[cfg(windows)]` red test requires explicit unsupported diagnostics from the adapter; it does not claim Unix parser behavior on Windows.

## New open gaps

- `G-F002-ADV-51` high: staged output must come from a deliberately divergent `TraceTarget::observe` result, not copied from expected trace values (`s13_capture_uses_actual_observation_not_trace_expectation`).
- `G-F002-ADV-52` high: NUL/invalid UTF-8 byte round trip (`s13_byte_observations_round_trip_nul_and_invalid_utf8`).
- `G-F002-ADV-53` high: reversible record path identity for NFC/NFD and non-Unicode Unix paths, independent of the lossy path string in `ObservationKind` (`s13_paths_round_trip_without_unicode_normalization_or_loss`).
- `G-F002-ADV-54` medium: deterministic record ordering and manifest-last behavior after a forced payload staging error (`s13_bundle_order_is_stable_and_manifest_is_published_last`). The test contract assumes payloads in an `observations/` directory; adapt the obstruction to the final layout without weakening manifest atomicity.
- `G-F002-ADV-55` high: Unix paste framing for newline and safe escape-like/opening-like values (`unix_paste_frames_newline_and_safe_escape_looking_payloads`).
- `G-F002-ADV-56` high: reject exact `ESC[201~` payload marker (`unix_paste_rejects_the_exact_closing_marker`).
- `G-F002-ADV-57` medium: ordinary Text remains unframed (`ordinary_text_does_not_gain_bracketed_paste_framing`); currently passes and is retained as regression coverage.
- `G-F002-ADV-58` high: Windows native paste path explicitly rejects unsupported Paste instead of typing it (`windows_frozen_oracle_reports_paste_unsupported_instead_of_typing_it`). Native confirmation remains pending.

LOG-05 and LOG-06 remain open. No GAP status was changed. No production files were edited. No mutation checks were performed in red-test mode.

Verdict: FINDINGS (S13 capture API absent; S14 Unix encoder diverges; Windows native behavior unverified).
