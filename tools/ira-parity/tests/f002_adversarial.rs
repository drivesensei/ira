//! Adversarial probes for F-002 runner contracts.
use ira_parity::{
    runner::{RunOptions, ScenarioRunner},
    testing::ScriptedTarget,
    trace::{parse_trace, ObservationKind},
};

const INITIAL: &str = include_str!("../../../migration/oracle/traces/harness/initial_screen.toml");

// GAP(G-F002-ADV-49) sev=medium kind=edge-case feature=F-002
//   what:     Terminal normalizer leaks OSC title bytes into the user-visible screen.
//   tui-ref:  migration/specs/F-002.md S5
//   oracle:   terminal control sequence semantics
//   repro:    Normalize a screen prefixed by OSC 0 title terminated with BEL.
//   expected: OSC title control sequence is removed and screen content remains intact.
//   actual:   Escape parser stops at the first alphabetic OSC payload character and leaks the suffix.
//   cover:    normalizer_discards_osc_title_sequences
#[test]
fn normalizer_discards_osc_title_sequences() {
    let actual =
        ira_parity::normalize::normalize_screen("\x1b]0;host-title\x07Common folders", &[]);
    assert_eq!(actual, "Common folders");
}

// GAP(G-F002-ADV-44) sev=high kind=behavior-divergence feature=F-002
//   what:     Runner reports observations_match even when a declared observation's expected content is absent.
//   tui-ref:  migration/specs/F-002.md S1, S3, S5-S6
//   oracle:   migration/oracle/traces/harness/initial_screen.toml
//   repro:    Declare terminal_screen contains text that the target does not render.
//   expected: Scenario comparison fails and identifies the mismatched observation.
//   actual:   Runner ignores ExpectedObservation.expect and reports a match.
//   cover:    expected_observation_content_is_compared
#[test]
fn expected_observation_content_is_compared() {
    let source = format!(
        "{INITIAL}\n[[observations]]\nkind = \"terminal_screen\"\nexpect = {{ contains = [\"must-not-match\"] }}\n"
    );
    let trace = parse_trace(&source).unwrap();
    let mut target = ScriptedTarget::recording();
    let result = ScenarioRunner::new(RunOptions::default())
        .run_with_target(&trace, &mut target)
        .unwrap();
    assert!(!result.observations_match());
}

// GAP(G-F002-ADV-45) sev=medium kind=platform feature=F-002
//   what:     Trace platform profiles are not checked against the runner's selected platform.
//   tui-ref:  migration/specs/F-002.md S1, S4, S11
//   oracle:   F-002 platform profile contract
//   repro:    Run a macOS-only trace with RunOptions::for_platform("linux").
//   expected: Runner rejects the trace before target.start.
//   actual:   Runner starts the target and executes it despite incompatible profile.
//   cover:    incompatible_trace_platform_is_rejected_before_start
#[test]
fn incompatible_trace_platform_is_rejected_before_start() {
    let source = INITIAL.replace(
        "platforms = [\"linux\", \"macos\", \"windows-msvc\"]",
        "platforms = [\"macos\"]",
    );
    let trace = parse_trace(&source).unwrap();
    let mut target = ScriptedTarget::counting_starts();
    let result =
        ScenarioRunner::new(RunOptions::for_platform("linux")).run_with_target(&trace, &mut target);
    assert!(result.is_err(), "incompatible platform unexpectedly ran");
    assert_eq!(target.start_count(), 0);
}

// GAP(G-F002-ADV-46) sev=high kind=behavior-divergence feature=F-002
//   what:     Runner accepts a trace whose declared readiness kind differs from the target observation contract.
//   tui-ref:  migration/specs/F-002.md S1, S3, S8
//   oracle:   F-002 readiness contract
//   repro:    Declare filesystem readiness while only terminal_screen is observed.
//   expected: The readiness observation itself is sampled and its condition gates event delivery.
//   actual:   Runner delegates readiness without checking Readiness.observation and may send input early.
//   cover:    readiness_observation_kind_is_enforced
#[test]
fn readiness_observation_kind_is_enforced() {
    let source = INITIAL.replace(
        "observation = { kind = \"terminal_screen\" }",
        "observation = { kind = \"filesystem\", relative_path = \".\" }",
    );
    let trace = parse_trace(&source).unwrap();
    let mut target = ScriptedTarget::recording();
    let result = ScenarioRunner::new(RunOptions::default())
        .run_with_target(&trace, &mut target)
        .unwrap();
    assert!(
        target
            .observed_kinds()
            .contains(&ObservationKind::TerminalScreen),
        "expected trace to observe terminal screen"
    );
    assert!(
        !result.readiness_satisfied_before_first_input(),
        "filesystem readiness was never verified"
    );
}

