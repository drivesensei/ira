//! F-002 red contract suite.
//!
//! API assumptions submitted for integrator confirmation are recorded in
//! migration/reports/F-002/adv-1.md. This file intentionally targets the
//! approved API seam; the package and implementation do not exist yet.

use ira_parity::baseline::{BaselineResolver, CacheMetadata};
use ira_parity::environment::{ChildEnvironment, EnvironmentPolicy};
use ira_parity::golden::{GoldenMetadata, GoldenStore};
use ira_parity::normalize::{compare_bytes, compare_screen, normalize_screen};
use ira_parity::runner::{RunOptions, ScenarioRunner};
use ira_parity::testing::ScriptedTarget;
use ira_parity::trace::{parse_trace, InputEvent, KeyCode, KeyPhase, ObservationKind, Trace};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const ORACLE_SHA: &str = "1cad4ce43cc72d52d4cc4eef920e0da22cb69568";
const ROOT: &str = "../../migration/oracle/traces/harness";
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ira-parity-f002-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn trace_file(name: &str) -> Trace {
    let path = PathBuf::from(ROOT).join(name);
    let source = std::fs::read_to_string(path).expect("trace fixture exists");
    parse_trace(&source).expect("trace fixture parses")
}

fn parse(source: &str) -> Result<Trace, ira_parity::trace::TraceError> {
    parse_trace(source)
}

fn minimal_trace() -> &'static str {
    include_str!("../../../migration/oracle/traces/harness/initial_screen.toml")
}

fn tempdir() -> TempDir {
    TempDir::new()
}

// GAP-RESOLVED(G-F002-ADV-01) sev=medium kind=test-gap feature=F-002
//   what:     S1 schema validation has no executable harness package yet.
//   tui-ref:  migration/specs/F-002.md S1
//   oracle:   tui-oracle-baseline 1cad4ce43cc72d52d4cc4eef920e0da22cb69568
//   repro:    Parse the valid trace and inspect event order.
//   expected: All logical events round-trip in source order without coalescing.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and parser are absent.
//   cover:    trace_v1_round_trips_ordered_key_text_paste_resize_events
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn trace_v1_round_trips_ordered_key_text_paste_resize_events() {
    let trace = trace_file("all_event_types.toml");
    let events = trace.events();
    assert_eq!(events.len(), 4);
    assert!(matches!(events[0], InputEvent::Key { .. }));
    assert!(matches!(events[1], InputEvent::Text { .. }));
    assert!(matches!(events[2], InputEvent::Paste { .. }));
    assert!(matches!(
        events[3],
        InputEvent::Resize {
            columns: 18,
            rows: 5
        }
    ));
    assert_eq!(parse_trace(&trace.to_toml()).unwrap().events(), events);
}

// GAP-RESOLVED(G-F002-ADV-02) sev=medium kind=test-gap feature=F-002
//   what:     S1 unknown schema versions cannot be exercised before the parser exists.
//   tui-ref:  migration/specs/F-002.md S1
//   oracle:   F-002 schema contract
//   repro:    Replace schema_version 1 with 999.
//   expected: Parsing fails with an unsupported-version diagnostic.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and parser are absent.
//   cover:    trace_rejects_unknown_version
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn trace_rejects_unknown_version() {
    let invalid = minimal_trace().replace("schema_version = 1", "schema_version = 999");
    let error = parse(&invalid).unwrap_err().to_string();
    assert!(error.contains("version"), "{error}");
}

