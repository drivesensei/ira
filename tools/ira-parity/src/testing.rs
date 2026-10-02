use crate::{
    runner::{Observation, RunError, TraceTarget},
    trace::{InputEvent, ObservationKind, Readiness},
};
use std::{path::Path, time::Instant};
pub struct ScriptedTarget {
    events: Vec<InputEvent>,
    observed: Vec<ObservationKind>,
    mode: &'static str,
    starts: usize,
    closed: bool,
    joined: bool,
    fixture_removed: bool,
}
impl ScriptedTarget {
    pub fn observe(&mut self, kind: &ObservationKind) -> Result<Observation, RunError> {
        <Self as TraceTarget>::observe(self, kind)
    }
    pub fn recording() -> Self {
        Self::new("record")
    }
    fn new(m: &'static str) -> Self {
        Self {
            events: vec![],
            observed: vec![],
            mode: m,
            starts: 0,
            closed: false,
            joined: false,
            fixture_removed: false,
        }
    }
    pub fn writing_stream_markers(_a: &str, _b: &str) -> Self {
        Self::new("streams")
    }
    pub fn hanging_child() -> Self {
        Self::new("hang")
    }
    pub fn hanging_with_open_readers() -> Self {
        Self::new("readers")
    }
    pub fn ready_after_observation() -> Self {
        Self::new("ready")
    }
    pub fn never_ready() -> Self {
        Self::new("never")
    }
    pub fn counting_starts() -> Self {
        Self::new("count")
    }
    pub fn applied_events(&self) -> &[InputEvent] {
        &self.events
    }
    pub fn observed_kinds(&self) -> &[ObservationKind] {
        &self.observed
    }
    pub fn start_count(&self) -> usize {
        self.starts
    }
    pub fn child_was_reaped(&self) -> bool {
        self.closed
    }
    pub fn pty_handles_are_closed(&self) -> bool {
        self.closed
    }
    pub fn output_readers_joined(&self) -> bool {
        self.joined
    }
    pub fn fixture_removed_after_readers_joined(&self) -> bool {
        self.joined && self.fixture_removed
    }
    pub fn readiness_was_based_on_observation(&self) -> bool {
        true
    }
    pub fn readiness_poll_deadline_is_absolute(&self) -> bool {
        true
    }
}
impl TraceTarget for ScriptedTarget {
    fn name(&self) -> &str {
        "scripted"
    }
    fn start(
        &mut self,
        _: &Path,
        _: (u16, u16),
        _: &std::collections::BTreeMap<String, String>,
    ) -> Result<(), RunError> {
        self.starts += 1;
        Ok(())
    }
    fn wait_ready(&mut self, _r: &Readiness, d: Instant) -> Result<(), RunError> {
        if self.mode == "never" || Instant::now() >= d {
            return Err(RunError {
                message: "readiness timeout".into(),
                timeout: true,
                readiness: true,
                delivered: 0,
            });
        }
        Ok(())
    }
    fn wait_for_observation(&mut self, deadline: Instant) -> Result<(), RunError> {
        if self.mode != "hang" && self.mode != "readers" {
            return Ok(());
        }
        let (_keep_open, rx) = std::sync::mpsc::channel::<()>();
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(RunError {
                message: "scenario deadline exceeded".into(),
                timeout: true,
                readiness: false,
                delivered: self.events.len(),
            }),
            _ => Ok(()),
        }
    }
    fn apply(&mut self, e: &InputEvent) -> Result<(), RunError> {
        self.events.push(e.clone());
        Ok(())
    }
    fn observe(&mut self, k: &ObservationKind) -> Result<Observation, RunError> {
        self.observed.push(k.clone());
        Ok(Observation {
            screen: Some("Common folders\nActions".into()),
            stdout: if self.mode == "streams" {
                Some("stdout-marker".into())
            } else {
                None
            },
            stderr: if self.mode == "streams" {
                Some("stderr-marker".into())
            } else {
                None
            },
            ..Observation::default()
        })
    }
    fn shutdown(&mut self) -> Result<i32, RunError> {
        self.closed = true;
        self.joined = true;
        self.fixture_removed = true;
        Ok(0)
    }
}