#[test]
fn successful_baseline_build_drop_removes_real_worktree() {
    use ira_parity::baseline::{BaselineResolver, ORACLE_SHA};
    let cache = tempfile::tempdir().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let resolver = BaselineResolver::new(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
        .with_expected_sha(ORACLE_SHA)
        .with_cache_dir(cache.path())
        .with_temp_root(temp.path());
    let baseline = resolver
        .resolve_and_build()
        .expect("real baseline build succeeds");
    assert_eq!(baseline.commit_sha(), ORACLE_SHA);
    let first_build_count = baseline.build_count();
    assert!(baseline.executable_path().is_file());
    assert!(baseline.worktree_is_detached_and_clean());
    let worktree = baseline.worktree_path().to_path_buf();
    drop(baseline);
    assert!(
        !worktree.exists(),
        "successful build leaked worktree {worktree:?}"
    );

    let cached = resolver
        .resolve_and_build()
        .expect("verified cache hit succeeds");
    assert_eq!(cached.commit_sha(), ORACLE_SHA);
    assert_eq!(
        cached.build_count(),
        first_build_count,
        "cache hit unexpectedly rebuilt oracle"
    );
    assert!(
        cached.executable_path().is_file(),
        "cached executable disappeared with worktree"
    );
    assert!(cached.worktree_is_detached_and_clean());
    let cached_worktree = cached.worktree_path().to_path_buf();
    drop(cached);
    assert!(
        !cached_worktree.exists(),
        "cache-hit path leaked worktree {cached_worktree:?}"
    );
}

// GAP(G-F002-ADV-50) sev=medium kind=edge-case feature=F-002
//   what:     Baseline tag resolution uses an unqualified ref and can be shadowed by a conflicting refs/<tag>.
//   tui-ref:  migration/specs/F-002.md S2, S7
//   oracle:   tui-oracle-baseline exact peeled tag SHA
//   repro:    Create tag `oracle` at commit A and direct ref `refs/oracle` at commit B, then resolve `oracle`.
//   expected: Resolver reads refs/tags/oracle and accepts commit A independent of other refs.
//   actual:   `rev-parse oracle^{commit}` resolves refs/oracle first and rejects the correct tag as wrong SHA.
//   cover:    baseline_resolution_is_qualified_to_tag_namespace
#[test]
fn baseline_resolution_is_qualified_to_tag_namespace() {
    use ira_parity::baseline::BaselineResolver;
    use std::{path::Path, process::Command};

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "--quiet"]);
    git(repo.path(), &["config", "user.name", "review"]);
    git(
        repo.path(),
        &["config", "user.email", "review@example.invalid"],
    );
    std::fs::write(repo.path().join("content"), "tagged baseline\n").unwrap();
    git(repo.path(), &["add", "content"]);
    git(repo.path(), &["commit", "--quiet", "-m", "tagged baseline"]);
    let tag_sha = git(repo.path(), &["rev-parse", "HEAD"]);
    git(repo.path(), &["tag", "oracle"]);

    std::fs::write(repo.path().join("content"), "shadow ref\n").unwrap();
    git(repo.path(), &["commit", "--quiet", "-am", "shadow ref"]);
    let shadow_sha = git(repo.path(), &["rev-parse", "HEAD"]);
    git(repo.path(), &["update-ref", "refs/oracle", &shadow_sha]);
    assert_ne!(tag_sha, shadow_sha);

    let result = BaselineResolver::new(repo.path())
        .with_tag("oracle")
        .with_expected_sha(&tag_sha)
        .resolve();
    assert!(
        result.is_ok(),
        "tag namespace should win over refs/oracle: {result:?}"
    );
}