// GAP-RESOLVED(G-F002-ADV-03) sev=medium kind=test-gap feature=F-002
//   what:     S1 required fields, including observation readiness, lack parser coverage.
//   tui-ref:  migration/specs/F-002.md S1, S3, S8
//   oracle:   F-002 schema contract
//   repro:    Remove each required top-level field, including readiness.
//   expected: Every omitted field is rejected with its field name.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and parser are absent.
//   cover:    trace_rejects_missing_required_fields_including_readiness
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn trace_rejects_missing_required_fields_including_readiness() {
    let source = minimal_trace();
    let invalid_cases = [
        (
            source.replace(r#"fixture = { kind = "empty_directory" }"#, ""),
            "fixture",
        ),
        (
            source.replace("[terminal]", "[missing_terminal]"),
            "terminal",
        ),
        (
            source.replace("platforms = [", "missing_platforms = ["),
            "platforms",
        ),
        (
            source.replace("[readiness]", "[missing_readiness]"),
            "readiness",
        ),
        (
            source.replace("events = []", "missing_events = []"),
            "events",
        ),
        (
            source.replace("[[observations]]", "[[missing_observations]]"),
            "observations",
        ),
    ];
    for (invalid, field) in invalid_cases {
        let error = parse(&invalid).unwrap_err().to_string();
        assert!(error.contains(field), "missing {field}: {error}");
    }
}

// GAP-RESOLVED(G-F002-ADV-04) sev=medium kind=test-gap feature=F-002
//   what:     S1 malformed tagged events and unknown fields lack parser coverage.
//   tui-ref:  migration/specs/F-002.md S1, S3
//   oracle:   F-002 schema contract
//   repro:    Supply two event tags, an unknown tag, and an unknown field.
//   expected: All malformed/unknown members fail closed with useful diagnostics.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and parser are absent.
//   cover:    trace_rejects_malformed_tagged_event_and_unknown_field
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn trace_rejects_malformed_tagged_event_and_unknown_field() {
    let all_events = include_str!("../../../migration/oracle/traces/harness/all_event_types.toml");
    for malformed in [
        all_events.replace(r#"kind = "resize""#, "kind = \"resize\"\nkind = \"paste\""),
        all_events.replace(r#"kind = "resize""#, r#"kind = "surprise""#),
        minimal_trace().replace("schema_version = 1", "schema_version = 1\nunknown = true"),
    ] {
        assert!(
            parse(&malformed).is_err(),
            "accepted malformed trace: {malformed}"
        );
    }
}

// GAP-RESOLVED(G-F002-ADV-05) sev=medium kind=test-gap feature=F-002
//   what:     S1 unsupported platform profiles lack parser coverage.
//   tui-ref:  migration/specs/F-002.md S1, S4
//   oracle:   F-002 schema contract
//   repro:    Add an unknown target profile to the platform list.
//   expected: Parser reports the unsupported profile instead of silently skipping it.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and parser are absent.
//   cover:    trace_rejects_unsupported_platform_profile
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn trace_rejects_unsupported_platform_profile() {
    let invalid = minimal_trace().replace("\"linux\"", "\"plan9\"");
    let error = parse(&invalid).unwrap_err().to_string();
    assert!(
        error.contains("plan9") || error.contains("platform"),
        "{error}"
    );
}

// GAP(G-F002-ADV-06) sev=medium kind=test-gap feature=F-002
//   what:     S2 baseline tag/SHA failure modes lack executable resolver checks.
//   tui-ref:  migration/specs/F-002.md S2, S7
//   oracle:   tui-oracle-baseline expected peeled SHA 1cad4ce43cc72d52d4cc4eef920e0da22cb69568
//   repro:    Resolve a missing tag and a tag pointing at a different commit.
//   expected: Both are explicit errors; no binary is returned.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and resolver are absent.
//   cover:    baseline_rejects_missing_tag_and_wrong_peeled_sha
#[test]
#[ignore = "GAP G-F002-ADV-06"]
fn baseline_rejects_missing_tag_and_wrong_peeled_sha() {
    let missing = BaselineResolver::new(Path::new(env!("CARGO_MANIFEST_DIR")))
        .with_tag("tag-that-does-not-exist")
        .resolve();
    assert!(missing.unwrap_err().to_string().contains("tag"));
    let wrong = BaselineResolver::new(Path::new(env!("CARGO_MANIFEST_DIR")))
        .with_expected_sha("0000000000000000000000000000000000000000")
        .resolve();
    assert!(wrong.unwrap_err().to_string().contains("SHA"));
}

// GAP(G-F002-ADV-07) sev=medium kind=test-gap feature=F-002
//   what:     S2 exact detached-worktree source and locked root build are untested.
//   tui-ref:  migration/specs/F-002.md S2, S7
//   oracle:   tui-oracle-baseline at exact SHA
//   repro:    Resolve and build the baseline in a temporary repository.
//   expected: Worktree is detached/clean and build uses its root Cargo.toml and Cargo.lock with --locked.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and resolver are absent.
//   cover:    baseline_build_uses_verified_detached_worktree_root_lockfile
#[test]
#[ignore = "GAP G-F002-ADV-07"]
fn baseline_build_uses_verified_detached_worktree_root_lockfile() {
    let baseline = BaselineResolver::new(Path::new(env!("CARGO_MANIFEST_DIR")))
        .with_expected_sha(ORACLE_SHA)
        .resolve_and_build()
        .unwrap();
    assert_eq!(baseline.commit_sha(), ORACLE_SHA);
    assert!(baseline.worktree_is_detached_and_clean());
    assert!(baseline
        .build_command()
        .contains("cargo build --locked --manifest-path"));
    assert!(baseline.worktree_path().join("Cargo.lock").is_file());
}

// GAP(G-F002-ADV-08) sev=medium kind=test-gap feature=F-002
//   what:     S2 must prevent accidental use of an unpinned current-tree binary.
//   tui-ref:  migration/specs/F-002.md S2, S7
//   oracle:   tui-oracle-baseline exact SHA
//   repro:    Place a fake current-tree ira executable on the fallback path.
//   expected: Resolver builds/selects only the exact detached baseline artifact.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and resolver are absent.
//   cover:    baseline_never_selects_current_head_binary
#[test]
#[ignore = "GAP G-F002-ADV-08"]
fn baseline_never_selects_current_head_binary() {
    let baseline = BaselineResolver::new(Path::new(env!("CARGO_MANIFEST_DIR")))
        .with_expected_sha(ORACLE_SHA)
        .resolve_and_build()
        .unwrap();
    assert_eq!(baseline.commit_sha(), ORACLE_SHA);
    assert_ne!(
        baseline.executable_path(),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/ira")
    );
}

// GAP(G-F002-ADV-09) sev=medium kind=test-gap feature=F-002
//   what:     S2's per-job build-once promise lacks a regression check.
//   tui-ref:  migration/specs/F-002.md S2, S7
//   oracle:   tui-oracle-baseline exact SHA
//   repro:    Resolve the same baseline twice in one test job.
//   expected: One cargo build occurs and both resolutions identify the same artifact digest.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and resolver are absent.
//   cover:    baseline_builds_once_for_job
#[test]
#[ignore = "GAP G-F002-ADV-09"]
fn baseline_builds_once_for_job() {
    let cache = tempdir();
    let resolver = BaselineResolver::new(Path::new(env!("CARGO_MANIFEST_DIR")))
        .with_expected_sha(ORACLE_SHA)
        .with_cache_dir(cache.path());
    let first = resolver.resolve_and_build().unwrap();
    let second = resolver.resolve_and_build().unwrap();
    assert_eq!(first.build_count(), 1);
    assert_eq!(first.executable_digest(), second.executable_digest());
}

// GAP-RESOLVED(G-F002-ADV-10) sev=medium kind=test-gap feature=F-002
//   what:     S3 event order and every declared observation kind lack target-contract coverage.
//   tui-ref:  migration/specs/F-002.md S3
//   oracle:   migration/oracle/traces/harness/initial_screen.toml
//   repro:    Run the valid trace against a recording target.
//   expected: Target receives events in order; readiness and every declared assertion are sampled.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and runner are absent.
//   cover:    target_contract_applies_events_in_order_and_observes_each_declared_kind
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn target_contract_applies_events_in_order_and_observes_each_declared_kind() {
    let trace = trace_file("initial_screen.toml");
    let mut target = ScriptedTarget::recording();
    ScenarioRunner::new(RunOptions::default())
        .run_with_target(&trace, &mut target)
        .unwrap();
    assert_eq!(target.applied_events(), trace.events());
    let mut expected_observations = vec![trace.readiness.observation.clone()];
    expected_observations.extend(trace.observation_kinds());
    assert_eq!(target.observed_kinds(), expected_observations);
}

// GAP-RESOLVED(G-F002-ADV-11) sev=medium kind=test-gap feature=F-002
//   what:     S3 unsupported target observations must fail with a diagnostic.
//   tui-ref:  migration/specs/F-002.md S3
//   oracle:   F-002 schema contract
//   repro:    Request a GPUI-only domain observation from the TUI target.
//   expected: Runner fails naming the unsupported observation kind and target.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and runner are absent.
//   cover:    target_rejects_unsupported_observation_kind
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn target_rejects_unsupported_observation_kind() {
    let trace = Trace::parse_toml(minimal_trace())
        .unwrap()
        .with_observation(ObservationKind::DomainSnapshot {
            name: "gpui_view_tree".into(),
        });
    let error = ScenarioRunner::new(RunOptions::default())
        .validate_for_target(&trace, "tui-pty")
        .unwrap_err()
        .to_string();
    assert!(error.contains("gpui_view_tree") && error.contains("unsupported"));
}

// GAP(G-F002-ADV-12) sev=medium kind=test-gap feature=F-002
//   what:     S3 domain snapshot names must remain feature-owned and UI-neutral.
//   tui-ref:  migration/specs/F-002.md S3
//   oracle:   F-002 schema contract
//   repro:    Parse a core-owned cwd snapshot and reject a GPUI view/action name.
//   expected: Neutral names parse; GPUI types/action identifiers are not schema dependencies.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and schema are absent.
//   cover:    target_keeps_domain_snapshot_names_ui_neutral
//   fixed-by: 176d6de
//   reopened: Current test checks a static dependency-name list and parses only `cwd`; it does not reject a GPUI/action-style domain label.
#[test]
fn target_keeps_domain_snapshot_names_ui_neutral() {
    let trace = Trace::parse_toml(minimal_trace())
        .unwrap()
        .with_observation(ObservationKind::DomainSnapshot { name: "cwd".into() });
    assert_eq!(
        trace.observation_kinds().last().unwrap().domain_name(),
        Some("cwd")
    );
    assert!(!trace
        .schema_dependencies()
        .iter()
        .any(|name| name.contains("gpui")));
}

// GAP(G-F002-ADV-13) sev=medium kind=test-gap feature=F-002
//   what:     S3 process stdout and stderr observations must stay distinct.
//   tui-ref:  migration/specs/F-002.md S3
//   oracle:   src/main.rs:38-45 (TUI writer selection); F-002 contract
//   repro:    Use a target that writes unique markers to each process stream.
//   expected: Process observation preserves each marker in its own stream field.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and target implementation are absent.
//   cover:    target_keeps_process_stdout_and_stderr_distinct
//   fixed-by: 176d6de
//   reopened: Current test calls ScriptedTarget directly, which fabricates separate markers; no process adapter or actual stdout/stderr streams are exercised.
#[test]
fn target_keeps_process_stdout_and_stderr_distinct() {
    let mut target = ScriptedTarget::writing_stream_markers("stdout-marker", "stderr-marker");
    let observation = target.observe(&ObservationKind::Process).unwrap();
    assert_eq!(observation.stdout(), Some("stdout-marker"));
    assert_eq!(observation.stderr(), Some("stderr-marker"));
}

// GAP-RESOLVED(G-F002-ADV-14) sev=medium kind=test-gap feature=F-002
//   what:     S4 Linux PTY event encoding/order, including readiness, lacks native coverage.
//   tui-ref:  migration/specs/F-002.md S3-S4; src/event.rs:50-63
//   oracle:   migration/oracle/traces/harness/initial_screen.toml
//   repro:    Replay press, text, paste, and resize under Linux PTY.
//   expected: Each supported event reaches the child in sequence after screen readiness.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and PTY adapter are absent.
//   cover:    pty_adapter_maps_supported_key_press_text_paste_and_resize_on_linux
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
#[cfg(target_os = "linux")]
fn pty_adapter_maps_supported_key_press_text_paste_and_resize_on_linux() {
    let trace = trace_file("all_event_types.toml");
    let result = ScenarioRunner::new(RunOptions::for_platform("linux"))
        .run_oracle_trace(&trace)
        .unwrap();
    assert_eq!(result.applied_events(), trace.events());
    assert!(result.readiness_satisfied_before_first_input());
    assert!(result.observations_match());
}

// GAP(G-F002-ADV-15) sev=medium kind=test-gap feature=F-002
//   what:     S4 macOS PTY event encoding/order lacks native coverage.
//   tui-ref:  migration/specs/F-002.md S3-S4; src/event.rs:50-63
//   oracle:   migration/oracle/traces/harness/initial_screen.toml
//   repro:    Replay the same logical event trace under macOS 14 PTY.
//   expected: The same ordered observations are available without silent skips.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and macOS runner are absent.
//   cover:    pty_adapter_maps_supported_key_press_text_paste_and_resize_on_macos
#[test]
#[ignore = "GAP G-F002-ADV-15"]
#[cfg(target_os = "macos")]
fn pty_adapter_maps_supported_key_press_text_paste_and_resize_on_macos() {
    let trace = trace_file("all_event_types.toml");
    let result = ScenarioRunner::new(RunOptions::for_platform("macos"))
        .run_oracle_trace(&trace)
        .unwrap();
    assert_eq!(result.applied_events(), trace.events());
    assert!(result.readiness_satisfied_before_first_input());
    assert!(result.observations_match());
}

// GAP(G-F002-ADV-16) sev=medium kind=test-gap feature=F-002
//   what:     S4 Windows ConPTY event mapping and live startup lack native coverage.
//   tui-ref:  migration/specs/F-002.md S3-S4; src/event.rs:50-63
//   oracle:   migration/oracle/traces/harness/initial_screen.toml
//   repro:    Replay the same logical event trace using MSVC ConPTY on Windows 10 1809+.
//   expected: Supported events arrive in order and unsupported events fail explicitly.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and Windows runner are absent.
//   cover:    conpty_adapter_maps_supported_events_on_windows
#[test]
#[ignore = "GAP G-F002-ADV-16"]
#[cfg(target_os = "windows")]
fn conpty_adapter_maps_supported_events_on_windows() {
    let trace = trace_file("all_event_types.toml");
    let result = ScenarioRunner::new(RunOptions::for_platform("windows-msvc"))
        .run_oracle_trace(&trace)
        .unwrap();
    assert_eq!(result.applied_events(), trace.events());
    assert!(result.readiness_satisfied_before_first_input());
    assert!(result.observations_match());
}

// GAP-RESOLVED(G-F002-ADV-17) sev=medium kind=test-gap feature=F-002
//   what:     S3-S4 repeat/release phases must fail instead of being synthesized.
//   tui-ref:  migration/specs/F-002.md S3-S4; src/event.rs:52-56
//   oracle:   migration/oracle/traces/harness/reject_repeat.toml and reject_release.toml
//   repro:    Apply repeat and release key events to the TUI adapter.
//   expected: Explicit unsupported-event errors; no synthetic press is delivered.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and TUI adapter are absent.
//   cover:    adapter_fails_explicitly_for_unsupported_repeat_and_release_phases
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn adapter_fails_explicitly_for_unsupported_repeat_and_release_phases() {
    for (trace_name, phase) in [
        ("reject_repeat.toml", "repeat"),
        ("reject_release.toml", "release"),
    ] {
        let trace = trace_file(trace_name);
        let error = ScenarioRunner::new(RunOptions::default())
            .run_oracle_trace(&trace)
            .unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("unsupported") && message.contains(phase),
            "{message}"
        );
        assert_eq!(
            error.input_events_delivered(),
            0,
            "unsupported event was synthesized"
        );
    }
}

// GAP-RESOLVED(G-F002-ADV-18) sev=medium kind=test-gap feature=F-002
//   what:     S5 normalization must strip only terminal controls and declared volatile roots.
//   tui-ref:  migration/specs/F-002.md S5
//   oracle:   migration/oracle/traces/harness/initial_screen.toml
//   repro:    Normalize control sequences and one declared temp root while retaining other paths.
//   expected: Only controls and the declared root are normalized.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and normalizer are absent.
//   cover:    normalizer_removes_only_declared_volatile_roots_and_control_sequences
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn normalizer_removes_only_declared_volatile_roots_and_control_sequences() {
    let screen = "\x1b[2Jroot=/run/a tmp=/tmp/run-a keep=/tmp/other";
    let normalized = normalize_screen(screen, &["/tmp/run-a"]);
    assert!(!normalized.contains("\x1b[2J"));
    assert!(normalized.contains("root=/run/a"));
    assert!(normalized.contains("tmp=<TEMP>"));
    assert!(normalized.contains("keep=/tmp/other"));
}

// GAP-RESOLVED(G-F002-ADV-19) sev=medium kind=test-gap feature=F-002
//   what:     S5 must preserve user-visible names, ordering, selection, and messages.
//   tui-ref:  migration/specs/F-002.md S5
//   oracle:   migration/oracle/traces/harness/initial_screen.toml
//   repro:    Normalize two screens differing only by controls and declared volatile root.
//   expected: Names/order/selection/message remain byte-for-byte equal after normalization.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and normalizer are absent.
//   cover:    normalizer_preserves_names_order_selection_and_messages
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn normalizer_preserves_names_order_selection_and_messages() {
    let a = normalize_screen("alpha.txt\n> beta.txt\nDeleted alpha.txt", &["/tmp/run-a"]);
    let b = normalize_screen("alpha.txt\n> beta.txt\nDeleted alpha.txt", &["/tmp/run-b"]);
    assert!(a.contains("alpha.txt\n> beta.txt"));
    assert!(a.contains("Deleted alpha.txt"));
    assert_eq!(a, b);
}

// GAP-RESOLVED(G-F002-ADV-20) sev=medium kind=test-gap feature=F-002
//   what:     S5 filesystem and persisted observations must remain byte-exact.
//   tui-ref:  migration/specs/F-002.md S5; AGENTS.md invariant 6
//   oracle:   F-002 contract; owning feature captures
//   repro:    Compare byte arrays differing by one byte.
//   expected: Byte comparison detects the difference without normalization.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and comparator are absent.
//   cover:    filesystem_and_persisted_observations_compare_exact_bytes
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn filesystem_and_persisted_observations_compare_exact_bytes() {
    assert!(compare_bytes(b"persisted\0\xff", b"persisted\0\xff").is_ok());
    assert!(compare_bytes(b"persisted\0\xff", b"persisted\0\xfe").is_err());
}

// GAP-RESOLVED(G-F002-ADV-21) sev=medium kind=test-gap feature=F-002
//   what:     S6 mismatch diagnostics lack exact actionable-field coverage.
//   tui-ref:  migration/specs/F-002.md S6
//   oracle:   F-002 schema contract
//   repro:    Compare an expected screen value with a different actual screen.
//   expected: Diagnostic names scenario, observation, expected, and actual.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and comparator are absent.
//   cover:    mismatch_diff_names_scenario_observation_expected_and_actual
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn mismatch_diff_names_scenario_observation_expected_and_actual() {
    let error = compare_screen(
        "resize-small",
        "terminal_screen",
        "expected rows",
        "actual rows",
    )
    .unwrap_err()
    .to_string();
    for part in [
        "resize-small",
        "terminal_screen",
        "expected rows",
        "actual rows",
    ] {
        assert!(error.contains(part), "{error}");
    }
}

// GAP-RESOLVED(G-F002-ADV-22) sev=medium kind=test-gap feature=F-002
//   what:     S6 candidate goldens must only be written to the chosen staging directory.
//   tui-ref:  migration/specs/F-002.md S6, state/effects
//   oracle:   migration/oracle/traces/harness/initial_screen.toml
//   repro:    Capture a candidate to a fresh caller-selected path.
//   expected: Candidate exists under staging; approved capture tree is unchanged.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and golden writer are absent.
//   cover:    golden_capture_writes_only_to_selected_staging_root
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn golden_capture_writes_only_to_selected_staging_root() {
    let temp = tempdir();
    let approved = temp.path().join("approved");
    let staging = temp.path().join("candidate");
    std::fs::create_dir_all(&approved).unwrap();
    let marker = approved.join("keep.toml");
    std::fs::write(&marker, b"approved").unwrap();
    GoldenStore::new(&approved)
        .stage_candidate(&staging, &trace_file("initial_screen.toml"))
        .unwrap();
    assert!(staging.join("initial_screen.toml").is_file());
    assert_eq!(std::fs::read(marker).unwrap(), b"approved");
}

// GAP-RESOLVED(G-F002-ADV-23) sev=medium kind=test-gap feature=F-002
//   what:     S6 ordinary test runs must not update approved goldens.
//   tui-ref:  migration/specs/F-002.md S6, state/effects
//   oracle:   migration/oracle/traces/harness/initial_screen.toml
//   repro:    Run a trace in ordinary mode while an approved golden is present.
//   expected: Approved bytes are unchanged and no candidate is written there.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and runner are absent.
//   cover:    ordinary_run_does_not_modify_approved_goldens
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn ordinary_run_does_not_modify_approved_goldens() {
    let temp = tempdir();
    let golden = temp.path().join("golden.toml");
    std::fs::write(&golden, b"approved-v1").unwrap();
    ScenarioRunner::new(
        RunOptions::default().with_approved_golden_dir(temp.path().join("approved")),
    )
    .run_oracle_trace(&trace_file("initial_screen.toml"))
    .unwrap();
    assert_eq!(std::fs::read(golden).unwrap(), b"approved-v1");
}

// GAP(G-F002-ADV-24) sev=medium kind=test-gap feature=F-002
//   what:     S7 baseline cache identity must bind all specified metadata.
//   tui-ref:  migration/specs/F-002.md S7
//   oracle:   tui-oracle-baseline exact SHA and baseline Cargo.lock
//   repro:    Derive cache keys while varying OS, target, toolchain, lock hash, or profile.
//   expected: Each required identity field changes the key and is present in the sidecar.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and resolver are absent.
//   cover:    baseline_cache_key_and_sidecar_bind_sha_os_target_toolchain_lock_hash_profile
#[test]
#[ignore = "GAP G-F002-ADV-24"]
fn baseline_cache_key_and_sidecar_bind_sha_os_target_toolchain_lock_hash_profile() {
    let resolver =
        BaselineResolver::new(Path::new(env!("CARGO_MANIFEST_DIR"))).with_expected_sha(ORACLE_SHA);
    let metadata: CacheMetadata = resolver.cache_metadata().unwrap();
    for field in [
        "baseline_sha",
        "os",
        "target_triple",
        "rust_toolchain",
        "baseline_lock_hash",
        "build_profile",
    ] {
        assert!(metadata.has_field(field), "missing {field}");
    }
    assert_ne!(
        metadata.cache_key_with_os("linux"),
        metadata.cache_key_with_os("windows")
    );
}

// GAP(G-F002-ADV-25) sev=medium kind=test-gap feature=F-002
//   what:     S7 cache hits must validate every sidecar field and executable digest.
//   tui-ref:  migration/specs/F-002.md S7
//   oracle:   tui-oracle-baseline exact SHA
//   repro:    Corrupt each identity field and then alter executable bytes.
//   expected: Every mismatch invalidates the cache entry and errors or rebuilds exact baseline.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and resolver are absent.
//   cover:    cache_hit_verifies_metadata_and_executable_digest
#[test]
#[ignore = "GAP G-F002-ADV-25"]
fn cache_hit_verifies_metadata_and_executable_digest() {
    let temp = tempdir();
    let mut metadata = CacheMetadata::for_test(ORACLE_SHA);
    let artifact = temp.path().join("ira");
    std::fs::write(&artifact, b"baseline").unwrap();
    metadata.record_executable(&artifact).unwrap();
    assert!(metadata.verify_executable(&artifact).is_ok());
    std::fs::write(&artifact, b"tampered").unwrap();
    assert!(metadata.verify_executable(&artifact).is_err());
    metadata.baseline_sha = "0000000000000000000000000000000000000000".into();
    assert!(metadata.verify_identity(ORACLE_SHA).is_err());
}

// GAP(G-F002-ADV-26) sev=medium kind=test-gap feature=F-002
//   what:     S7 cache misses must build from exact baseline and not current HEAD.
//   tui-ref:  migration/specs/F-002.md S7
//   oracle:   tui-oracle-baseline exact SHA
//   repro:    Resolve an empty cache.
//   expected: A locked baseline build produces a verified binary with the baseline identity.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and resolver are absent.
//   cover:    cache_miss_builds_exact_baseline
#[test]
#[ignore = "GAP G-F002-ADV-26"]
fn cache_miss_builds_exact_baseline() {
    let temp = tempdir();
    let result = BaselineResolver::new(Path::new(env!("CARGO_MANIFEST_DIR")))
        .with_expected_sha(ORACLE_SHA)
        .with_cache_dir(temp.path())
        .resolve_and_build()
        .unwrap();
    assert_eq!(result.commit_sha(), ORACLE_SHA);
    assert!(result.executable_digest().len() >= 64);
}

// GAP(G-F002-ADV-27) sev=medium kind=test-gap feature=F-002
//   what:     S7 detached worktree cleanup is required on every success/failure path.
//   tui-ref:  migration/specs/F-002.md S7
//   oracle:   tui-oracle-baseline exact SHA
//   repro:    Exercise success, cache hit, build failure, timeout, and test failure.
//   expected: Temporary checkout is removed after each exit path and cached binary remains usable.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and resolver are absent.
//   cover:    baseline_worktree_is_removed_after_success_cache_hit_build_failure_timeout_and_test_failure
#[test]
#[ignore = "GAP G-F002-ADV-27"]
fn baseline_worktree_is_removed_after_success_cache_hit_build_failure_timeout_and_test_failure() {
    for exit in [
        "success",
        "cache-hit",
        "build-failure",
        "timeout",
        "test-failure",
    ] {
        let temp = tempdir();
        let resolver = BaselineResolver::new(Path::new(env!("CARGO_MANIFEST_DIR")))
            .with_expected_sha(ORACLE_SHA)
            .with_test_exit_path(exit)
            .with_temp_root(temp.path());
        let _ = resolver.resolve_and_build();
        let worktree = resolver.last_worktree_path_for_test();
        assert!(
            !worktree.exists(),
            "worktree leaked for {exit}: {worktree:?}"
        );
    }
}

// GAP(G-F002-ADV-28) sev=medium kind=test-gap feature=F-002
//   what:     S7 cleanup errors must be surfaced alongside the primary failure.
//   tui-ref:  migration/specs/F-002.md S7-S8
//   oracle:   F-002 lifecycle contract
//   repro:    Cause baseline build failure and worktree-removal failure together.
//   expected: Diagnostic preserves both primary build error and cleanup error.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and resolver are absent.
//   cover:    cleanup_error_is_reported_with_primary_error
#[test]
#[ignore = "GAP G-F002-ADV-28"]
fn cleanup_error_is_reported_with_primary_error() {
    let error = BaselineResolver::new(Path::new(env!("CARGO_MANIFEST_DIR")))
        .with_simultaneous_build_and_cleanup_failure()
        .resolve_and_build()
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("build") && error.contains("cleanup"),
        "{error}"
    );
}

// GAP-FIXED(G-F002-ADV-29) sev=medium kind=test-gap feature=F-002
//   what:     S8 timed-out children must be terminated, reaped, and PTY handles closed.
//   tui-ref:  migration/specs/F-002.md S8
//   oracle:   F-002 lifecycle contract
//   repro:    Run a fake target that never exits before the scenario deadline.
//   expected: Child is terminated/reaped and PTY handles are closed within cleanup deadline.
//   actual:   Prior implementation did not exercise OS child/PTY cleanup.
//   cover:    RED HARNESS CONTRACT: real_pty_session_timeout_kills_and_reaps_helper_child;
//             real_pty_session_closes_master_and_slave_before_fixture_removal
//   fixed-by: see migration/reports/F-002/dev-lifecycle-1.md
#[test]
fn deadline_terminates_reaps_child_and_closes_pty() {
    // Keep the orchestration assertion alongside the real child-backed
    // lifecycle contract in f002_lifecycle_session.rs.
    let mut target = ScriptedTarget::hanging_child();
    let result = ScenarioRunner::new(RunOptions::with_deadlines(
        Duration::from_millis(25),
        Duration::from_secs(1),
    ))
    .run_with_target(&trace_file("initial_screen.toml"), &mut target);
    assert!(result.unwrap_err().is_timeout());
    assert!(target.child_was_reaped());
    assert!(target.pty_handles_are_closed());
}

// GAP-FIXED(G-F002-ADV-30) sev=medium kind=test-gap feature=F-002
//   what:     S8 cleanup must drain/join output readers before removing the fixture.
//   tui-ref:  migration/specs/F-002.md S8
//   oracle:   F-002 lifecycle contract
//   repro:    Timeout a child while stdout/stderr readers hold fixture-associated handles.
//   expected: Readers are joined and handles closed before fixture deletion.
//   actual:   Prior implementation did not join the real PTY output reader before fixture cleanup.
//   cover:    RED HARNESS CONTRACT: real_pty_session_joins_reader_before_fixture_removal;
//             real_pty_session_preserves_primary_and_cleanup_errors
//   fixed-by: see migration/reports/F-002/dev-lifecycle-1.md
#[test]
fn timeout_drains_and_joins_readers_before_fixture_removal() {
    // Keep the orchestration assertion alongside the real child-backed
    // lifecycle contract in f002_lifecycle_session.rs.
    let mut target = ScriptedTarget::hanging_with_open_readers();
    let result = ScenarioRunner::new(RunOptions::with_deadlines(
        Duration::from_millis(25),
        Duration::from_secs(1),
    ))
    .run_with_target(&trace_file("initial_screen.toml"), &mut target);
    assert!(result.unwrap_err().is_timeout());
    assert!(target.output_readers_joined());
    assert!(target.fixture_removed_after_readers_joined());
}

// GAP-RESOLVED(G-F002-ADV-31) sev=medium kind=test-gap feature=F-002
//   what:     Runner readiness has no evidence that it waits on the trace's declared observation kind.
//   tui-ref:  migration/specs/F-002.md S1, S3, S8; src/main.rs:26-56
//   oracle:   migration/oracle/traces/harness/initial_screen.toml
//   repro:    Declare filesystem readiness while a pending input is present.
//   expected: Runner rejects or waits for the declared filesystem observation before applying input.
//   actual:   At initial review, runner accepted readiness without comparing the declared filesystem observation.
//   cover:    readiness_uses_declared_observation_kind_before_input
//   reopened: prior test trusted hardcoded ScriptedTarget readiness booleans; it never inspected observation kind.
//   verified-by: adversarial reviewer 2026-10-01 against 62db2ce; filesystem bytes mismatching condition prevent input
#[test]
fn readiness_uses_declared_observation_kind_before_input() {
    let source = minimal_trace().replace(
        "observation = { kind = \"terminal_screen\" }",
        "observation = { kind = \"filesystem\", relative_path = \".\" }",
    );
    let trace = parse(&source).unwrap().with_events(vec![InputEvent::Key {
        code: KeyCode::Character { character: 'q' },
        modifiers: BTreeSet::new(),
        phase: KeyPhase::Press,
    }]);
    let mut target = ScriptedTarget::recording_filesystem(b"actual fixture bytes".to_vec());
    let result = ScenarioRunner::new(RunOptions::default()).run_with_target(&trace, &mut target);
    assert!(
        result.is_err(),
        "runner must not bypass unavailable filesystem readiness"
    );
    assert!(
        target.applied_events().is_empty(),
        "input was sent before declared readiness"
    );
    assert!(target
        .observed_kinds()
        .contains(&ObservationKind::Filesystem {
            relative_path: ".".into()
        }));
    assert!(target.observations().iter().any(|(kind, observation)| {
        kind == &ObservationKind::Filesystem {
            relative_path: ".".into(),
        } && observation.bytes.as_deref() == Some(b"actual fixture bytes")
    }));
}

// GAP-RESOLVED(G-F002-ADV-32) sev=medium kind=test-gap feature=F-002
//   what:     S8 must send no event if the observation-based readiness deadline expires.
//   tui-ref:  migration/specs/F-002.md S1, S3, S8
//   oracle:   migration/oracle/traces/harness/initial_screen.toml
//   repro:    Use a target that never produces the readiness observation with a pending key.
//   expected: Readiness timeout occurs and target's applied-event list remains empty.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and runner are absent.
//   cover:    readiness_timeout_sends_no_input
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn readiness_timeout_sends_no_input() {
    let trace = trace_file("initial_screen.toml").with_events(vec![InputEvent::Key {
        code: KeyCode::Character { character: 'q' },
        modifiers: BTreeSet::new(),
        phase: KeyPhase::Press,
    }]);
    let mut target = ScriptedTarget::never_ready();
    let result = ScenarioRunner::new(RunOptions::with_readiness_deadline(Duration::from_millis(
        25,
    )))
    .run_with_target(&trace, &mut target);
    assert!(result.unwrap_err().is_readiness_timeout());
    assert!(target.applied_events().is_empty());
}

// GAP-RESOLVED(G-F002-ADV-33) sev=medium kind=test-gap feature=F-002
//   what:     S9 poisoned Unix environment must not redirect protected roots.
//   tui-ref:  migration/specs/F-002.md S9, config influence/persistence
//   oracle:   F-002 environment contract
//   repro:    Poison parent HOME, XDG roots, IRA variables, and terminal host variables.
//   expected: Child uses only isolated roots within scenario root and allowlisted variables.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and environment builder are absent.
//   cover:    poisoned_parent_environment_cannot_escape_isolated_roots_unix
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
#[cfg(unix)]
fn poisoned_parent_environment_cannot_escape_isolated_roots_unix() {
    let policy = EnvironmentPolicy::unix_for_test();
    let env = ChildEnvironment::build(
        &policy,
        "/tmp/run-root",
        [
            ("HOME", "/host/home"),
            ("IRA_IMAGES", "kitty"),
            ("TMUX", "host"),
        ],
    )
    .unwrap();
    for key in [
        "HOME",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_DATA_HOME",
        "TMPDIR",
    ] {
        assert!(env.path_is_within_root(key), "{key} escaped");
    }
    assert!(env.get("IRA_IMAGES").is_none());
    assert!(env.get("TMUX").is_none());
}

// GAP(G-F002-ADV-34) sev=medium kind=test-gap feature=F-002
//   what:     S9 poisoned Windows environment isolation lacks Windows profile coverage.
//   tui-ref:  migration/specs/F-002.md S9
//   oracle:   F-002 environment contract
//   repro:    Poison USERPROFILE, APPDATA, LOCALAPPDATA, TEMP, TMP, and IRA variables.
//   expected: Protected Windows paths remain under run root; documented system vars survive.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and environment builder are absent.
//   cover:    poisoned_parent_environment_cannot_escape_isolated_roots_windows
#[test]
#[ignore = "GAP G-F002-ADV-34"]
#[cfg(target_os = "windows")]
fn poisoned_parent_environment_cannot_escape_isolated_roots_windows() {
    let policy = EnvironmentPolicy::windows_for_test();
    let env = ChildEnvironment::build(
        &policy,
        r"C:\run-root",
        [
            ("APPDATA", r"C:\host\appdata"),
            ("TEMP", r"C:\host\temp"),
            ("IRA_IMAGES", "kitty"),
        ],
    )
    .unwrap();
    for key in ["USERPROFILE", "APPDATA", "LOCALAPPDATA", "TEMP", "TMP"] {
        assert!(env.path_is_within_root(key), "{key} escaped");
    }
    assert!(env.get("SystemRoot").is_some());
    assert!(env.get("IRA_IMAGES").is_none());
}

// GAP-RESOLVED(G-F002-ADV-35) sev=medium kind=test-gap feature=F-002
//   what:     S9 scenario overrides may not escape protected roots after path resolution.
//   tui-ref:  migration/specs/F-002.md S9
//   oracle:   F-002 environment contract
//   repro:    Override a protected root with an absolute outside path and a traversal path.
//   expected: Both overrides are rejected before child launch.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and environment builder are absent.
//   cover:    scenario_override_cannot_escape_protected_roots
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn scenario_override_cannot_escape_protected_roots() {
    for value in ["/outside/home", "../../outside/tmp"] {
        let error = ChildEnvironment::build_with_overrides(
            &EnvironmentPolicy::unix_for_test(),
            "/tmp/run-root",
            [("HOME", value)],
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("protected") || error.contains("run root"),
            "{error}"
        );
    }
}

// GAP-RESOLVED(G-F002-ADV-36) sev=medium kind=test-gap feature=F-002
//   what:     S9 protected-root canonicalization must reject external symlink escapes.
//   tui-ref:  migration/specs/F-002.md S9
//   oracle:   F-002 environment contract
//   repro:    Symlink a protected child root to a directory outside the scenario root.
//   expected: Environment validation fails closed and does not follow the escape.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and environment builder are absent.
//   cover:    protected_root_symlink_escape_is_rejected
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn protected_root_symlink_escape_is_rejected() {
    let temp = tempdir();
    let root = temp.path().join("run");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("home")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&outside, root.join("home")).unwrap();
    let error = ChildEnvironment::build_with_root(&EnvironmentPolicy::host(), &root)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("escape") || error.contains("protected"),
        "{error}"
    );
}

