//! Independent logic-review probes for the F-002 harness contract.

use ira_parity::{
    baseline::BaselineResolver,
    environment::{ChildEnvironment, EnvironmentPolicy},
    golden::{GoldenMetadata, GoldenStore},
    normalize::normalize_screen,
    runner::{RunOptions, ScenarioRunner},
    testing::ScriptedTarget,
    trace::{parse_trace, InputEvent, KeyCode, KeyPhase, NamedKey, ObservationKind},
};
use std::process::Command;
use std::{collections::BTreeSet, fs, path::Path};

// GAP-RESOLVED(G-F002-LOG-01) sev=high kind=behavior-divergence feature=F-002
//   what:     The oracle runner treats expected non-screen observations as matched without comparing them.
//   tui-ref:  migration/specs/F-002.md S3, S5, S6; src/main.rs:65-68 (process event effects)
//   oracle:   migration/oracle/traces/harness/initial_screen.toml (trace schema/contract)
//   repro:    Declare process.exit_code=1 while ScriptedTarget returns no process exit code.
//   expected: The declared process assertion mismatches (or fails with a diagnostic naming process).
//   actual:   Runner reports observations_match=true because expected_observations_match only inspects screen text and dimensions.
//   cover:    process_observation_assertions_are_compared
//   verified-by: logic-reviewer 2026-10-01; 62db2ce; process-observation probe passes and mismatch diagnostic is emitted.
#[test]
fn process_observation_assertions_are_compared() {
    let source = include_str!("../../../migration/oracle/traces/harness/initial_screen.toml")
        .replace(
            "kind = \"terminal_screen\"\nexpect = { contains = [\"Common folders\"] }",
            "kind = \"process\"\nexpect = { exit_code = 1 }",
        );
    let trace = parse_trace(&source).expect("valid process observation trace");
    assert_eq!(trace.observation_kinds(), vec![ObservationKind::Process]);
    let mut target = ScriptedTarget::recording();
    let result = ScenarioRunner::new(RunOptions::default())
        .run_with_target(&trace, &mut target)
        .expect("scripted run itself succeeds");
    assert!(
        !result.observations_match(),
        "missing/mismatching process observation must fail"
    );
}

// GAP-RESOLVED(G-F002-LOG-02) sev=medium kind=behavior-divergence feature=F-002
//   what:     CSI sequences terminated by nonalphabetic finals erase the first character of following UI text.
//   tui-ref:  migration/specs/F-002.md S5; src/tui.rs:62-67 (terminal control sequences)
//   oracle:   ANSI/VT screen stream emitted by the frozen TUI
//   repro:    Normalize ESC [ 3 ~ followed by the literal message "Message".
//   expected: The delete-key CSI sequence is removed and the full message remains.
//   actual:   The normalizer treats '~' as payload until 'M', dropping that first message character.
//   cover:    normalizer_preserves_text_after_csi_tilde_final
//   verified-by: logic-reviewer 2026-10-01; 62db2ce; CSI tilde-final probe passes.
#[test]
fn normalizer_preserves_text_after_csi_tilde_final() {
    assert_eq!(normalize_screen("\x1b[3~Message", &[]), "Message");
}

// GAP-RESOLVED(G-F002-LOG-03) sev=high kind=security feature=F-002
//   what:     A protected-root override through a symlink can create a directory outside the scenario root before rejection.
//   tui-ref:  migration/specs/F-002.md S9; environment/config isolation contract
//   oracle:   isolated scenario root policy in F-002 S9
//   repro:    Point HOME at a nonexistent child of a symlink inside the run root that resolves outside it.
//   expected: Reject before following the symlink or creating anything outside the run root.
//   actual:   Canonicalization falls back to lexical containment for the missing leaf; create_dir_all follows the symlink, then later containment validation rejects.
//   cover:    symlink_parent_escape_has_no_external_side_effect
//   verified-by: logic-reviewer 2026-10-01; 62db2ce; symlink-parent escape rejected without creating the outside leaf.
#[cfg(unix)]
#[test]
fn symlink_parent_escape_has_no_external_side_effect() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("scenario");
    let external = temp.path().join("external");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&external).unwrap();
    std::os::unix::fs::symlink(&external, root.join("redirect")).unwrap();

    let result = ChildEnvironment::build_with_overrides(
        &EnvironmentPolicy::unix_for_test(),
        &root,
        [("HOME", "redirect/created-outside")],
    );
    assert!(result.is_err(), "escaped protected root must be rejected");
    assert!(
        !external.join("created-outside").exists(),
        "containment must be checked before creating the external path"
    );
}

