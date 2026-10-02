//! Compile-time contract for the S13 capture API, which is absent at the
//! reviewed base (46811ea). The expected API is intentionally exercised here
//! so cargo reports exactly which observation-bundle surface is missing.
use ira_parity::{
    golden::{GoldenMetadata, GoldenStore, ObservationBundle, ObservationRecord, ObservationValue},
    trace::{parse_trace, ObservationKind, Trace},
};
use std::{fs, path::PathBuf};

fn bundle(observations: Vec<ObservationRecord>) -> ObservationBundle {
    let trace: Trace = parse_trace(include_str!(
        "../../../migration/oracle/traces/harness/initial_screen.toml"
    ))
    .unwrap();
    ObservationBundle::new(
        GoldenMetadata::from_trace(
            "1cad4ce43cc72d52d4cc4eef920e0da22cb69568",
            &trace,
            "linux",
            "2026-10-01",
        ),
        trace.events().to_vec(),
        observations,
    )
}

#[test]
fn s13_capture_uses_actual_observation_not_trace_expectation() {
    let observed = ObservationRecord::new(
        ObservationKind::TerminalScreen,
        None,
        ObservationValue::Text("observed screen output".into()),
    );
    let bundle = bundle(vec![observed]);
    let stage = tempfile::tempdir().unwrap();
    GoldenStore::new(stage.path())
        .capture_bundle_to_staging(stage.path(), &bundle)
        .unwrap();
    let loaded = ObservationBundle::load_from_staging(stage.path()).unwrap();
    assert_eq!(
        loaded.observations()[0].value(),
        &ObservationValue::Text("observed screen output".into())
    );
}

#[test]
fn s13_byte_observations_round_trip_nul_and_invalid_utf8() {
    let bytes = vec![0, 0xff, b'A', 0, 0xc3, 0x28];
    let bundle = bundle(vec![ObservationRecord::new(
        ObservationKind::PersistedBytes {
            relative_path: "state.bin".into(),
        },
        Some(PathBuf::from("state.bin")),
        ObservationValue::Bytes(bytes.clone()),
    )]);
    let stage = tempfile::tempdir().unwrap();
    GoldenStore::new(stage.path())
        .capture_bundle_to_staging(stage.path(), &bundle)
        .unwrap();
    let loaded = ObservationBundle::load_from_staging(stage.path()).unwrap();
    assert_eq!(
        loaded.observations()[0].value(),
        &ObservationValue::Bytes(bytes)
    );
}

#[test]
fn s13_paths_round_trip_without_unicode_normalization_or_loss() {
    let nfc = PathBuf::from("é.txt");
    let nfd = PathBuf::from("e\u{301}.txt");
    let mut paths = vec![nfc.clone(), nfd.clone()];
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        paths.push(PathBuf::from(std::ffi::OsString::from_vec(vec![
            b'x', 0xff, b'y',
        ])));
    }
    let observations = paths
        .iter()
        .map(|path| {
            ObservationRecord::new(
                ObservationKind::Filesystem {
                    relative_path: path.to_string_lossy().into_owned(),
                },
                Some(path.clone()),
                ObservationValue::Bytes(vec![1, 2, 3]),
            )
        })
        .collect();
    let bundle = bundle(observations);
    let stage = tempfile::tempdir().unwrap();
    GoldenStore::new(stage.path())
        .capture_bundle_to_staging(stage.path(), &bundle)
        .unwrap();
    let loaded = ObservationBundle::load_from_staging(stage.path()).unwrap();
    let got: Vec<_> = loaded
        .observations()
        .iter()
        .map(|item| item.relative_path().unwrap().clone())
        .collect();
    let mut expected = paths;
    expected.sort();
    assert_eq!(got, expected);
}

