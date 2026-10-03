//! Tagged baseline replay through the production PTY session and staging entry point.
use ira_parity::{
    golden::{GoldenStore, ObservationBundle, ObservationValue},
    runner::{RunOptions, ScenarioRunner},
    trace::parse_trace,
};
#[test]
fn tagged_oracle_capture_retains_actual_screen_before_fixture_removal() {
    let trace = parse_trace(include_str!(
        "../../../migration/oracle/traces/harness/initial_screen.toml"
    ))
    .unwrap();
    let capture = ScenarioRunner::new(RunOptions::for_current_platform())
        .run_oracle_capture(&trace)
        .unwrap();
    assert!(capture.result.fixture_removed());
    assert!(!capture.result.fixture_root().exists());
    let stage = tempfile::tempdir().unwrap();
    GoldenStore::new(stage.path())
        .capture_run_to_staging(stage.path(), &capture)
        .unwrap();
    let bundle = ObservationBundle::load_from_staging(stage.path()).unwrap();
    assert_eq!(
        bundle.metadata.oracle_sha(),
        ira_parity::baseline::ORACLE_SHA
    );
    assert_eq!(bundle.metadata.review_status(), "pending_logic_review");
    assert!(bundle.observations().iter().any(
        |record| matches!(record.value(), ObservationValue::Text(text) if ira_parity::normalize::normalize_screen(text, &[]).contains("Common folders"))
    ));
}
