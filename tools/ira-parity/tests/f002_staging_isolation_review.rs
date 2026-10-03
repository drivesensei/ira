//! Independent staging isolation regression from reviewer-harness (T-012).
#![cfg(unix)]
// GAP(G-F002-ADV-61) sev=high kind=security feature=F-002
//   what: Golden staging follows metadata.toml symlinks and overwrites external evidence.
//   tui-ref: migration/specs/F-002.md S6, State and effects
//   repro: A synthetic staging metadata.toml symlink points at an approved sentinel file.
//   expected: The external approved file remains byte-identical.
//   actual: capture_bundle_to_staging returns Ok and overwrites the sentinel.
//   cover: metadata_symlink_must_not_write_outside_staging
//   ledger: G-0027; reviewed-production: b4829e21e032670abfbd5506f9df1983e094dbb4
use ira_parity::{
    golden::{GoldenMetadata, GoldenStore, ObservationBundle},
    trace::parse_trace,
};
fn bundle() -> ObservationBundle {
    let t = parse_trace(include_str!(
        "../../../migration/oracle/traces/harness/initial_screen.toml"
    ))
    .unwrap();
    ObservationBundle::new(
        GoldenMetadata::from_trace(ira_parity::baseline::ORACLE_SHA, &t, "macos", "2026-10-03"),
        t.events().to_vec(),
        vec![],
    )
}
#[test]
fn metadata_symlink_must_not_write_outside_staging() {
    let root = tempfile::tempdir().unwrap();
    let stage = root.path().join("stage");
    std::fs::create_dir(&stage).unwrap();
    let approved = root.path().join("approved.toml");
    std::fs::write(&approved, b"APPROVED SENTINEL").unwrap();
    std::os::unix::fs::symlink(&approved, stage.join("metadata.toml")).unwrap();
    let result = GoldenStore::new(&approved).capture_bundle_to_staging(&stage, &bundle());
    println!("stage result={result:?}");
    assert_eq!(
        std::fs::read(&approved).unwrap(),
        b"APPROVED SENTINEL",
        "staging followed metadata symlink and overwrote approved evidence outside stage"
    );
}
