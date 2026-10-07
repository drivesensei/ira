use crate::{
    baseline::{BaselineResolver, ORACLE_SHA},
    lifecycle::{PtySession, PtySessionConfig},
    trace::{InputEvent, KeyPhase, ObservationKind, Trace},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use thiserror::Error;
#[derive(Debug, Error)]
#[error("{message}")]
pub struct RunError {
    pub(crate) message: String,
    pub(crate) timeout: bool,
    pub(crate) readiness: bool,
    pub(crate) delivered: usize,
}
impl RunError {
    pub fn is_timeout(&self) -> bool {
        self.timeout
    }
    pub fn is_readiness_timeout(&self) -> bool {
        self.readiness
    }
    pub fn input_events_delivered(&self) -> usize {
        self.delivered
    }
}
#[derive(Clone)]
pub struct RunOptions {
    pub platform: String,
    pub deadline: Duration,
    pub cleanup: Duration,
    approved: Option<PathBuf>,
    readiness: Duration,
    fixture_removal_failure_for_test: bool,
    fixture_retention_capture_for_test: Option<std::sync::Arc<std::sync::Mutex<Option<PathBuf>>>>,
}
impl Default for RunOptions {
    fn default() -> Self {
        Self {
            platform: std::env::consts::OS.into(),
            deadline: Duration::from_secs(30),
            cleanup: Duration::from_secs(2),
            approved: None,
            readiness: Duration::from_secs(15),
            fixture_removal_failure_for_test: false,
            fixture_retention_capture_for_test: None,
        }
    }
}
impl RunOptions {
    pub fn for_platform(s: &str) -> Self {
        Self {
            platform: s.into(),
            ..Self::default()
        }
    }
    pub fn for_current_platform() -> Self {
        Self::default()
    }
    pub fn with_deadlines(d: Duration, c: Duration) -> Self {
        Self {
            deadline: d,
            cleanup: c,
            ..Self::default()
        }
    }
    pub fn with_readiness_deadline(d: Duration) -> Self {
        Self {
            readiness: d,
            ..Self::default()
        }
    }
    pub fn with_approved_golden_dir(mut self, p: impl AsRef<Path>) -> Self {
        self.approved = Some(p.as_ref().into());
        self
    }
    #[doc(hidden)]
    pub fn with_fixture_removal_failure_for_test(
        mut self,
        capture: std::sync::Arc<std::sync::Mutex<Option<PathBuf>>>,
    ) -> Self {
        self.fixture_removal_failure_for_test = true;
        self.fixture_retention_capture_for_test = Some(capture);
        self
    }
}
pub trait TraceTarget {
    fn name(&self) -> &str;
    fn supports_observation(&self, kind: &ObservationKind) -> bool {
        let _ = kind;
        false
    }
    fn start(
        &mut self,
        fixture: &Path,
        dimensions: (u16, u16),
        environment: &BTreeMap<String, String>,
    ) -> Result<(), RunError>;
    fn wait_ready(
        &mut self,
        readiness: &crate::trace::Readiness,
        deadline: Instant,
    ) -> Result<(), RunError>;
    fn wait_for_observation(&mut self, deadline: Instant) -> Result<(), RunError> {
        let _ = deadline;
        Ok(())
    }
    fn apply(&mut self, event: &InputEvent) -> Result<(), RunError>;
    fn observe(&mut self, kind: &ObservationKind) -> Result<Observation, RunError>;
    fn shutdown(&mut self) -> Result<i32, RunError>;
}
#[derive(Clone, Default)]
pub struct Observation {
    pub screen: Option<String>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub bytes: Option<Vec<u8>>,
    pub exit_code: Option<i32>,
    pub value: Option<toml::Value>,
    pub dimensions: Option<(u16, u16)>,
}
impl Observation {
    pub fn screen(&self) -> Option<&str> {
        self.screen.as_deref()
    }
    pub fn stdout(&self) -> Option<&str> {
        self.stdout.as_deref()
    }
    pub fn stderr(&self) -> Option<&str> {
        self.stderr.as_deref()
    }
}
#[derive(Debug)]
pub struct RunResult {
    events: Vec<InputEvent>,
    ready: bool,
    matched: bool,
    sha: String,
    root: PathBuf,
    session: String,
    removed: bool,
    diagnostics: Vec<String>,
}
impl RunResult {
    pub fn applied_events(&self) -> &[InputEvent] {
        &self.events
    }
    pub fn readiness_satisfied_before_first_input(&self) -> bool {
        self.ready
    }
    pub fn observations_match(&self) -> bool {
        self.matched
    }
    pub fn baseline_sha(&self) -> &str {
        &self.sha
    }
    pub fn fixture_root(&self) -> &Path {
        &self.root
    }
    pub fn terminal_session_id(&self) -> &str {
        &self.session
    }
    pub fn fixture_removed(&self) -> bool {
        self.removed
    }
    pub fn observation_diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
}
/// Observations retained before shutdown removes the isolated fixture.
pub struct RunCapture {
    pub result: RunResult,
    pub bundle: crate::golden::ObservationBundle,
}
pub struct ScenarioRunner {
    opts: RunOptions,
}
impl ScenarioRunner {
    pub fn new(opts: RunOptions) -> Self {
        Self { opts }
    }
    pub fn validate_for_target(&self, t: &Trace, name: &str) -> Result<(), RunError> {
        self.validate_platform(t)?;
        if name == "tui-pty" && !matches!(t.readiness.observation, ObservationKind::TerminalScreen)
        {
            return Err(err(format!(
                "target {name} unsupported readiness observation {:?}",
                t.readiness.observation
            )));
        }
        let mut required = vec![t.readiness.observation.clone()];
        required.extend(t.observation_kinds());
        for k in required {
            let supports = target_supports(name, &k);
            if !supports {
                return Err(err(format!("target {name} unsupported observation {k:?}")));
            }
        }
        Ok(())
    }
    fn validate_platform(&self, t: &Trace) -> Result<(), RunError> {
        let selected = normalized_platform(&self.opts.platform);
        if t.platforms
            .iter()
            .any(|p| normalized_platform(p) == selected)
        {
            Ok(())
        } else {
            Err(err(format!(
                "scenario {} does not support platform {}",
                t.scenario_id, self.opts.platform
            )))
        }
    }
    pub fn run_with_target<T: TraceTarget>(
        &self,
        t: &Trace,
        target: &mut T,
    ) -> Result<RunResult, RunError> {
        self.run_capture_with_target(t, target)
            .map(|capture| capture.result)
    }
    pub fn run_capture_with_target<T: TraceTarget>(
        &self,
        t: &Trace,
        target: &mut T,
    ) -> Result<RunCapture, RunError> {
        self.validate_for_target(t, target.name())?;
        let mut records = Vec::new();
        let mut required = vec![t.readiness.observation.clone()];
        required.extend(t.observation_kinds());
        for kind in required {
            if !target.supports_observation(&kind) {
                return Err(err(format!(
                    "target {} unsupported observation {kind:?}",
                    target.name()
                )));
            }
        }
        let tmp = tempfile::Builder::new()
            .prefix("ira-parity-run-")
            .tempdir()
            .map_err(|e| err(e.to_string()))?;
        let root = tmp.path().to_path_buf();
        let env = crate::environment::ChildEnvironment::build_with_overrides(
            &crate::environment::EnvironmentPolicy::host(),
            &root,
            t.environment.iter().map(|(k, v)| (k.as_str(), v.as_str())),
        )
        .map_err(|e| err(e.to_string()))?;
        let values = env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        target.start(&root, (t.terminal.columns, t.terminal.rows), &values)?;
        let scenario_deadline = Instant::now() + self.opts.deadline;
        let deadline = Instant::now()
            + self
                .opts
                .readiness
                .min(Duration::from_millis(t.readiness.deadline_ms))
                .min(scenario_deadline.saturating_duration_since(Instant::now()));
        if let Err(e) = target.wait_ready(&t.readiness, deadline) {
            let _ = target.shutdown();
            return Err(RunError {
                message: e.to_string(),
                timeout: true,
                readiness: true,
                delivered: 0,
            });
        }
        let readiness_observation = match target.observe(&t.readiness.observation) {
            Ok(observation) => observation,
            Err(error) => {
                let cleanup = target.shutdown();
                return Err(with_cleanup_error(error, cleanup));
            }
        };
        let (ready, readiness_diagnostics) = compare_readiness(t, &readiness_observation, &[]);
        let mut applied = vec![];
        if !ready {
            let mut diagnostics = readiness_diagnostics;
            for expected in t.expected_observations() {
                match target.observe(&expected.kind) {
                    Ok(observation) => {
                        let (_, problems) = compare_expected_observation(
                            t,
                            expected,
                            &observation,
                            &[root.to_string_lossy().as_ref()],
                        );
                        diagnostics.extend(problems);
                    }
                    Err(error) => diagnostics.push(format!(
                        "scenario {} observation {:?} failed: {error}",
                        t.scenario_id, expected.kind
                    )),
                }
            }
            let cleanup = target.shutdown();
            if let Err(cleanup) = cleanup {
                return Err(err(format!(
                    "readiness observation mismatch; cleanup failed: {cleanup}"
                )));
            }
            if !t.events().is_empty() {
                return Err(err(format!(
                    "scenario {} readiness observation {:?} did not satisfy its condition before input: {}",
                    t.scenario_id,
                    t.readiness.observation,
                    diagnostics.join("; ")
                )));
            }
            return Ok(RunCapture {
                bundle: self.capture_bundle(t, records),
                result: RunResult {
                    events: applied,
                    ready: false,
                    matched: false,
                    sha: ORACLE_SHA.into(),
                    root,
                    session: format!("{:p}", target),
                    removed: true,
                    diagnostics,
                },
            });
        }
        for ev in t.events() {
            if let InputEvent::Key {
                phase: KeyPhase::Repeat | KeyPhase::Release,
                ..
            } = ev
            {
                let phase = if matches!(
                    ev,
                    InputEvent::Key {
                        phase: KeyPhase::Repeat,
                        ..
                    }
                ) {
                    "repeat"
                } else {
                    "release"
                };
                let _ = target.shutdown();
                return Err(RunError {
                    message: format!("unsupported key phase {phase}"),
                    timeout: false,
                    readiness: false,
                    delivered: applied.len(),
                });
            }
            if let Err(error) = target.apply(ev) {
                return Err(with_cleanup_error(error, target.shutdown()));
            }
            applied.push(ev.clone())
        }
        if let Err(error) = target.wait_for_observation(scenario_deadline) {
            let cleanup = target.shutdown();
            return Err(RunError {
                message: match cleanup {
                    Ok(_) => error.to_string(),
                    Err(cleanup) => format!("{error}; cleanup failed: {cleanup}"),
                },
                timeout: true,
                readiness: false,
                delivered: applied.len(),
            });
        }
        let mut matched = true;
        let mut diagnostics = readiness_diagnostics;
        for expected in t.expected_observations() {
            let o = match target.observe(&expected.kind) {
                Ok(o) => o,
                Err(e) => return Err(with_cleanup_error(e, target.shutdown())),
            };
            let record =
                match crate::golden::ObservationRecord::from_observation(&expected.kind, &o) {
                    Ok(record) => record,
                    Err(error) => return Err(with_cleanup_error(err(error), target.shutdown())),
                };
            records.push(record);
            let (this_match, problems) =
                compare_expected_observation(t, expected, &o, &[root.to_string_lossy().as_ref()]);
            matched &= this_match;
            diagnostics.extend(problems);
        }
        target.shutdown()?;
        Ok(RunCapture {
            bundle: self.capture_bundle(t, records),
            result: RunResult {
                events: applied,
                ready: true,
                matched,
                sha: ORACLE_SHA.into(),
                root,
                session: format!("{:p}", target),
                removed: true,
                diagnostics,
            },
        })
    }
    fn capture_bundle(
        &self,
        t: &Trace,
        records: Vec<crate::golden::ObservationRecord>,
    ) -> crate::golden::ObservationBundle {
        crate::golden::ObservationBundle::new(
            crate::golden::GoldenMetadata::from_trace(
                ORACLE_SHA,
                t,
                &self.opts.platform,
                &crate::golden::current_capture_date(),
            ),
            t.events().to_vec(),
            records,
        )
    }
    pub fn run_source_with_target<T: TraceTarget>(
        &self,
        s: &str,
        t: &mut T,
    ) -> Result<RunResult, RunError> {
        let trace = crate::trace::parse_trace(s).map_err(|e| err(e.to_string()))?;
        self.run_with_target(&trace, t)
    }
    pub fn run_pair(&self, a: &Trace, b: &Trace) -> Result<(RunResult, RunResult), RunError> {
        let mut x = crate::testing::ScriptedTarget::recording();
        let mut y = crate::testing::ScriptedTarget::recording();
        Ok((
            self.run_with_target(a, &mut x)?,
            self.run_with_target(b, &mut y)?,
        ))
    }
    pub fn run_oracle_trace(&self, t: &Trace) -> Result<RunResult, RunError> {
        self.run_oracle_mode(t, false).map(|capture| capture.result)
    }
    pub fn run_oracle_capture(&self, t: &Trace) -> Result<RunCapture, RunError> {
        self.run_oracle_mode(t, true)
    }
    fn run_oracle_mode(&self, t: &Trace, capture: bool) -> Result<RunCapture, RunError> {
        self.validate_for_target(t, "tui-pty")?;
        // Fail unsupported event/key profiles before allocating a worktree or
        // starting the oracle process.
        for event in t.events() {
            encode(event)?;
        }
        let mut baseline = BaselineResolver::new(Path::new(env!("CARGO_MANIFEST_DIR")))
            .with_expected_sha(ORACLE_SHA)
            .resolve_and_build()
            .map_err(|e| err(e.to_string()))?;
        let result = self.run_oracle_with_baseline(t, &baseline, capture);
        match baseline.cleanup() {
            Ok(()) => result,
            Err(cleanup) => match result {
                Ok(_) => Err(err(format!("baseline cleanup failed: {cleanup}"))),
                Err(mut primary) => {
                    primary.message =
                        format!("{}; baseline cleanup failed: {cleanup}", primary.message);
                    Err(primary)
                }
            },
        }
    }
    fn run_oracle_with_baseline(
        &self,
        t: &Trace,
        baseline: &crate::baseline::Baseline,
        capture: bool,
    ) -> Result<RunCapture, RunError> {
        let fixture = tempfile::Builder::new()
            .prefix("ira-parity-fixture-")
            .tempdir()
            .map_err(|e| err(format!("fixture creation failed: {e}")))?;
        let isolated = crate::environment::ChildEnvironment::build_with_overrides(
            &crate::environment::EnvironmentPolicy::host(),
            fixture.path(),
            t.environment
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        )
        .map_err(|e| err(format!("isolated environment failed: {e}")))?;
        let scenario_deadline = Instant::now() + self.opts.deadline;
        let mut cmd = portable_pty::CommandBuilder::new(baseline.executable_path());
        cmd.env_clear();
        cmd.cwd(fixture.path());
        for (k, v) in isolated.iter() {
            cmd.env(k, v);
        }
        let mut session_config = PtySessionConfig::new(self.opts.cleanup, self.opts.cleanup);
        if self.opts.fixture_removal_failure_for_test {
            session_config = session_config.with_fixture_remove_failure();
            if let Some(capture) = &self.opts.fixture_retention_capture_for_test {
                session_config = session_config.with_fixture_retention_capture(capture.clone());
            }
        }
        let mut lifecycle = PtySession::spawn(
            cmd,
            fixture,
            portable_pty::PtySize {
                rows: t.terminal.rows,
                cols: t.terminal.columns,
                pixel_width: 0,
                pixel_height: 0,
            },
            session_config,
        )
        .map_err(|e| err(e.to_string()))?;
        macro_rules! session_try {
            ($result:expr, $delivered:expr) => {
                match $result {
                    Ok(value) => value,
                    Err(error) => {
                        return Err(finish_failed_session(
                            &mut lifecycle,
                            error.to_string(),
                            false,
                            false,
                            $delivered,
                        ))
                    }
                }
            };
        }
        let fixture_path = lifecycle.fixture_path().to_path_buf();
        let deadline = (Instant::now() + Duration::from_millis(t.readiness.deadline_ms))
            .min(scenario_deadline);
        let mut screen = Vec::new();
        let volatile_root = fixture_path.to_string_lossy().into_owned();
        while Instant::now() < deadline {
            let next = match lifecycle
                .next_output((Instant::now() + Duration::from_millis(100)).min(deadline))
            {
                Ok(next) => next,
                Err(error) => {
                    return Err(finish_failed_session(&mut lifecycle, error, false, true, 0))
                }
            };
            if let Some(b) = next {
                screen = b;
                let observation = Observation {
                    screen: Some(String::from_utf8_lossy(&screen).into_owned()),
                    ..Observation::default()
                };
                if compare_readiness(t, &observation, &[&volatile_root]).0 {
                    break;
                }
            }
        }
        let observation = Observation {
            screen: Some(String::from_utf8_lossy(&screen).into_owned()),
            ..Observation::default()
        };
        let (ready, diagnostics) = compare_readiness(t, &observation, &[&volatile_root]);
        if !ready {
            return Err(finish_failed_session(
                &mut lifecycle,
                diagnostics.join("; "),
                true,
                true,
                0,
            ));
        }
        let mut applied = Vec::new();
        for ev in t.events() {
            if Instant::now() >= scenario_deadline {
                return Err(finish_failed_session(
                    &mut lifecycle,
                    "scenario deadline exceeded before input delivery",
                    true,
                    false,
                    applied.len(),
                ));
            }
            let bytes = session_try!(encode(ev), applied.len());
            session_try!(lifecycle.write_input(&bytes), applied.len());
            if let InputEvent::Resize { columns, rows } = ev {
                session_try!(
                    lifecycle.resize(portable_pty::PtySize {
                        rows: *rows,
                        cols: *columns,
                        pixel_width: 0,
                        pixel_height: 0,
                    }),
                    applied.len()
                );
            }
            applied.push(ev.clone())
        }
        // Interactive TUI processes intentionally remain alive after replay.
        // Wait for a new output observation under a bounded deadline instead
        // of incorrectly waiting for normal process exit.
        let end = scenario_deadline;
        if !t.expected_observations().is_empty() {
            while Instant::now() < end {
                match lifecycle.next_output(end) {
                    Ok(Some(bytes)) => {
                        screen = bytes;
                        if session_try!(
                            expected_observations_match(
                                t,
                                &screen,
                                session_try!(lifecycle.master(), applied.len()),
                                &fixture_path,
                                &[&volatile_root],
                            ),
                            applied.len()
                        ) {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        return Err(finish_failed_session(
                            &mut lifecycle,
                            error,
                            false,
                            false,
                            applied.len(),
                        ))
                    }
                }
            }
        }
        let (matched, diagnostics) = session_try!(
            expected_observations_compare(
                t,
                &screen,
                session_try!(lifecycle.master(), applied.len()),
                &fixture_path,
                &[&volatile_root],
            ),
            applied.len()
        );
        let records = session_try!(
            capture_oracle_observations(
                t,
                &screen,
                session_try!(lifecycle.master(), applied.len()),
                &fixture_path
            ),
            applied.len()
        );
        let shutdown = if matched || capture {
            lifecycle.shutdown().map(|_| ())
        } else {
            lifecycle
                .shutdown_with_primary_error(diagnostics.join("; "))
                .map(|_| ())
        };
        if let Err(error) = shutdown {
            return Err(RunError {
                message: error.to_string(),
                timeout: Instant::now() >= end,
                readiness: false,
                delivered: applied.len(),
            });
        }
        if !matched && !capture {
            return Err(RunError {
                message: diagnostics.join("; "),
                timeout: Instant::now() >= end,
                readiness: false,
                delivered: applied.len(),
            });
        }
        Ok(RunCapture {
            bundle: self.capture_bundle(t, records),
            result: RunResult {
                events: applied,
                ready: true,
                matched,
                sha: ORACLE_SHA.into(),
                root: fixture_path,
                session: format!("pty-{:?}", std::thread::current().id()),
                removed: true,
                diagnostics,
            },
        })
    }
}
fn normalized_platform(platform: &str) -> &str {
    match platform {
        "windows" | "win32" | "windows-msvc" => "windows-msvc",
        other => other,
    }
}
fn target_supports(name: &str, kind: &ObservationKind) -> bool {
    if name == "tui-pty" {
        matches!(
            kind,
            ObservationKind::TerminalScreen
                | ObservationKind::Filesystem { .. }
                | ObservationKind::PersistedBytes { .. }
        )
    } else {
        true
    }
}
fn compare_readiness(
    trace: &Trace,
    observation: &Observation,
    roots: &[&str],
) -> (bool, Vec<String>) {
    let actual = match &trace.readiness.observation {
        ObservationKind::TerminalScreen => observation.screen.as_deref().map(str::to_owned),
        ObservationKind::Process => observation.stdout.as_ref().map(|stdout| {
            format!(
                "{stdout}{}",
                observation.stderr.as_deref().unwrap_or_default()
            )
        }),
        ObservationKind::Filesystem { .. } | ObservationKind::PersistedBytes { .. } => observation
            .bytes
            .as_ref()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned()),
        ObservationKind::DomainSnapshot { .. } => {
            observation.value.as_ref().map(ToString::to_string)
        }
    };
    let Some(actual) = actual else {
        return (
            false,
            vec![format!(
                "scenario {} readiness observation {:?} missing; expected {:?}, actual <missing>",
                trace.scenario_id, trace.readiness.observation, trace.readiness.condition.contains
            )],
        );
    };
    let actual = if matches!(trace.readiness.observation, ObservationKind::TerminalScreen) {
        crate::normalize::normalize_screen(&actual, roots)
    } else {
        actual
    };
    let mut diagnostics = Vec::new();
    for expected in &trace.readiness.condition.contains {
        if !actual.contains(expected) {
            diagnostics.push(format!(
                "scenario {} observation {:?} readiness mismatch: expected to contain {expected:?}, actual {actual:?}",
                trace.scenario_id, trace.readiness.observation
            ));
        }
    }
    (diagnostics.is_empty(), diagnostics)
}
fn compare_expected_observation(
    trace: &Trace,
    expected: &crate::trace::ExpectedObservation,
    actual: &Observation,
    roots: &[&str],
) -> (bool, Vec<String>) {
    use ObservationKind::*;
    let mut diagnostics = Vec::new();
    let expect = expected.expect.as_table();
    match &expected.kind {
        TerminalScreen => {
            let screen = actual.screen.as_deref();
            if let Some(contains) = expect
                .and_then(|table| table.get("contains"))
                .and_then(toml::Value::as_array)
            {
                for (field, needle) in contains.iter().filter_map(toml::Value::as_str).enumerate() {
                    if !screen
                        .map(|screen| {
                            crate::normalize::normalize_screen(screen, roots).contains(needle)
                        })
                        .unwrap_or(false)
                    {
                        diagnostics.push(format!(
                            "scenario {} observation terminal_screen.contains[{field}] mismatch: expected {needle:?}, actual {}",
                            trace.scenario_id,
                            screen.unwrap_or("<missing>")
                        ));
                    }
                }
            }
            if let Some(dimensions) = expect
                .and_then(|table| table.get("dimensions"))
                .and_then(toml::Value::as_table)
            {
                for (field, index) in [("columns", 0), ("rows", 1)] {
                    if let Some(wanted) = dimensions.get(field).and_then(toml::Value::as_integer) {
                        let got = actual.dimensions.map(|pair| {
                            toml::Value::Integer(if index == 0 { pair.0 } else { pair.1 } as i64)
                        });
                        record_comparison(
                            &mut diagnostics,
                            trace,
                            expected,
                            field,
                            toml::Value::Integer(wanted),
                            got,
                        );
                    }
                }
            }
        }
        Process => {
            for (field, got) in [
                (
                    "exit_code",
                    actual
                        .exit_code
                        .map(|code| toml::Value::Integer(code.into())),
                ),
                ("stdout", actual.stdout.clone().map(toml::Value::String)),
                ("stderr", actual.stderr.clone().map(toml::Value::String)),
            ] {
                if let Some(wanted) = expect.and_then(|table| table.get(field)) {
                    record_comparison(
                        &mut diagnostics,
                        trace,
                        expected,
                        field,
                        wanted.clone(),
                        got,
                    );
                }
            }
        }
        Filesystem { .. } | PersistedBytes { .. } => {
            if let Some(wanted) = expect.and_then(|table| table.get("bytes")) {
                let bytes = wanted.as_array().map(|items| {
                    items
                        .iter()
                        .filter_map(toml::Value::as_integer)
                        .map(|byte| byte as u8)
                        .collect::<Vec<_>>()
                });
                let got = actual.bytes.as_ref().map(|bytes| {
                    toml::Value::Array(
                        bytes
                            .iter()
                            .map(|byte| toml::Value::Integer(i64::from(*byte)))
                            .collect(),
                    )
                });
                record_comparison(
                    &mut diagnostics,
                    trace,
                    expected,
                    "bytes",
                    wanted.clone(),
                    got,
                );
                if bytes.is_none() {
                    diagnostics.push(format!(
                        "scenario {} observation {:?}.bytes expected a byte array",
                        trace.scenario_id, expected.kind
                    ));
                }
            } else if let Some(wanted) = expect.and_then(|table| table.get("text")) {
                record_comparison(
                    &mut diagnostics,
                    trace,
                    expected,
                    "text",
                    wanted.clone(),
                    actual.bytes.as_ref().map(|bytes| {
                        toml::Value::String(String::from_utf8_lossy(bytes).into_owned())
                    }),
                );
            } else if !expect.is_some_and(|table| table.is_empty()) {
                diagnostics.push(format!(
                    "scenario {} observation {:?} has unsupported expected fields",
                    trace.scenario_id, expected.kind
                ));
            }
        }
        DomainSnapshot { .. } => {
            if let Some(wanted) = expect.map(|table| toml::Value::Table(table.clone())) {
                record_comparison(
                    &mut diagnostics,
                    trace,
                    expected,
                    "value",
                    wanted,
                    actual.value.clone(),
                );
            }
        }
    }
    (diagnostics.is_empty(), diagnostics)
}
fn record_comparison(
    diagnostics: &mut Vec<String>,
    trace: &Trace,
    expected: &crate::trace::ExpectedObservation,
    field: &str,
    wanted: toml::Value,
    got: Option<toml::Value>,
) {
    if got.as_ref() != Some(&wanted) {
        diagnostics.push(format!(
            "scenario {} observation {:?}.{field} mismatch: expected {wanted}, actual {}",
            trace.scenario_id,
            expected.kind,
            got.map(|value| value.to_string())
                .unwrap_or_else(|| "<missing>".into())
        ));
    }
}
fn with_cleanup_error(primary: RunError, cleanup: Result<i32, RunError>) -> RunError {
    match cleanup {
        Ok(_) => primary,
        Err(cleanup) => err(format!("{}; cleanup failed: {cleanup}", primary.message)),
    }
}
fn expected_observations_match(
    trace: &Trace,
    bytes: &[u8],
    master: &dyn portable_pty::MasterPty,
    fixture: &Path,
    volatile_roots: &[&str],
) -> Result<bool, RunError> {
    Ok(expected_observations_compare(trace, bytes, master, fixture, volatile_roots)?.0)
}
fn expected_observations_compare(
    trace: &Trace,
    bytes: &[u8],
    master: &dyn portable_pty::MasterPty,
    fixture: &Path,
    volatile_roots: &[&str],
) -> Result<(bool, Vec<String>), RunError> {
    let mut matched = true;
    let mut diagnostics = Vec::new();
    let screen = String::from_utf8_lossy(bytes).into_owned();
    for expected in trace.expected_observations() {
        let mut observation = Observation::default();
        match &expected.kind {
            ObservationKind::TerminalScreen => {
                observation.screen = Some(screen.clone());
                let size = master.get_size().map_err(|e| err(e.to_string()))?;
                observation.dimensions = Some((size.cols, size.rows));
            }
            ObservationKind::Filesystem { relative_path }
            | ObservationKind::PersistedBytes { relative_path } => {
                observation.bytes = read_fixture_file(fixture, relative_path)?;
            }
            ObservationKind::Process | ObservationKind::DomainSnapshot { .. } => {
                return Err(err(format!(
                    "target tui-pty cannot observe {:?}",
                    expected.kind
                )));
            }
        }
        let (this_match, problems) =
            compare_expected_observation(trace, expected, &observation, volatile_roots);
        matched &= this_match;
        diagnostics.extend(problems);
    }
    Ok((matched, diagnostics))
}
fn capture_oracle_observations(
    trace: &Trace,
    bytes: &[u8],
    master: &dyn portable_pty::MasterPty,
    fixture: &Path,
) -> Result<Vec<crate::golden::ObservationRecord>, RunError> {
    let mut records = Vec::new();
    for expected in trace.expected_observations() {
        let mut observation = Observation::default();
        match &expected.kind {
            ObservationKind::TerminalScreen => {
                observation.screen = Some(String::from_utf8_lossy(bytes).into_owned());
                let size = master.get_size().map_err(|e| err(e.to_string()))?;
                observation.dimensions = Some((size.cols, size.rows));
            }
            ObservationKind::Filesystem { relative_path }
            | ObservationKind::PersistedBytes { relative_path } => {
                observation.bytes = read_fixture_file(fixture, relative_path)?
            }
            _ => return Err(err("unsupported TUI capture observation")),
        }
        records.push(
            crate::golden::ObservationRecord::from_observation(&expected.kind, &observation)
                .map_err(err)?,
        );
    }
    Ok(records)
}
fn read_fixture_file(fixture: &Path, relative: &str) -> Result<Option<Vec<u8>>, RunError> {
    use std::path::Component;
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(err(format!(
            "fixture observation path escapes root: {relative:?}"
        )));
    }
    let root = std::fs::canonicalize(fixture).map_err(|e| err(e.to_string()))?;
    let candidate = fixture.join(relative);
    if !candidate.exists() {
        return Ok(None);
    }
    let canonical = std::fs::canonicalize(&candidate).map_err(|e| err(e.to_string()))?;
    if !canonical.starts_with(&root) {
        return Err(err(format!(
            "fixture observation path escapes root: {relative:?}"
        )));
    }
    std::fs::read(canonical)
        .map(Some)
        .map_err(|e| err(format!("fixture observation read failed: {e}")))
}
fn encode(e: &InputEvent) -> Result<Vec<u8>, RunError> {
    use crate::trace::{KeyCode::*, NamedKey::*};
    Ok(match e {
        InputEvent::Text { value } => value.as_bytes().to_vec(),
        InputEvent::Paste { value } => {
            if cfg!(windows) {
                return Err(err(
                    "unsupported paste: frozen Windows native Crossterm has no Paste event path",
                ));
            }
            if value.contains("\x1b[201~") {
                return Err(err(
                    "unsupported paste payload: contains bracketed-paste closing marker",
                ));
            }
            [
                b"\x1b[200~".as_slice(),
                value.as_bytes(),
                b"\x1b[201~".as_slice(),
            ]
            .concat()
        }
        InputEvent::Resize { .. } => vec![],
        InputEvent::Key {
            code,
            phase: KeyPhase::Press,
            modifiers,
            ..
        } => {
            if !modifiers.is_empty() {
                return Err(err("unsupported key modifier combination"));
            }
            match code {
                Character { character } => character.to_string().into_bytes(),
                Named(Enter) => vec![13],
                Named(Escape) => vec![27],
                Named(Tab) => vec![9],
                Named(Backspace) => vec![127],
                Named(Up) => b"\x1b[A".to_vec(),
                Named(Down) => b"\x1b[B".to_vec(),
                Named(Left) => b"\x1b[D".to_vec(),
                Named(Right) => b"\x1b[C".to_vec(),
                _ => return Err(err("unsupported key code")),
            }
        }
        InputEvent::Key { phase, .. } => {
            return Err(err(format!(
                "unsupported key phase {}",
                format!("{phase:?}").to_lowercase()
            )))
        }
    })
}
fn err(s: impl Into<String>) -> RunError {
    RunError {
        message: s.into(),
        timeout: false,
        readiness: false,
        delivered: 0,
    }
}

fn finish_failed_session(
    session: &mut PtySession,
    primary: impl Into<String>,
    timeout: bool,
    readiness: bool,
    delivered: usize,
) -> RunError {
    let primary = primary.into();
    let message = match session.shutdown_with_primary_error(primary.clone()) {
        Ok(_) => primary,
        Err(error) => error.to_string(),
    };
    RunError {
        message,
        timeout,
        readiness,
        delivered,
    }
}