// GAP(G-F002-ADV-37) sev=medium kind=test-gap feature=F-002
//   what:     S9 environment allowlist must exclude host IRA/terminal variables.
//   tui-ref:  migration/specs/F-002.md S9
//   oracle:   F-002 environment contract
//   repro:    Seed parent process with IRA_*, TERM_PROGRAM, TMUX, and SSH_TTY.
//   expected: Child receives only documented OS-required variables and isolated roots.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and environment builder are absent.
//   cover:    only_documented_os_environment_is_inherited
#[test]
#[ignore = "GAP G-F002-ADV-37"]
#[cfg(unix)]
fn only_documented_os_environment_is_inherited() {
    let env = ChildEnvironment::build(
        &EnvironmentPolicy::unix_for_test(),
        "/tmp/run-root",
        [
            ("IRA_CONFIG", "/host/config"),
            ("TERM_PROGRAM", "host-terminal"),
            ("TMUX", "host-tmux"),
            ("SSH_TTY", "/dev/pts/9"),
        ],
    )
    .unwrap();
    for key in ["IRA_CONFIG", "TERM_PROGRAM", "TMUX", "SSH_TTY"] {
        assert!(env.get(key).is_none(), "unexpected inherited {key}");
    }
}

// GAP-RESOLVED(G-F002-ADV-38) sev=medium kind=test-gap feature=F-002
//   what:     S10 staged golden metadata serialization is not verified against required fields and values.
//   tui-ref:  migration/specs/F-002.md S10
//   oracle:   migration/oracle/traces/harness/initial_screen.toml
//   repro:    Stage a golden candidate and inspect the written metadata.toml.
//   expected: Serialized metadata includes every required field and exact baseline SHA.
//   actual:   Prior test only called has_field(), which checks a static allowlist and never reads serialization.
//   cover:    golden_metadata_requires_oracle_sha_scenario_fixture_dimensions_events_os_and_date
//   reopened: previous assertion used GoldenMetadata::has_field's constant list, not staged serialized metadata.
//   verified-by: adversarial reviewer 2026-10-01 against 62db2ce; staged TOML is checked field-by-field and value-by-value
#[test]
fn golden_metadata_requires_oracle_sha_scenario_fixture_dimensions_events_os_and_date() {
    let trace = trace_file("initial_screen.toml").with_events(vec![InputEvent::Text {
        value: "golden metadata event".into(),
    }]);
    let temp = tempdir();
    let approved = temp.path().join("approved");
    let stage = temp.path().join("stage");
    GoldenStore::new(&approved)
        .capture_to_staging(&stage, &trace)
        .unwrap();
    let bytes = std::fs::read_to_string(stage.join("metadata.toml")).unwrap();
    let serialized: toml::Value = toml::from_str(&bytes).unwrap();
    let table = serialized.as_table().expect("metadata is a TOML table");
    for field in [
        "oracle_sha",
        "scenario_id",
        "fixture",
        "dimensions",
        "events",
        "os_profile",
        "capture_date",
    ] {
        assert!(
            table.contains_key(field),
            "serialized metadata missing {field}: {bytes}"
        );
    }
    assert_eq!(table["oracle_sha"].as_str(), Some(ORACLE_SHA));
    assert_eq!(
        table["scenario_id"].as_str(),
        Some(trace.scenario_id.as_str())
    );
    assert_eq!(table["fixture"].as_str(), Some(trace.fixture.kind.as_str()));
    assert_eq!(table["dimensions"].as_str(), Some("90x24"));
    let expected_events = format!("{:?}", trace.events());
    assert_eq!(table["events"].as_str(), Some(expected_events.as_str()));
    let expected_os = if cfg!(target_os = "windows") {
        "windows-msvc"
    } else {
        std::env::consts::OS
    };
    assert_eq!(table["os_profile"].as_str(), Some(expected_os));
    let capture_date = table["capture_date"]
        .as_str()
        .expect("capture date is text");
    assert!(
        capture_date.len() == 10
            && capture_date.as_bytes()[4] == b'-'
            && capture_date.as_bytes()[7] == b'-'
            && capture_date
                .bytes()
                .enumerate()
                .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit()),
        "invalid capture date: {capture_date:?}"
    );
}

