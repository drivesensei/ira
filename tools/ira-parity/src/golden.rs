use crate::trace::Trace;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GoldenMetadata {
    pub oracle_sha: String,
    pub scenario_id: String,
    pub fixture: String,
    pub dimensions: String,
    pub events: String,
    pub os_profile: String,
    pub capture_date: String,
    #[serde(default)]
    status: String,
}
impl GoldenMetadata {
    pub fn from_trace(sha: &str, t: &Trace, os: &str, date: &str) -> Self {
        Self {
            oracle_sha: sha.into(),
            scenario_id: t.scenario_id.clone(),
            fixture: t.fixture.kind.clone(),
            dimensions: format!("{}x{}", t.terminal.columns, t.terminal.rows),
            events: format!("{:?}", t.events()),
            os_profile: os.into(),
            capture_date: date.into(),
            status: "pending_logic_review".into(),
        }
    }
    pub fn has_field(&self, n: &str) -> bool {
        [
            "oracle_sha",
            "scenario_id",
            "fixture",
            "dimensions",
            "events",
            "os_profile",
            "capture_date",
        ]
        .contains(&n)
    }
    pub fn oracle_sha(&self) -> &str {
        &self.oracle_sha
    }
    pub fn review_status(&self) -> &str {
        &self.status
    }
    pub fn load_from_staging(p: impl AsRef<Path>) -> Result<Self, String> {
        let s = fs::read_to_string(p.as_ref().join("metadata.toml")).map_err(|e| e.to_string())?;
        toml::from_str(&s).map_err(|e| e.to_string())
    }
}
pub(crate) fn current_capture_date() -> String {
    let days_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| (duration.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
pub struct GoldenStore {
    approved: PathBuf,
}
impl GoldenStore {
    pub fn new(p: impl AsRef<Path>) -> Self {
        Self {
            approved: p.as_ref().into(),
        }
    }
    pub fn stage_candidate(&self, stage: impl AsRef<Path>, t: &Trace) -> Result<(), String> {
        self.capture_to_staging(stage, t)
    }
    /// Legacy trace/metadata staging only. No completion manifest is published;
    /// use `capture_run_to_staging` for actual observed golden evidence.
    pub fn capture_to_staging(&self, stage: impl AsRef<Path>, t: &Trace) -> Result<(), String> {
        let stage = stage.as_ref();
        fs::create_dir_all(stage).map_err(|e| e.to_string())?;
        fs::write(stage.join("initial_screen.toml"), t.to_toml()).map_err(|e| e.to_string())?;
        let metadata = GoldenMetadata::from_trace(
            "1cad4ce43cc72d52d4cc4eef920e0da22cb69568",
            t,
            if cfg!(target_os = "windows") {
                "windows-msvc"
            } else {
                std::env::consts::OS
            },
            &current_capture_date(),
        );
        fs::write(
            stage.join("metadata.toml"),
            toml::to_string(&metadata).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let _ = &self.approved;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObservationValue {
    Missing,
    Bytes(Vec<u8>),
    Text(String),
    Structured(toml::Value),
}
#[derive(Clone, Debug)]
pub struct ObservationRecord {
    kind: crate::trace::ObservationKind,
    relative_path: Option<PathBuf>,
    value: ObservationValue,
}
impl ObservationRecord {
    pub fn new(
        kind: crate::trace::ObservationKind,
        relative_path: Option<PathBuf>,
        value: ObservationValue,
    ) -> Self {
        Self {
            kind,
            relative_path,
            value,
        }
    }
    pub fn value(&self) -> &ObservationValue {
        &self.value
    }
    pub fn relative_path(&self) -> Option<&PathBuf> {
        self.relative_path.as_ref()
    }
    pub fn kind(&self) -> &crate::trace::ObservationKind {
        &self.kind
    }
    pub(crate) fn from_observation(
        kind: &crate::trace::ObservationKind,
        o: &crate::runner::Observation,
    ) -> Result<Self, String> {
        use crate::trace::ObservationKind::*;
        let path = match kind {
            Filesystem { relative_path } | PersistedBytes { relative_path } => {
                Some(PathBuf::from(relative_path))
            }
            _ => None,
        };
        let value = match kind {
            Filesystem { .. } | PersistedBytes { .. } => o
                .bytes
                .clone()
                .map(ObservationValue::Bytes)
                .unwrap_or(ObservationValue::Missing),
            TerminalScreen => o
                .screen()
                .map(|text| ObservationValue::Text(text.into()))
                .unwrap_or(ObservationValue::Missing),
            DomainSnapshot { .. } => o
                .value
                .clone()
                .map(ObservationValue::Structured)
                .unwrap_or(ObservationValue::Missing),
            Process => {
                let mut table = toml::map::Map::new();
                if let Some(code) = o.exit_code {
                    table.insert("exit_code".into(), toml::Value::Integer(code.into()));
                }
                if let Some(text) = o.stdout() {
                    table.insert("stdout".into(), toml::Value::String(text.into()));
                }
                if let Some(text) = o.stderr() {
                    table.insert("stderr".into(), toml::Value::String(text.into()));
                }
                ObservationValue::Structured(toml::Value::Table(table))
            }
        };
        Ok(Self::new(kind.clone(), path, value))
    }
}
#[derive(Clone, Debug)]
pub struct ObservationBundle {
    pub metadata: GoldenMetadata,
    pub events: Vec<crate::trace::InputEvent>,
    observations: Vec<ObservationRecord>,
}
impl ObservationBundle {
    pub fn new(
        metadata: GoldenMetadata,
        events: Vec<crate::trace::InputEvent>,
        mut observations: Vec<ObservationRecord>,
    ) -> Self {
        observations.sort_by(|a, b| {
            kind_key(&a.kind)
                .cmp(&kind_key(&b.kind))
                .then_with(|| a.relative_path.cmp(&b.relative_path))
        });
        Self {
            metadata,
            events,
            observations,
        }
    }
    pub fn observations(&self) -> &[ObservationRecord] {
        &self.observations
    }
    pub fn load_from_staging(stage: impl AsRef<Path>) -> Result<Self, String> {
        let stage = stage.as_ref();
        let manifest: BundleManifest = toml::from_str(
            &fs::read_to_string(stage.join("manifest.toml")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if manifest.schema_version != 1 || manifest.oracle_tag != "tui-oracle-baseline" {
            return Err("unsupported observation bundle schema or oracle tag".into());
        }
        let mut observations = Vec::new();
        for (i, record) in manifest.observations.into_iter().enumerate() {
            let expected = format!("observations/{i:06}.toml");
            if record.payload != expected {
                return Err("invalid observation payload path".into());
            }
            let payload: Payload = toml::from_str(
                &fs::read_to_string(stage.join(&record.payload)).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let value = match payload {
                Payload::Missing => ObservationValue::Missing,
                Payload::Utf8 { value } => ObservationValue::Bytes(value.into_bytes()),
                Payload::Hex { value } => {
                    ObservationValue::Bytes(hex::decode(value).map_err(|e| e.to_string())?)
                }
                Payload::Text { value } => ObservationValue::Text(value),
                Payload::Structured { value } => ObservationValue::Structured(value),
            };
            let path = record.path.map(decode_path).transpose()?;
            observations.push(ObservationRecord::new(record.kind, path, value));
        }
        Ok(Self::new(manifest.metadata, manifest.events, observations))
    }
}
fn kind_key(kind: &crate::trace::ObservationKind) -> String {
    // Paths are ordered by native identity rather than the lossy schema label.
    use crate::trace::ObservationKind::*;
    match kind {
        Process => "process".into(),
        TerminalScreen => "terminal_screen".into(),
        DomainSnapshot { name } => format!("domain_snapshot:{name}"),
        Filesystem { .. } => "filesystem".into(),
        PersistedBytes { .. } => "persisted_bytes".into(),
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleManifest {
    schema_version: u32,
    oracle_tag: String,
    screen_policy: String,
    metadata: GoldenMetadata,
    events: Vec<crate::trace::InputEvent>,
    observations: Vec<RecordManifest>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordManifest {
    kind: crate::trace::ObservationKind,
    path: Option<EncodedPath>,
    payload: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EncodedPath {
    encoding: String,
    value: String,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "encoding", rename_all = "snake_case", deny_unknown_fields)]
enum Payload {
    Missing,
    Utf8 { value: String },
    Hex { value: String },
    Text { value: String },
    Structured { value: toml::Value },
}
fn encode_path(path: &Path) -> EncodedPath {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        EncodedPath {
            encoding: "unix_bytes_hex".into(),
            value: hex::encode(path.as_os_str().as_bytes()),
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let bytes: Vec<u8> = path
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect();
        EncodedPath {
            encoding: "windows_utf16le_hex".into(),
            value: hex::encode(bytes),
        }
    }
}
fn decode_path(path: EncodedPath) -> Result<PathBuf, String> {
    let bytes = hex::decode(path.value).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        if path.encoding != "unix_bytes_hex" {
            return Err("path encoding does not match Unix host".into());
        }
        Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        if path.encoding != "windows_utf16le_hex" || bytes.len() % 2 != 0 {
            return Err("invalid Windows path encoding".into());
        }
        let words: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        Ok(PathBuf::from(std::ffi::OsString::from_wide(&words)))
    }
}
impl GoldenStore {
    pub fn capture_run_to_staging(
        &self,
        stage: impl AsRef<Path>,
        run: &crate::runner::RunCapture,
    ) -> Result<(), String> {
        self.capture_bundle_to_staging(stage, &run.bundle)
    }
    pub fn capture_bundle_to_staging(
        &self,
        stage: impl AsRef<Path>,
        bundle: &ObservationBundle,
    ) -> Result<(), String> {
        let stage = stage.as_ref();
        fs::create_dir_all(stage).map_err(|e| e.to_string())?;
        // Existing captures are immutable. A failed attempt has no completion manifest.
        fs::create_dir(stage.join("observations")).map_err(|e| e.to_string())?;
        let mut records = Vec::new();
        for (i, item) in bundle.observations.iter().enumerate() {
            let payload = match &item.value {
                ObservationValue::Missing => Payload::Missing,
                ObservationValue::Bytes(bytes) => match std::str::from_utf8(bytes) {
                    Ok(text) => Payload::Utf8 { value: text.into() },
                    Err(_) => Payload::Hex {
                        value: hex::encode(bytes),
                    },
                },
                ObservationValue::Text(value) => Payload::Text {
                    value: value.clone(),
                },
                ObservationValue::Structured(value) => Payload::Structured {
                    value: value.clone(),
                },
            };
            let relative = format!("observations/{i:06}.toml");
            fs::write(
                stage.join(&relative),
                toml::to_string(&payload).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            records.push(RecordManifest {
                kind: item.kind.clone(),
                path: item.relative_path.as_deref().map(encode_path),
                payload: relative,
            });
        }
        let manifest = BundleManifest {
            schema_version: 1,
            oracle_tag: "tui-oracle-baseline".into(),
            screen_policy: "adapter-provided UTF-8 text; no additional normalization".into(),
            metadata: bundle.metadata.clone(),
            events: bundle.events.clone(),
            observations: records,
        };
        fs::write(
            stage.join("metadata.toml"),
            toml::to_string(&bundle.metadata).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        use std::io::Write;
        let mut pending = tempfile::NamedTempFile::new_in(stage).map_err(|e| e.to_string())?;
        pending
            .write_all(
                toml::to_string(&manifest)
                    .map_err(|e| e.to_string())?
                    .as_bytes(),
            )
            .map_err(|e| e.to_string())?;
        pending.as_file().sync_all().map_err(|e| e.to_string())?;
        pending
            .persist_noclobber(stage.join("manifest.toml"))
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