#[test]
fn s13_bundle_order_is_stable_and_manifest_is_published_last() {
    let a = ObservationRecord::new(
        ObservationKind::PersistedBytes {
            relative_path: "a".into(),
        },
        Some("a".into()),
        ObservationValue::Bytes(vec![1]),
    );
    let b = ObservationRecord::new(
        ObservationKind::PersistedBytes {
            relative_path: "b".into(),
        },
        Some("b".into()),
        ObservationValue::Bytes(vec![2]),
    );
    let left = bundle(vec![b.clone(), a.clone()]);
    let right = bundle(vec![a, b]);
    let one = tempfile::tempdir().unwrap();
    let two = tempfile::tempdir().unwrap();
    GoldenStore::new(one.path())
        .capture_bundle_to_staging(one.path(), &left)
        .unwrap();
    GoldenStore::new(two.path())
        .capture_bundle_to_staging(two.path(), &right)
        .unwrap();
    fn collect(root: &std::path::Path, at: &std::path::Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(at).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                collect(root, &entry.path(), files);
            } else {
                files.push((
                    entry.path().strip_prefix(root).unwrap().to_owned(),
                    fs::read(entry.path()).unwrap(),
                ));
            }
        }
    }
    let tree = |root: &std::path::Path| {
        let mut files = Vec::new();
        collect(root, root, &mut files);
        files.sort_by(|x, y| x.0.cmp(&y.0));
        files
    };
    assert_eq!(tree(one.path()), tree(two.path()));

    // A conflicting payload path deterministically interrupts payload writing.
    // The implementation must not publish a complete manifest on this failure.
    let interrupted = tempfile::tempdir().unwrap();
    fs::write(
        interrupted.path().join("observations"),
        b"block directory creation",
    )
    .unwrap();
    let result =
        GoldenStore::new(interrupted.path()).capture_bundle_to_staging(interrupted.path(), &left);
    assert!(result.is_err());
    assert!(!interrupted.path().join("manifest.toml").exists());
}
// GAP(G-F002-ADV-51) sev=high kind=test-gap feature=F-002
//   what:     Staged golden output must contain the target's observed value, not only the trace input/expectation.
//   tui-ref:  migration/specs/F-002.md S6,S13
//   oracle:   Observe(kind) value is the capture evidence; trace expectations are separate inputs.
//   repro:    Capture a bundle with "observed screen output" and reload its observation record.
//   expected: Loaded bundle has that exact observed output and does not substitute the trace assertion.
//   actual:   Golden capture API has no observation bundle or capture_bundle_to_staging method.
//   cover:    s13_capture_uses_actual_observation_not_trace_expectation
// GAP(G-F002-ADV-52) sev=high kind=data-compat feature=F-002
//   what:     Byte observations require lossless tagged encoding, including NUL and invalid UTF-8 bytes.
//   tui-ref:  migration/specs/F-002.md S5,S13
//   oracle:   PersistedBytes observations are byte-exact; TOML text alone cannot encode arbitrary bytes.
//   repro:    Round-trip [00 ff 41 00 c3 28] through the staged observation bundle.
//   expected: Reloaded byte payload is identical byte-for-byte.
//   actual:   ObservationValue::Bytes and ObservationBundle do not exist.
//   cover:    s13_byte_observations_round_trip_nul_and_invalid_utf8
// GAP(G-F002-ADV-53) sev=high kind=data-compat feature=F-002
//   what:     Captured paths need reversible identity; lossy strings or normalization merge distinct paths.
//   tui-ref:  migration/specs/F-002.md S5,S13; AGENTS.md invariant 5
//   oracle:   Paths retain platform OsString identity; NFC/NFD are distinct on Linux and raw Unix bytes may be invalid UTF-8.
//   repro:    Round-trip NFC, NFD, and (on Unix) an OsString containing byte ff.
//   expected: Loaded relative paths preserve identity and remain deterministically ordered.
//   actual:   Current ObservationKind path is String and no reversible path codec/bundle loader exists.
//   cover:    s13_paths_round_trip_without_unicode_normalization_or_loss
// GAP(G-F002-ADV-54) sev=medium kind=edge-case feature=F-002
//   what:     Observation order must be stable and a failed/incomplete write must not publish a complete manifest.
//   tui-ref:  migration/specs/F-002.md S13
//   oracle:   Golden staging publication is deterministic and manifest-last or atomically renamed.
//   repro:    Stage identical records in reverse input order; then force payload directory creation to fail.
//   expected: Byte-identical staged trees; interrupted staging returns an error and has no manifest.
//   actual:   Existing writer writes trace TOML and metadata directly; no bundle manifest/payload transaction exists.
//   cover:    s13_bundle_order_is_stable_and_manifest_is_published_last