// GAP-RESOLVED(G-F002-ADV-39) sev=medium kind=test-gap feature=F-002
//   what:     S10 golden capture must stage candidates without replacing approved captures.
//   tui-ref:  migration/specs/F-002.md S6, S10
//   oracle:   migration/oracle/traces/harness/initial_screen.toml
//   repro:    Stage a candidate while an approved trace/golden is present.
//   expected: Existing evidence bytes remain unchanged; candidate is separate and unapproved.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and golden implementation are absent.
//   cover:    golden_capture_stages_without_replacing_approved_capture
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn golden_capture_stages_without_replacing_approved_capture() {
    let temp = tempdir();
    let approved = temp.path().join("approved");
    let staging = temp.path().join("stage");
    std::fs::create_dir_all(&approved).unwrap();
    let original = approved.join("screen.golden");
    std::fs::write(&original, b"approved bytes").unwrap();
    GoldenStore::new(&approved)
        .capture_to_staging(&staging, &trace_file("initial_screen.toml"))
        .unwrap();
    assert_eq!(std::fs::read(original).unwrap(), b"approved bytes");
    assert!(staging.join("initial_screen.toml").exists());
    assert_eq!(
        GoldenMetadata::load_from_staging(&staging)
            .unwrap()
            .review_status(),
        "pending_logic_review"
    );
}

