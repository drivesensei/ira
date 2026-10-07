//! Independent staging isolation regression from reviewer-harness (T-012).
#![cfg(unix)]
// GAP-FIXED(G-F002-ADV-61) sev=high kind=security feature=F-002
//   what: Golden staging follows metadata.toml symlinks and overwrites external evidence.
//   tui-ref: migration/specs/F-002.md S6, State and effects
//   repro: A synthetic staging metadata.toml symlink points at an approved sentinel file.
//   expected: The external approved file remains byte-identical.
//   actual: capture_bundle_to_staging returns Ok and overwrites the sentinel.
//   cover: metadata_symlink_must_not_write_outside_staging
//   fixed-by: T017 anchored no-follow directories and exclusive artifact creation; independent verification pending.
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
#[test]
fn staging_parent_and_payload_directory_symlinks_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let link = root.path().join("parent");
    std::os::unix::fs::symlink(outside.path(), &link).unwrap();
    assert!(GoldenStore::new(outside.path())
        .capture_bundle_to_staging(link.join("candidate"), &bundle())
        .is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    let stage = root.path().join("stage");
    std::fs::create_dir(&stage).unwrap();
    std::os::unix::fs::symlink(outside.path(), stage.join("observations")).unwrap();
    assert!(GoldenStore::new(outside.path())
        .capture_bundle_to_staging(&stage, &bundle())
        .is_err());
    assert!(!stage.join("metadata.toml").exists());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}
#[test]
fn preexisting_metadata_manifest_and_pending_artifacts_are_immutable() {
    for name in ["metadata.toml", "manifest.toml", "manifest.pending"] {
        let stage = tempfile::tempdir().unwrap();
        std::fs::write(stage.path().join(name), b"CALLER OWNED").unwrap();
        std::fs::write(stage.path().join("unrelated"), b"KEEP").unwrap();
        assert!(GoldenStore::new(stage.path())
            .capture_bundle_to_staging(stage.path(), &bundle())
            .is_err());
        assert_eq!(
            std::fs::read(stage.path().join(name)).unwrap(),
            b"CALLER OWNED"
        );
        assert_eq!(
            std::fs::read(stage.path().join("unrelated")).unwrap(),
            b"KEEP"
        );
        assert!(!stage.path().join("observations").exists());
    }
}
#[test]
fn legacy_trace_staging_rejects_metadata_symlink_without_external_writes() {
    let root = tempfile::tempdir().unwrap();
    let stage = root.path().join("stage");
    std::fs::create_dir(&stage).unwrap();
    let outside = root.path().join("approved");
    std::fs::write(&outside, b"SENTINEL").unwrap();
    std::os::unix::fs::symlink(&outside, stage.join("metadata.toml")).unwrap();
    let trace = parse_trace(include_str!(
        "../../../migration/oracle/traces/harness/initial_screen.toml"
    ))
    .unwrap();
    assert!(GoldenStore::new(&outside)
        .capture_to_staging(&stage, &trace)
        .is_err());
    assert_eq!(std::fs::read(outside).unwrap(), b"SENTINEL");
    assert!(!stage.join("initial_screen.toml").exists());
}
#[test]
fn concurrent_capture_has_exactly_one_exclusive_owner() {
    let stage = tempfile::tempdir().unwrap();
    let root = stage.path().to_owned();
    let joins: Vec<_> = (0..8)
        .map(|_| {
            let root = root.clone();
            std::thread::spawn(move || {
                GoldenStore::new(&root).capture_bundle_to_staging(&root, &bundle())
            })
        })
        .collect();
    let results: Vec<_> = joins.into_iter().map(|j| j.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    let loaded = ObservationBundle::load_from_staging(&root).unwrap();
    assert_eq!(
        loaded.metadata.oracle_sha(),
        ira_parity::baseline::ORACLE_SHA
    );
}
