//! Compile-time contract for the S13 capture API, which is absent at the
//! reviewed base (46811ea). The expected API is intentionally exercised here
//! so cargo reports exactly which observation-bundle surface is missing.
use ira_parity::{
    golden::{GoldenMetadata, GoldenStore, ObservationBundle, ObservationRecord, ObservationValue},
    runner::{Observation, RunCapture, RunOptions, ScenarioRunner, TraceTarget},
    trace::{parse_trace, ObservationKind, Trace},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

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

struct DivergentObservationTarget;

impl TraceTarget for DivergentObservationTarget {
    fn name(&self) -> &str {
        "capture-probe"
    }
    fn supports_observation(&self, _: &ObservationKind) -> bool {
        true
    }
    fn start(
        &mut self,
        _: &Path,
        _: (u16, u16),
        _: &BTreeMap<String, String>,
    ) -> Result<(), ira_parity::runner::RunError> {
        Ok(())
    }
    fn wait_ready(
        &mut self,
        _: &ira_parity::trace::Readiness,
        _: Instant,
    ) -> Result<(), ira_parity::runner::RunError> {
        Ok(())
    }
    fn apply(
        &mut self,
        _: &ira_parity::trace::InputEvent,
    ) -> Result<(), ira_parity::runner::RunError> {
        Ok(())
    }
    fn observe(
        &mut self,
        _: &ObservationKind,
    ) -> Result<Observation, ira_parity::runner::RunError> {
        let mut table = toml::map::Map::new();
        table.insert(
            "status".into(),
            toml::Value::String("observed-only output".into()),
        );
        Ok(Observation {
            value: Some(toml::Value::Table(table)),
            ..Observation::default()
        })
    }
    fn shutdown(&mut self) -> Result<i32, ira_parity::runner::RunError> {
        Ok(0)
    }
}

#[test]
fn s13_capture_uses_actual_observation_not_trace_expectation() {
    let trace = parse_trace(
        r#"
schema_version = 1
scenario_id = "capture.observed-output"
platforms = ["linux"]
fixture = { kind = "empty_directory" }
events = []
[terminal]
columns = 80
rows = 24
[environment]
[readiness]
observation = { kind = "domain_snapshot", name = "capture_probe" }
condition = { contains = ["observed-only"] }
deadline_ms = 1000
[[observations]]
kind = "domain_snapshot"
name = "capture_probe"
expect = { status = "trace expectation" }
"#,
    )
    .unwrap();
    let capture: RunCapture = ScenarioRunner::new(RunOptions::for_platform("linux"))
        .run_capture_with_target(&trace, &mut DivergentObservationTarget)
        .unwrap();
    let stage = tempfile::tempdir().unwrap();
    GoldenStore::new(stage.path())
        .capture_run_to_staging(stage.path(), &capture)
        .unwrap();
    let loaded = ObservationBundle::load_from_staging(stage.path()).unwrap();
    assert_eq!(
        loaded.observations()[0].value(),
        &ObservationValue::Structured(toml::Value::Table(toml::map::Map::from_iter([(
            "status".into(),
            toml::Value::String("observed-only output".into()),
        )])))
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
                    // Deliberately lossy for the raw Unix name: the record's
                    // PathBuf must remain the authoritative serialized path.
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
//   repro:    Run a custom TraceTarget returning status="observed-only output" against a trace expecting status="trace expectation", then stage the run capture.
//   expected: Loaded bundle contains the target's actual observation despite the expectation mismatch.
//   actual:   No RunCapture/run_capture_with_target/capture_run_to_staging seam exists; the current run result drops observations.
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
//   repro:    Record NFC, NFD, and (on Unix) OsString byte ff while the kind's String path uses to_string_lossy; round-trip the bundle.
//   expected: Loaded relative_path preserves PathBuf identity; the lossy kind string never becomes serialized path authority.
//   actual:   Current ObservationKind relative_path is String and no reversible record path codec/bundle loader exists.
//   cover:    s13_paths_round_trip_without_unicode_normalization_or_loss
// GAP(G-F002-ADV-54) sev=medium kind=edge-case feature=F-002
//   what:     Observation order must be stable and a failed/incomplete write must not publish a complete manifest.
//   tui-ref:  migration/specs/F-002.md S13
//   oracle:   Golden staging publication is deterministic and manifest-last or atomically renamed.
//   repro:    Stage identical records in reverse input order; then force payload directory creation to fail.
//   expected: Byte-identical staged trees; interrupted staging returns an error and has no manifest.
//   actual:   Existing writer writes trace TOML and metadata directly; no bundle manifest/payload transaction exists.
//   cover:    s13_bundle_order_is_stable_and_manifest_is_published_last
