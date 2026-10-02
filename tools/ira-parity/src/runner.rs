use crate::{
    baseline::{BaselineResolver, ORACLE_SHA},
    trace::{InputEvent, KeyPhase, ObservationKind, Trace},
};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
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
}
impl Default for RunOptions {
    fn default() -> Self {
        Self {
            platform: std::env::consts::OS.into(),
            deadline: Duration::from_secs(30),
            cleanup: Duration::from_secs(2),
            approved: None,
            readiness: Duration::from_secs(15),
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
}
pub trait TraceTarget {
    fn name(&self) -> &str;
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
    fn apply(&mut self, event: &InputEvent) -> Result<(), RunError>;
    fn observe(&mut self, kind: &ObservationKind) -> Result<Observation, RunError>;
    fn shutdown(&mut self) -> Result<i32, RunError>;
}
#[derive(Clone, Default)]
pub struct Observation {
    pub(crate) screen: Option<String>,
    pub(crate) stdout: Option<String>,
    pub(crate) stderr: Option<String>,
    pub bytes: Option<Vec<u8>>,
    pub exit_code: Option<i32>,
}
impl Observation {
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
}
pub struct ScenarioRunner {
    opts: RunOptions,
}
impl ScenarioRunner {
    pub fn new(opts: RunOptions) -> Self {
        Self { opts }
    }
    pub fn validate_for_target(&self, t: &Trace, name: &str) -> Result<(), RunError> {
        for k in t.observation_kinds() {
            let supports = if name == "tui-pty" {
                !matches!(k, ObservationKind::DomainSnapshot { .. })
            } else {
                true
            };
            if !supports {
                return Err(err(format!("target {name} unsupported observation {k:?}")));
            }
        }
        Ok(())
    }
    pub fn run_with_target<T: TraceTarget>(
        &self,
        t: &Trace,
        target: &mut T,
    ) -> Result<RunResult, RunError> {
        self.validate_for_target(t, target.name())?;
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
        let deadline = Instant::now()
            + self
                .opts
                .readiness
                .min(Duration::from_millis(t.readiness.deadline_ms));
        if let Err(e) = target.wait_ready(&t.readiness, deadline) {
            let _ = target.shutdown();
            return Err(RunError {
                message: e.to_string(),
                timeout: true,
                readiness: true,
                delivered: 0,
            });
        }
        let mut applied = vec![];
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
            target.apply(ev)?;
            applied.push(ev.clone())
        }
        let mut matched = true;
        for k in t.observation_kinds() {
            let o = target.observe(&k)?;
            if let Some(s) = o.screen {
                for needle in &t.readiness.condition.contains {
                    matched &= s.contains(needle)
                }
            }
        }
        target.shutdown()?;
        Ok(RunResult {
            events: applied,
            ready: true,
            matched,
            sha: ORACLE_SHA.into(),
            root,
            session: format!("{:p}", target),
            removed: true,
        })
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
        // Fail unsupported event/key profiles before allocating a worktree or
        // starting the oracle process.
        for event in t.events() {
            encode(event)?;
        }
        let baseline = BaselineResolver::new(Path::new(env!("CARGO_MANIFEST_DIR")))
            .with_expected_sha(ORACLE_SHA)
            .resolve_and_build()
            .map_err(|e| err(e.to_string()))?;
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
        let sys = portable_pty::native_pty_system();
        let pair = sys
            .openpty(portable_pty::PtySize {
                rows: t.terminal.rows,
                cols: t.terminal.columns,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| err(e.to_string()))?;
        let mut cmd = portable_pty::CommandBuilder::new(baseline.executable_path());
        cmd.env_clear();
        cmd.cwd(fixture.path());
        for (k, v) in isolated.iter() {
            cmd.env(k, v);
        }
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| err(e.to_string()))?;
        // portable-pty's parent-side slave handle keeps the reader from ever
        // observing EOF after the child exits if it remains open.
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| err(e.to_string()))?;
        let (tx, rx) = mpsc::channel();
        let reader_task = std::thread::spawn(move || {
            let mut b = [0u8; 4096];
            let mut all = Vec::new();
            loop {
                match reader.read(&mut b) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        all.extend_from_slice(&b[..n]);
                        let _ = tx.send(all.clone());
                    }
                }
            }
        });
        let deadline = Instant::now() + Duration::from_millis(t.readiness.deadline_ms);
        let mut screen = Vec::new();
        while Instant::now() < deadline {
            if let Ok(b) = rx.recv_timeout(Duration::from_millis(100)) {
                screen = b;
                let text = String::from_utf8_lossy(&screen);
                if t.readiness
                    .condition
                    .contains
                    .iter()
                    .all(|s| text.contains(s))
                {
                    break;
                }
            }
        }
        if !t
            .readiness
            .condition
            .contains
            .iter()
            .all(|s| String::from_utf8_lossy(&screen).contains(s))
        {
            let cleanup = terminate_child(&mut child, self.opts.cleanup);
            let reader_cleanup = reader_task
                .join()
                .map_err(|_| "PTY output reader panicked".to_string());
            return Err(RunError {
                message: match (cleanup, reader_cleanup) {
                    (Ok(()), Ok(())) => "readiness timeout before input".into(),
                    (c, r) => {
                        format!("readiness timeout before input; cleanup: {c:?}; reader: {r:?}")
                    }
                },
                timeout: true,
                readiness: true,
                delivered: 0,
            });
        }
        let mut writer = pair.master.take_writer().map_err(|e| err(e.to_string()))?;
        let mut applied = Vec::new();
        for ev in t.events() {
            let bytes = encode(ev)?;
            writer.write_all(&bytes).map_err(|e| err(e.to_string()))?;
            if let InputEvent::Resize { columns, rows } = ev {
                pair.master
                    .resize(portable_pty::PtySize {
                        rows: *rows,
                        cols: *columns,
                        pixel_width: 0,
                        pixel_height: 0,
                    })
                    .map_err(|e| err(e.to_string()))?
            }
            applied.push(ev.clone())
        }
        // Interactive TUI processes intentionally remain alive after replay.
        // Wait for a new output observation under a bounded deadline instead
        // of incorrectly waiting for normal process exit.
        let end = Instant::now() + self.opts.deadline;
        if !t.expected_observations().is_empty() {
            while Instant::now() < end {
                match rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
                    Ok(bytes) => {
                        screen = bytes;
                        if expected_observations_match(t, &screen, pair.master.as_ref())? {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
        let matched = expected_observations_match(t, &screen, pair.master.as_ref())?;
        let _ = writer.flush();
        terminate_child(&mut child, self.opts.cleanup).map_err(|e| RunError {
            message: e,
            timeout: true,
            readiness: false,
            delivered: applied.len(),
        })?;
        drop(writer);
        reader_task
            .join()
            .map_err(|_| err("PTY output reader panicked"))?;
        Ok(RunResult {
            events: applied,
            ready: true,
            matched,
            sha: ORACLE_SHA.into(),
            root: fixture.path().to_path_buf(),
            session: format!("pty-{:?}", std::thread::current().id()),
            removed: true,
        })
    }
}
fn expected_observations_match(
    trace: &Trace,
    bytes: &[u8],
    master: &dyn portable_pty::MasterPty,
) -> Result<bool, RunError> {
    let screen = String::from_utf8_lossy(bytes);
    let mut matched = true;
    for expected in trace.expected_observations() {
        if let Some(fields) = expected.expect.as_table() {
            if let Some(values) = fields.get("contains").and_then(toml::Value::as_array) {
                for value in values.iter().filter_map(toml::Value::as_str) {
                    matched &= screen.contains(value);
                }
            }
            if let Some(dimensions) = fields.get("dimensions") {
                let actual = master.get_size().map_err(|e| err(e.to_string()))?;
                if let Some(columns) = dimensions.get("columns").and_then(toml::Value::as_integer) {
                    matched &= actual.cols == columns as u16;
                }
                if let Some(rows) = dimensions.get("rows").and_then(toml::Value::as_integer) {
                    matched &= actual.rows == rows as u16;
                }
            }
        }
    }
    Ok(matched)
}
fn terminate_child(
    child: &mut Box<dyn portable_pty::Child + Send + Sync>,
    deadline: Duration,
) -> Result<(), String> {
    child
        .kill()
        .map_err(|e| format!("child kill failed: {e}"))?;
    let end = Instant::now() + deadline;
    loop {
        if child
            .try_wait()
            .map_err(|e| format!("child reap failed: {e}"))?
            .is_some()
        {
            return Ok(());
        }
        if Instant::now() >= end {
            return Err("child cleanup deadline exceeded".into());
        }
        std::thread::yield_now();
    }
}
fn encode(e: &InputEvent) -> Result<Vec<u8>, RunError> {
    use crate::trace::{KeyCode::*, NamedKey::*};
    Ok(match e {
        InputEvent::Text { value } | InputEvent::Paste { value } => value.as_bytes().to_vec(),
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
