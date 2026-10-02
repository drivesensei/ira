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
fn current_capture_date() -> String {
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