// GAP(G-F002-ADV-40) sev=medium kind=test-gap feature=F-002
//   what:     S11 lacks live tagged-oracle replay coverage on each native runner.
//   tui-ref:  migration/specs/F-002.md S11; F-150 platform obligation
//   oracle:   migration/oracle/traces/harness/resize_small.toml
//   repro:    Replay a readiness-gated resize trace against the exact baseline on Linux/macOS/Windows.
//   expected: Screen observation matches the approved trace on every native runner, without skip.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and native harness jobs are absent.
//   cover:    tagged_oracle_trace_replays_on_linux_macos_windows
//   fixed-by: 176d6de
//   reopened: Only Linux was run locally; required native macOS and Windows live-replay evidence was not inspected.
#[test]
fn tagged_oracle_trace_replays_on_linux_macos_windows() {
    let trace = trace_file("resize_small.toml");
    let result = ScenarioRunner::new(RunOptions::for_current_platform())
        .run_oracle_trace(&trace)
        .unwrap();
    assert!(result.readiness_satisfied_before_first_input());
    assert!(result.observations_match());
    assert!(result.baseline_sha() == ORACLE_SHA);
}

// GAP-RESOLVED(G-F002-ADV-41) sev=medium kind=test-gap feature=F-002
//   what:     S11 malformed traces must fail before starting any child process.
//   tui-ref:  migration/specs/F-002.md S1, S11
//   oracle:   F-002 schema contract
//   repro:    Run a trace with no required readiness assertion.
//   expected: Parser/validator reports the malformed field and child start count remains zero.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and runner are absent.
//   cover:    malformed_trace_fails_before_child_start
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn malformed_trace_fails_before_child_start() {
    let malformed = minimal_trace().replace("[readiness]", "[missing_readiness]");
    let mut target = ScriptedTarget::counting_starts();
    assert!(ScenarioRunner::new(RunOptions::default())
        .run_source_with_target(&malformed, &mut target)
        .is_err());
    assert_eq!(target.start_count(), 0);
}

