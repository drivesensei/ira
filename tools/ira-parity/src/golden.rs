use crate::trace::Trace;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
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
    pub fn capture_to_staging(&self, stage: impl AsRef<Path>, t: &Trace) -> Result<(), String> {
        let stage = stage.as_ref();
        fs::create_dir_all(stage).map_err(|e| e.to_string())?;
        fs::write(stage.join("initial_screen.toml"), t.to_toml()).map_err(|e| e.to_string())?;
        let metadata = GoldenMetadata::from_trace(
            "1cad4ce43cc72d52d4cc4eef920e0da22cb69568",
            t,
            "unknown",
            "unknown",
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
