use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("invalid trace: {0}")]
pub struct TraceError(String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputEvent {
    Key {
        code: KeyCode,
        #[serde(default)]
        modifiers: BTreeSet<Modifier>,
        phase: KeyPhase,
    },
    Text {
        value: String,
    },
    Paste {
        value: String,
    },
    Resize {
        columns: u16,
        rows: u16,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeyCode {
    Character { character: char },
    Named(NamedKey),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NamedKey {
    Enter,
    Escape,
    Tab,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Shift,
    Control,
    Alt,
    Super,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyPhase {
    Press,
    Repeat,
    Release,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObservationKind {
    Process,
    TerminalScreen,
    DomainSnapshot { name: String },
    Filesystem { relative_path: String },
    PersistedBytes { relative_path: String },
}
impl ObservationKind {
    pub fn domain_name(&self) -> Option<&str> {
        if let Self::DomainSnapshot { name } = self {
            Some(name)
        } else {
            None
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTrace {
    schema_version: u32,
    scenario_id: String,
    platforms: Vec<String>,
    fixture: Fixture,
    terminal: Terminal,
    #[serde(default)]
    environment: std::collections::BTreeMap<String, String>,
    readiness: Readiness,
    events: Option<Vec<InputEvent>>,
    observations: Option<Vec<ExpectedObservation>>,
    #[serde(default)]
    expected_error: Option<ExpectedError>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub kind: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terminal {
    pub columns: u16,
    pub rows: u16,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Readiness {
    pub observation: ObservationKind,
    pub condition: Condition,
    pub deadline_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    #[serde(default)]
    pub contains: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExpectedObservation {
    #[serde(flatten)]
    pub kind: ObservationKind,
    pub expect: toml::Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedError {
    pub kind: String,
    pub message_contains: String,
}
#[derive(Clone, Debug)]
pub struct Trace {
    pub scenario_id: String,
    pub platforms: Vec<String>,
    pub fixture: Fixture,
    pub terminal: Terminal,
    pub environment: std::collections::BTreeMap<String, String>,
    pub readiness: Readiness,
    events: Vec<InputEvent>,
    observations: Vec<ExpectedObservation>,
    pub expected_error: Option<ExpectedError>,
}
pub fn parse_trace(s: &str) -> Result<Trace, TraceError> {
    Trace::parse_toml(s)
}
impl Trace {
    pub fn parse_toml(s: &str) -> Result<Self, TraceError> {
        let raw: RawTrace = toml::from_str(s).map_err(|e| TraceError(e.to_string()))?;
        if raw.schema_version != 1 {
            return Err(TraceError(format!(
                "unsupported schema version {}",
                raw.schema_version
            )));
        }
        for p in &raw.platforms {
            if !["linux", "macos", "windows-msvc"].contains(&p.as_str()) {
                return Err(TraceError(format!("unsupported platform profile {p}")));
            }
        }
        if raw.scenario_id.is_empty() {
            return Err(TraceError("scenario_id is required".into()));
        }
        let events = raw
            .events
            .ok_or_else(|| TraceError("events is required".into()))?;
        let observations = match (raw.observations, raw.expected_error.is_some()) {
            (Some(values), _) => values,
            (None, true) => Vec::new(),
            (None, false) => return Err(TraceError("observations is required".into())),
        };
        Ok(Self {
            scenario_id: raw.scenario_id,
            platforms: raw.platforms,
            fixture: raw.fixture,
            terminal: raw.terminal,
            environment: raw.environment,
            readiness: raw.readiness,
            events,
            observations,
            expected_error: raw.expected_error,
        })
    }
    pub fn events(&self) -> &[InputEvent] {
        &self.events
    }
    pub fn with_events(mut self, e: Vec<InputEvent>) -> Self {
        self.events = e;
        self
    }
    pub fn observation_kinds(&self) -> Vec<ObservationKind> {
        self.observations.iter().map(|x| x.kind.clone()).collect()
    }
    pub fn expected_observations(&self) -> &[ExpectedObservation] {
        &self.observations
    }
    pub fn with_observation(mut self, k: ObservationKind) -> Self {
        self.observations.push(ExpectedObservation {
            kind: k,
            expect: toml::Value::Table(Default::default()),
        });
        self
    }
    pub fn to_toml(&self) -> String {
        toml::to_string(&SerializableTrace {
            schema_version: 1,
            scenario_id: &self.scenario_id,
            platforms: &self.platforms,
            fixture: &self.fixture,
            terminal: &self.terminal,
            environment: &self.environment,
            readiness: &self.readiness,
            events: &self.events,
            observations: &self.observations,
            expected_error: &self.expected_error,
        })
        .unwrap_or_else(|e| format!("# serialization error: {e}"))
    }
    pub fn schema_dependencies(&self) -> Vec<&'static str> {
        vec!["std"]
    }
}
#[derive(Serialize)]
struct SerializableTrace<'a> {
    schema_version: u32,
    scenario_id: &'a str,
    platforms: &'a Vec<String>,
    fixture: &'a Fixture,
    terminal: &'a Terminal,
    environment: &'a std::collections::BTreeMap<String, String>,
    readiness: &'a Readiness,
    events: &'a Vec<InputEvent>,
    observations: &'a Vec<ExpectedObservation>,
    expected_error: &'a Option<ExpectedError>,
}