// GAP(G-F002-ADV-42) sev=medium kind=test-gap feature=F-002
//   what:     S11 parallel scenarios must not share mutable fixtures or terminal sessions.
//   tui-ref:  migration/specs/F-002.md S11; errors/edge cases
//   oracle:   migration/oracle/traces/harness/initial_screen.toml
//   repro:    Run two copies of a trace concurrently.
//   expected: Each run has a unique fixture root and PTY session.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and runner are absent.
//   cover:    parallel_scenarios_have_disjoint_roots_and_sessions
#[test]
#[ignore = "GAP G-F002-ADV-42"]
fn parallel_scenarios_have_disjoint_roots_and_sessions() {
    let trace = trace_file("initial_screen.toml");
    let (a, b) = ScenarioRunner::new(RunOptions::default())
        .run_pair(&trace, &trace)
        .unwrap();
    assert_ne!(a.fixture_root(), b.fixture_root());
    assert_ne!(a.terminal_session_id(), b.terminal_session_id());
    assert!(a.fixture_removed() && b.fixture_removed());
}

// GAP-RESOLVED(G-F002-ADV-43) sev=medium kind=test-gap feature=F-002
//   what:     S11 observation mismatch detection lacks mutation-sensitivity evidence.
//   tui-ref:  migration/specs/F-002.md S6, S11
//   oracle:   migration/oracle/traces/harness/initial_screen.toml
//   repro:    Change one observed screen value while keeping the expected value fixed.
//   expected: Comparison fails and reports the changed observation.
//   actual:   Blocked because tools/ira-parity/Cargo.toml and comparator are absent.
//   cover:    changed_observation_fails_assertion_mutation
//   fixed-by: 176d6de
//   verified-by: adversarial reviewer 2026-10-01 against 46811ea
#[test]
fn changed_observation_fails_assertion_mutation() {
    let expected = "Common folders\nActions";
    let actual = "Common folders\nActionz";
    let result = compare_screen("initial-screen", "terminal_screen", expected, actual);
    assert!(
        result.is_err(),
        "a changed observed value must fail the assertion"
    );
}