// GAP-RESOLVED(G-F002-LOG-04) sev=high kind=behavior-divergence feature=F-002
//   what:     Baseline resolution fails immediately when the pinned tag is absent locally instead of fetching it.
//   tui-ref:  migration/specs/F-002.md S2, S7 (baseline acquisition from immutable tag)
//   oracle:   tui-oracle-baseline tag peeled to 1cad4ce43cc72d52d4cc4eef920e0da22cb69568
//   repro:    Clone a remote with --no-tags, then resolve the known tag through BaselineResolver.
//   expected: Resolver fetches the baseline tag, verifies its peeled SHA, and proceeds.
//   actual:   resolve() returns "tag ... not found" without attempting fetch.
//   cover:    baseline_resolution_fetches_missing_pinned_tag
//   verified-by: logic-reviewer 2026-10-01; 62db2ce; missing-tag fetch probe passes and verifies the peeled SHA.
#[test]
fn baseline_resolution_fetches_missing_pinned_tag() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let bare = temp.path().join("remote.git");
    let clone = temp.path().join("clone");
    std::fs::create_dir_all(&source).unwrap();
    let run = |dir: &std::path::Path, args: &[&str]| {
        let result = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        result
    };
    run(&source, &["init"]);
    run(
        &source,
        &["config", "user.email", "logic-review@example.invalid"],
    );
    run(&source, &["config", "user.name", "Logic Review"]);
    std::fs::write(source.join("file"), "baseline").unwrap();
    run(&source, &["add", "file"]);
    run(&source, &["commit", "-m", "baseline"]);
    let sha = String::from_utf8_lossy(&run(&source, &["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();
    run(&source, &["tag", "tui-oracle-baseline"]);
    assert!(Command::new("git")
        .args(["init", "--bare"])
        .arg(&bare)
        .output()
        .unwrap()
        .status
        .success());
    run(
        &source,
        &["remote", "add", "origin", bare.to_str().unwrap()],
    );
    run(&source, &["push", "origin", "HEAD", "--tags"]);
    let clone_result = Command::new("git")
        .args(["clone", "--no-tags"])
        .arg(&bare)
        .arg(&clone)
        .output()
        .unwrap();
    assert!(
        clone_result.status.success(),
        "git clone --no-tags failed: {}",
        String::from_utf8_lossy(&clone_result.stderr)
    );
    assert!(!clone.join(".git/refs/tags/tui-oracle-baseline").exists());

    let resolved = BaselineResolver::new(&clone)
        .with_expected_sha(&sha)
        .resolve();
    assert!(
        resolved.is_ok(),
        "missing pinned tag should be fetched: {resolved:?}"
    );
}

#[test]
fn repeated_expected_observations_are_all_checked() {
    let source = format!(
        "{}\n[[observations]]\nkind = \"terminal_screen\"\nexpect = {{ contains = [\"Common folders\"] }}\n\
         [[observations]]\nkind = \"terminal_screen\"\nexpect = {{ contains = [\"not present\"] }}\n",
        include_str!("../../../migration/oracle/traces/harness/initial_screen.toml")
    );
    let trace = parse_trace(&source).expect("valid trace with repeated observations");
    let mut target = ScriptedTarget::recording();
    let result = ScenarioRunner::new(RunOptions::default())
        .run_with_target(&trace, &mut target)
        .expect("scenario runs even when an observation assertion mismatches");
    assert!(!result.observations_match());
    assert!(result
        .observation_diagnostics()
        .iter()
        .any(|message| message.contains("not present")));
}

#[test]
fn readiness_uses_the_declared_process_observation() {
    let source = include_str!("../../../migration/oracle/traces/harness/initial_screen.toml")
        .replace(
            "observation = { kind = \"terminal_screen\" }\ncondition = { contains = [\"Common folders\"] }",
            "observation = { kind = \"process\" }\ncondition = { contains = [\"stdout-marker\"] }",
        );
    let trace = parse_trace(&source)
        .expect("valid process-readiness trace")
        .with_events(vec![InputEvent::Text {
            value: "after-ready".into(),
        }]);
    let mut target = ScriptedTarget::writing_stream_markers("unused", "unused");
    let result = ScenarioRunner::new(RunOptions::default())
        .run_with_target(&trace, &mut target)
        .expect("process output satisfies process readiness");
    assert!(result.readiness_satisfied_before_first_input());
    assert!(result.observations_match());
    assert_eq!(
        target.applied_events(),
        &[InputEvent::Text {
            value: "after-ready".into()
        }]
    );
    assert_eq!(
        target.observed_kinds().first(),
        Some(&ObservationKind::Process)
    );
}

fn staged_files(root: &Path) -> Vec<Vec<u8>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(staged_files(&path));
        } else {
            files.push(fs::read(path).unwrap());
        }
    }
    files
}

// GAP-FIXED(G-F002-LOG-06) sev=high kind=missing-feature feature=F-002
//   fixed-by: typed actual observation bundles and explicit bracketed paste protocol (T-006)
//   what:     Golden staging writes the declared input trace but loses observations collected by the target.
//   tui-ref:  migration/specs/F-002.md S6 and S10; migration/oracle/traces/harness/initial_screen.toml
//   oracle:   live TraceTarget observation from the declared terminal_screen assertion
//   repro:    Run a trace against ScriptedTarget (screen="Common folders\\nActions") and stage its candidate.
//   expected: Staging contains the captured screen output as well as oracle/scenario/fixture/dimensions/event metadata.
//   actual:   GoldenStore::capture_to_staging accepts only Trace and serializes trace.to_toml(); captured "Actions" is absent.
//   cover:    golden_candidate_contains_captured_observations_and_input_evidence
#[test]
fn golden_candidate_contains_captured_observations_and_input_evidence() {
    let mut trace = parse_trace(include_str!(
        "../../../migration/oracle/traces/harness/initial_screen.toml"
    ))
    .unwrap();
    trace = trace.with_events(vec![InputEvent::Key {
        code: KeyCode::Named(NamedKey::Escape),
        modifiers: BTreeSet::new(),
        phase: KeyPhase::Press,
    }]);
    let mut target = ScriptedTarget::recording();
    let run = ScenarioRunner::new(RunOptions::default())
        .run_capture_with_target(&trace, &mut target)
        .unwrap();
    assert!(run.result.observations_match());

    let temp = tempfile::tempdir().unwrap();
    let staging = temp.path().join("candidate");
    GoldenStore::new(temp.path().join("approved"))
        .capture_run_to_staging(&staging, &run)
        .unwrap();
    let metadata = GoldenMetadata::load_from_staging(&staging).unwrap();
    assert!(
        metadata.events.contains("Escape"),
        "logical event evidence must be retained"
    );
    assert!(
        staged_files(&staging)
            .iter()
            .any(|contents| String::from_utf8_lossy(contents).contains("Actions")),
        "candidate must include the target's captured screen, not only trace expectations"
    );
}
