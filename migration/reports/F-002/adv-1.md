# Adversarial report: F-002 (red-test preparation) round 1

Commit reviewed: `d8250b55aa852a406658961b64039c3f52e719f8` (`main`); oracle commit: `1cad4ce43cc72d52d4cc4eef920e0da22cb69568`.

## Live oracle attempts and observations

The exact baseline was checked out detached at `/dev/shm/ira-f002-oracle` and built with:

```text
CARGO_TARGET_DIR=/dev/shm/ira-f002-target cargo build --locked --manifest-path /dev/shm/ira-f002-oracle/Cargo.toml --bin ira
Finished `dev` profile ...
```

All launches used a fresh temporary HOME/XDG/TMP root, `TERM=xterm-256color`, and `IRA_IMAGES=blocks`.

1. **Happy startup:** `tmux new-session -d -x 90 -y 24 -- /dev/shm/ira-f002-target/debug/ira`. The live screen showed the Drives panel (two mounted drives), Common folders (Home), Bookmarks, Actions, the empty file pane, and key hints. This confirms screen output is active on a PTY and user-visible ordering/panel labels are observable. TUI startup initializes to mounted drives/common folders, not the process working directory (`src/app.rs:759-784`).
2. **Resize edge:** while the same live screen was running, resized tmux from `90x24` to `18x5`; `#{pane_width}x#{pane_height}` reported `18x5`, the TUI remained alive, and capture showed the small viewport. The TUI event loop receives resize events (`src/event.rs:59-60`) and redraws each loop (`src/main.rs:58-69`).
3. **Non-TTY process edge:** running the frozen binary with piped stdio and `--version` returned exit 0, stdout exactly `ira 0.1.21\n`, stderr empty. `--check-terminal` under the same non-TTY/isolated environment returned exit 0 and stdout reported `truecolor: no (256-color fallback)`, `image protocol: Blocks (IRA_IMAGES override)`, `cell size: 10x20 px`, and an absent theme file under the temporary XDG root; stderr was empty. Flag early returns and output routing are at `src/main.rs:11-24`.

PTy input injection did not produce a trustworthy key-event result in this environment: tmux output displayed the UI and resized it, but injected `q` did not quit; a separate Ctrl-C attempt terminated by signal. I do not treat this as a TUI behavior finding or as an asserted oracle result. A parity adapter must prove its own startup-readiness and input delivery before using key outcomes as goldens. No oracle files or fixtures were changed.

## Input/observation schema proposal (accepted)

Use the F-002 S3 enums as tagged TOML values with closed field sets. Keep event order as the trace array order; do not sort or coalesce events. Proposed first contract tests (all require the absent `tools/ira-parity` crate and are therefore not added yet):

| Clause | Proposed exact red test(s) | Coverable now? |
|---|---|---|
| S1 | `trace_v1_round_trips_ordered_key_text_paste_resize_events`; `trace_rejects_unknown_version`; `trace_rejects_missing_fixture_dimensions_events_or_observations`; `trace_rejects_malformed_tagged_event_and_unknown_field`; `trace_rejects_unsupported_platform_profile` | Oracle CLI/screen observation only; schema tests require harness |
| S2 | `baseline_rejects_missing_tag_and_wrong_peeled_sha`; `baseline_build_uses_verified_detached_worktree_root_lockfile`; `baseline_never_selects_current_head_binary`; `baseline_builds_once_for_job` | Frozen tag/build verified manually; resolver tests require harness |
| S3 | `target_contract_applies_events_in_order_and_observes_each_declared_kind`; `target_rejects_unsupported_observation_kind`; `target_keeps_domain_snapshot_names_ui_neutral`; `target_keeps_process_stdout_and_stderr_distinct` | TUI render observed; contract tests require harness |
| S4 | `pty_adapter_maps_supported_key_press_text_paste_and_resize_on_linux`; same test on macOS; `conpty_adapter_maps_supported_events_on_windows`; `adapter_fails_explicitly_for_unsupported_key_phase_or_modifier` | Resize observed on Linux; native three-OS adapter tests require harness/runners |
| S5 | `normalizer_removes_only_declared_volatile_roots_and_control_sequences`; `normalizer_preserves_names_order_selection_and_messages`; `filesystem_and_persisted_observations_compare_exact_bytes` | Baseline screen visible; deterministic comparison tests require harness |
| S6 | `mismatch_diff_names_scenario_observation_expected_and_actual`; `golden_capture_writes_only_to_selected_staging_root`; `ordinary_run_does_not_modify_approved_goldens` | Requires harness |
| S7 | `baseline_cache_key_and_sidecar_bind_sha_os_target_toolchain_lock_hash_profile`; `cache_hit_verifies_metadata_and_executable_digest`; `cache_miss_builds_exact_baseline`; `baseline_worktree_is_removed_after_success_cache_hit_build_failure_timeout_and_test_failure`; `cleanup_error_is_reported_with_primary_error` | Exact baseline build manually verified; cache/cleanup tests require resolver |
| S8 | `deadline_terminates_reaps_child_and_closes_pty`; `timeout_drains_and_joins_readers_before_fixture_removal`; `timeout_reports_primary_and_cleanup_errors`; `readiness_uses_output_polling_and_absolute_deadline` | Manual PTY attempt only; bounded lifecycle tests require harness |
| S9 | `poisoned_parent_environment_cannot_escape_isolated_roots_unix`; Windows profile counterpart; `scenario_override_cannot_escape_protected_roots`; `protected_root_symlink_escape_is_rejected`; `only_documented_os_environment_is_inherited` | Isolated roots manually used; adversarial env tests require harness |
| S10 | `golden_metadata_requires_oracle_sha_scenario_fixture_dimensions_events_os_and_date`; `golden_capture_stages_without_replacing_approved_capture`; `golden_update_requires_external_logic_review_evidence` | Requires trace/golden format |
| S11 | `tagged_oracle_trace_replays_on_linux_macos_windows`; `malformed_trace_fails_before_child_start`; `parallel_scenarios_have_disjoint_roots_and_sessions`; `changed_observation_fails_assertion_mutation` | Linux baseline startup/resize observed; full tests require harness and F-150 runners |

The manager accepted the proposal and committed readiness-before-input plus key-press-only TUI adapter clarifications in `67da7b88b9c9d192302ed34c33e3b0dfa156ecd8`. Those decisions are reflected in F-002 S1/S3/S4/S8.

## Contract points to settle before implementation

- S3's `key.phase = press|repeat|release` and S4's promise to preserve phase need an explicit adapter rule: which phases Crossterm/ConPTY can actually emit, and whether a phase absent from the selected terminal protocol must fail as unsupported. The existing TUI event loop only forwards `KeyEventKind::Press` (`src/event.rs:50-56`); it does not provide a repeat/release oracle behavior.
- Readiness should be an explicit harness condition, not elapsed delay. The current TUI capability probe runs before raw mode/window drawing (`src/main.rs:26-56`), so sending input on alternate-screen entry alone can race the first rendered frame.
- PTY-driven q/Ctrl-C attempts were inconclusive as recorded above. Harness smoke tests must establish event delivery and capture the process result before adding event-derived goldens.

These were test-design clarifications, not `kind=spec-gap` notes.

## Red suite prepared after acceptance

Added 43 `#[test]` contract cases in `tools/ira-parity/tests/f002_contract.rs`, covering S1–S11, with one `GAP(G-F002-ADV-xx)` `kind=test-gap` note per test. All are ignored with the corresponding GAP ID because this checkout intentionally has no `tools/ira-parity/Cargo.toml` or crate source. Added five schema-v1 TOML fixtures under `migration/oracle/traces/harness/`: `initial_screen`, `all_event_types`, `resize_small`, `reject_repeat`, and `reject_release`. Every trace gates input on a `terminal_screen` observation containing `Common folders`; supported-event trace sends key press, text, paste, and resize in order, with a final 18x5 dimension assertion. It uses `z` as the non-exiting key. Repeat/release traces expect explicit unsupported-event errors.

Pre-harness checks: TOML syntax and Rust formatting pass. Cargo execution is impossible because the package is absent; exact command and output:

```text
$ cargo test --manifest-path tools/ira-parity/Cargo.toml --test f002_contract
error: manifest path `tools/ira-parity/Cargo.toml` does not exist
```

The suite's API signatures are proposals for integrator confirmation, not confirmed crate APIs. Assumptions include crate `ira_parity`; modules/types `baseline::{BaselineResolver, CacheMetadata}`, `environment::{ChildEnvironment, EnvironmentPolicy}`, `golden::{GoldenMetadata, GoldenStore}`, `normalize::{compare_bytes, compare_screen, normalize_screen}`, `runner::{RunOptions, ScenarioRunner}`, `testing::ScriptedTarget`, and `trace::{parse_trace, InputEvent, KeyCode, KeyPhase, ObservationKind, Trace}`; plus methods invoked by the contract tests. Fixture field names and `path_is_within_root` are also proposed seam details. Coverage that requires an actual child, native PTY/ConPTY, real cleanup, environment construction, or golden implementation remains unexecuted until the integrator scaffolds the crate. No production files, matrix/state/decision files, or other captures were changed.

Attempts made: exact frozen baseline build; tmux first-screen capture; terminal resize 90x24→18x5; non-TTY `--version` and `--check-terminal`; isolated HOME/XDG/TMP roots; PTY q/Ctrl-C input attempts (inconclusive, not asserted). Findings: no spec-gap filed. Verdict: red contract suite prepared; implementation and executable red runs await harness package.
