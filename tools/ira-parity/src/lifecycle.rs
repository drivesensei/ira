//! Owned PTY process/session lifecycle used by the live oracle adapter.
//!
//! Teardown is deliberately staged: release input/slave ownership, terminate
//! and reap the direct child, close the PTY master to unblock the reader, join
//! it under a separate deadline, then remove the fixture.

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Condvar, Mutex, OnceLock,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupEvent {
    InputClosed,
    ChildReaped,
    HandlesClosed,
    ReaderJoined,
    FixtureRemovalAttempted,
}

#[derive(Clone, Debug)]
pub struct PtySessionConfig {
    pub cleanup_deadline: Duration,
    pub reader_join_deadline: Duration,
    reap_poll_gate: Option<Arc<AtomicBool>>,
    reader_failure_after_bytes: Option<usize>,
    fixture_remove_failure: bool,
    fixture_retention_capture: Option<Arc<Mutex<Option<PathBuf>>>>,
}

impl PtySessionConfig {
    pub fn new(cleanup_deadline: Duration, reader_join_deadline: Duration) -> Self {
        Self {
            cleanup_deadline,
            reader_join_deadline,
            reap_poll_gate: None,
            reader_failure_after_bytes: None,
            fixture_remove_failure: false,
            fixture_retention_capture: None,
        }
    }

    /// Deterministic scheduling seam: signals still run, but observed reaping
    /// (foreground and deferred) waits until this gate opens. Normal runs have no gate.
    pub fn with_reap_poll_gate(mut self, gate: Arc<AtomicBool>) -> Self {
        self.reap_poll_gate = Some(gate);
        self
    }

    pub fn with_reader_failure_after_bytes(mut self, n: usize) -> Self {
        self.reader_failure_after_bytes = Some(n);
        self
    }

    pub fn with_fixture_remove_failure(mut self) -> Self {
        self.fixture_remove_failure = true;
        self
    }

    pub fn with_fixture_retention_capture(mut self, capture: Arc<Mutex<Option<PathBuf>>>) -> Self {
        self.fixture_retention_capture = Some(capture);
        self
    }
}

#[derive(Clone, Debug)]
pub struct ShutdownReport {
    pub child_reaped: bool,
    pub reader_joined: bool,
    pub handles_closed_before_fixture_removed: bool,
    pub events: Vec<CleanupEvent>,
}

#[derive(Debug)]
pub struct LifecycleError {
    primary: Option<String>,
    cleanup: String,
    events: Vec<CleanupEvent>,
    child_reaped: bool,
}

impl LifecycleError {
    pub fn primary(&self) -> &str {
        self.primary.as_deref().unwrap_or("")
    }
    pub fn cleanup(&self) -> &str {
        &self.cleanup
    }
    pub fn events(&self) -> &[CleanupEvent] {
        &self.events
    }
    pub fn child_reaped(&self) -> bool {
        self.child_reaped
    }
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.primary.as_deref() {
            Some(primary) if self.cleanup.is_empty() => write!(f, "{primary}"),
            Some(primary) => write!(f, "{primary}; cleanup: {}", self.cleanup),
            None => write!(f, "cleanup: {}", self.cleanup),
        }
    }
}
impl std::error::Error for LifecycleError {}

enum ReaderMessage {
    Bytes(Vec<u8>),
    Failed(String),
    Closed,
}

pub struct PtySession {
    fixture: Option<tempfile::TempDir>,
    fixture_path: PathBuf,
    master: Option<Box<dyn MasterPty + Send>>,
    slave: Option<Box<dyn portable_pty::SlavePty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    reader: Option<JoinHandle<()>>,
    rx: Receiver<ReaderMessage>,
    output: Vec<u8>,
    reader_closed: bool,
    reader_error: Option<String>,
    config: PtySessionConfig,
    fixture_removal_attempted: bool,
    fixture_remove_failure: bool,
    shutdown_attempted: bool,
    cleanup_permit: Option<CleanupPermit>,
}

impl PtySession {
    pub fn spawn(
        command: CommandBuilder,
        fixture: tempfile::TempDir,
        size: PtySize,
        config: PtySessionConfig,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let failure_after = config.reader_failure_after_bytes;
        let fixture_remove_failure = config.fixture_remove_failure;
        Self::spawn_configured(
            command,
            fixture,
            size,
            config,
            failure_after,
            fixture_remove_failure,
        )
    }

    fn spawn_configured(
        command: CommandBuilder,
        fixture: tempfile::TempDir,
        size: PtySize,
        config: PtySessionConfig,
        reader_failure_after_bytes: Option<usize>,
        fixture_remove_failure: bool,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let cleanup_permit = CleanupPermit::acquire()?;
        let fixture_path = fixture.path().to_path_buf();
        let pair = portable_pty::native_pty_system().openpty(size)?;
        let reader = pair.master.try_clone_reader()?;
        let child = pair.slave.spawn_command(command)?;
        let (tx, rx) = mpsc::sync_channel(4096);
        let reader_tx = tx.clone();
        let failure_after = reader_failure_after_bytes;
        let reader_task = std::thread::spawn(move || {
            let mut reader = reader;
            let mut buffer = [0u8; 4096];
            let mut total = 0usize;
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        let n = failure_after.map_or(n, |limit| n.min(limit.saturating_sub(total)));
                        if n > 0 {
                            total += n;
                            if reader_tx
                                .send(ReaderMessage::Bytes(buffer[..n].to_vec()))
                                .is_err()
                            {
                                break;
                            }
                        }
                        if failure_after.is_some_and(|limit| total >= limit) {
                            let _ = reader_tx
                                .send(ReaderMessage::Failed("injected PTY reader failure".into()));
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = reader_tx
                            .send(ReaderMessage::Failed(format!("PTY reader failed: {error}")));
                        break;
                    }
                }
            }
            let _ = reader_tx.send(ReaderMessage::Closed);
        });
        Ok(Self {
            fixture: Some(fixture),
            fixture_path,
            master: Some(pair.master),
            slave: Some(pair.slave),
            writer: None,
            child: Some(child),
            reader: Some(reader_task),
            rx,
            output: Vec::new(),
            reader_closed: false,
            reader_error: None,
            config,
            fixture_removal_attempted: false,
            fixture_remove_failure,
            shutdown_attempted: false,
            cleanup_permit: Some(cleanup_permit),
        })
    }

    pub fn fixture_path(&self) -> &Path {
        &self.fixture_path
    }

    pub fn master(&self) -> Result<&dyn MasterPty, String> {
        self.master
            .as_deref()
            .map(|master| master as &dyn MasterPty)
            .ok_or_else(|| "PTY master is closed".into())
    }

    pub fn write_input(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.take_writer()?
            .write_all(bytes)
            .map_err(|e| e.to_string())
    }

    pub fn resize(&self, size: PtySize) -> Result<(), String> {
        self.master()?.resize(size).map_err(|e| e.to_string())
    }

    pub fn next_output(&mut self, deadline: Instant) -> Result<Option<Vec<u8>>, String> {
        if let Some(error) = &self.reader_error {
            return Err(error.clone());
        }
        if self.reader_closed {
            return Ok(None);
        }
        match self
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            Ok(ReaderMessage::Bytes(bytes)) => {
                self.output.extend_from_slice(&bytes);
                Ok(Some(self.output.clone()))
            }
            Ok(ReaderMessage::Failed(error)) => {
                self.reader_error = Some(error.clone());
                Err(error)
            }
            Ok(ReaderMessage::Closed) => {
                self.reader_closed = true;
                Ok(None)
            }
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                self.reader_closed = true;
                Ok(None)
            }
        }
    }

    pub fn output(&self) -> &[u8] {
        &self.output
    }

    pub fn take_writer(&mut self) -> Result<&mut (dyn Write + Send + '_), String> {
        if self.writer.is_none() {
            let master = self.master.as_ref().ok_or("PTY master is closed")?;
            self.writer = Some(master.take_writer().map_err(|e| e.to_string())?);
        }
        self.writer
            .as_mut()
            .map(|writer| writer.as_mut() as &mut (dyn Write + Send + '_))
            .ok_or_else(|| "PTY writer unavailable".into())
    }

    pub fn wait_for_output(
        &mut self,
        needle: &[u8],
        deadline: Duration,
    ) -> Result<Vec<u8>, String> {
        let end = Instant::now() + deadline;
        while Instant::now() < end {
            if let Some(error) = &self.reader_error {
                return Err(error.clone());
            }
            if needle.is_empty() || self.output.windows(needle.len()).any(|w| w == needle) {
                return Ok(self.output.clone());
            }
            if self.reader_closed {
                break;
            }
            self.receive_until(end)?;
        }
        Err(self.reader_error.clone().unwrap_or_else(|| {
            format!(
                "timed out waiting for output {:?}; received {} bytes: {:?}",
                needle,
                self.output.len(),
                String::from_utf8_lossy(&self.output)
            )
        }))
    }

    pub fn wait_for_child_exit_and_drain(&mut self, deadline: Duration) -> Result<Vec<u8>, String> {
        let end = Instant::now() + deadline;
        loop {
            if Instant::now() >= end {
                return Err("child exit deadline exceeded".into());
            }
            if self
                .child
                .as_mut()
                .ok_or("child unavailable")?
                .try_wait()
                .map_err(|e| e.to_string())?
                .is_some()
            {
                self.close_pty_handles();
                self.drain_reader_until_finished(end)?;
                if let Some(error) = &self.reader_error {
                    return Err(error.clone());
                }
                return Ok(self.output.clone());
            }
            self.receive_until((Instant::now() + Duration::from_millis(10)).min(end))?;
        }
    }

    fn receive_until(&mut self, end: Instant) -> Result<(), String> {
        match self
            .rx
            .recv_timeout(end.saturating_duration_since(Instant::now()))
        {
            Ok(message) => self.accept(message),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) if self.reader_closed => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err("PTY output reader channel closed without EOF".into())
            }
        }
        if let Some(error) = &self.reader_error {
            return Err(error.clone());
        }
        Ok(())
    }

    fn drain_reader_until_finished(&mut self, deadline: Instant) -> Result<(), String> {
        loop {
            while let Ok(message) = self.rx.try_recv() {
                self.accept(message);
            }
            if self.reader.as_ref().is_some_and(JoinHandle::is_finished) && self.reader_closed {
                while let Ok(message) = self.rx.try_recv() {
                    self.accept(message);
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("PTY output reader drain deadline exceeded".into());
            }
            match self.rx.recv_timeout(
                Duration::from_millis(5).min(deadline.saturating_duration_since(Instant::now())),
            ) {
                Ok(message) => self.accept(message),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) if self.reader_closed => return Ok(()),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("PTY output reader channel closed".into())
                }
            }
        }
    }

    fn accept(&mut self, message: ReaderMessage) {
        match message {
            ReaderMessage::Bytes(bytes) => self.output.extend(bytes),
            ReaderMessage::Failed(error) => self.reader_error = Some(error),
            ReaderMessage::Closed => self.reader_closed = true,
        }
    }

    fn close_pty_handles(&mut self) {
        self.writer.take();
        self.slave.take();
        self.master.take();
    }

    fn terminate_and_reap_child(
        &mut self,
        deadline: Instant,
        cleanup: &mut Vec<String>,
        hard_abort: bool,
    ) -> bool {
        let Some(child) = self.child.as_mut() else {
            return true;
        };
        let allowed = self
            .config
            .reap_poll_gate
            .as_ref()
            .is_none_or(|gate| gate.load(Ordering::Acquire));
        match if allowed { child.try_wait() } else { Ok(None) } {
            Ok(Some(_)) => {
                self.child.take();
                return true;
            }
            Ok(None) => {}
            Err(error) => {
                cleanup.push(format!("child reap poll failed: {error}"));
                return false;
            }
        }

        // Normal shutdown preserves the input-close/HUP handler contract when
        // spare budget exists. Always reserve 20ms for escalation/reaping; short
        // deadlines and primary-error aborts get an immediate hard signal.
        // Both signal paths are nonblocking and share the original absolute deadline.
        let grace = if hard_abort {
            Duration::ZERO
        } else {
            deadline
                .saturating_duration_since(Instant::now())
                .saturating_sub(Duration::from_millis(20))
                .min(Duration::from_millis(100))
        };
        let force_at = Instant::now() + grace;
        let mut force_sent = grace.is_zero();
        if force_sent {
            if let Err(error) = hard_terminate(child.as_mut()) {
                cleanup.push(error);
            }
        } else {
            let mut signaler = child.clone_killer();
            if let Err(error) = signaler.kill() {
                cleanup.push(format!("child termination signal failed: {error}"));
            }
        }

        loop {
            let allowed = self
                .config
                .reap_poll_gate
                .as_ref()
                .is_none_or(|gate| gate.load(Ordering::Acquire));
            let status = self
                .child
                .as_mut()
                .map(|child| if allowed { child.try_wait() } else { Ok(None) });
            match status {
                Some(Ok(Some(_))) => {
                    self.child.take();
                    return true;
                }
                Some(Ok(None)) => {}
                Some(Err(error)) => {
                    cleanup.push(format!("child reap poll failed: {error}"));
                    return false;
                }
                None => return true,
            }

            if !force_sent && Instant::now() >= force_at {
                if let Some(child) = self.child.as_mut() {
                    if let Err(error) = hard_terminate(child.as_mut()) {
                        cleanup.push(error);
                    }
                }
                force_sent = true;
            }
            if Instant::now() >= deadline {
                cleanup.push("child cleanup deadline exceeded".into());
                return false;
            }
            match self.rx.recv_timeout(
                Duration::from_millis(1).min(deadline.saturating_duration_since(Instant::now())),
            ) {
                Ok(message) => self.accept(message),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
            }
        }
    }

    pub fn shutdown(&mut self) -> Result<ShutdownReport, LifecycleError> {
        self.shutdown_inner(None)
    }

    pub fn shutdown_with_primary_error(
        &mut self,
        primary: impl Into<String>,
    ) -> Result<ShutdownReport, LifecycleError> {
        self.shutdown_inner(Some(primary.into()))
    }

    fn shutdown_inner(
        &mut self,
        primary: Option<String>,
    ) -> Result<ShutdownReport, LifecycleError> {
        self.shutdown_attempted = true;
        let child_deadline = Instant::now() + self.config.cleanup_deadline;
        let mut events = Vec::new();
        let mut cleanup = Vec::new();
        // S12 phase 1: release all input and slave endpoints before polling or signaling.
        self.writer.take();
        self.slave.take();
        events.push(CleanupEvent::InputClosed);
        let child_reaped =
            self.terminate_and_reap_child(child_deadline, &mut cleanup, primary.is_some());
        if child_reaped {
            events.push(CleanupEvent::ChildReaped);
        }

        // Keep pumping output during child termination, then close the PTY
        // endpoints to unblock the platform reader.
        self.master.take();
        events.push(CleanupEvent::HandlesClosed);

        let reader_joined = if self.reader.is_some() {
            let end = child_deadline.max(Instant::now()) + self.config.reader_join_deadline;
            if self.drain_reader_until_finished(end).is_ok() {
                if let Some(reader) = self.reader.take() {
                    match reader.join() {
                        Ok(()) => true,
                        Err(_) => {
                            cleanup.push("PTY output reader panicked".into());
                            false
                        }
                    }
                } else {
                    false
                }
            } else {
                cleanup.push("PTY output reader cleanup deadline exceeded".into());
                false
            }
        } else {
            true
        };
        if reader_joined {
            events.push(CleanupEvent::ReaderJoined);
        }
        if let Some(error) = self.reader_error.take() {
            cleanup.push(error);
        }

        let handles_closed_before_fixture_removed = child_reaped
            && reader_joined
            && self.master.is_none()
            && self.slave.is_none()
            && self.writer.is_none();
        if handles_closed_before_fixture_removed {
            events.push(CleanupEvent::FixtureRemovalAttempted);
            self.fixture_removal_attempted = true;
            if self.fixture_remove_failure {
                cleanup.push("fixture removal failed (injected)".into());
                if let Some(capture) = &self.config.fixture_retention_capture {
                    *capture.lock().unwrap_or_else(|poison| poison.into_inner()) =
                        Some(self.fixture_path.clone());
                }
                if let Some(fixture) = self.fixture.take() {
                    let _retained_path = fixture.keep();
                }
            } else if self.fixture.is_some() {
                match std::fs::remove_dir_all(&self.fixture_path) {
                    Ok(()) => {
                        self.fixture.take();
                    }
                    Err(error) => cleanup.push(format!("fixture removal failed: {error}")),
                }
            }
        } else {
            cleanup
                .push("fixture retained because child or reader cleanup did not complete".into());
        }

        if self.fixture.is_none() && self.child.is_none() && self.reader.is_none() {
            self.cleanup_permit.take();
        }
        if cleanup.is_empty() && primary.is_none() {
            Ok(ShutdownReport {
                child_reaped,
                reader_joined,
                handles_closed_before_fixture_removed,
                events,
            })
        } else {
            Err(LifecycleError {
                primary,
                cleanup: cleanup.join("; "),
                events,
                child_reaped,
            })
        }
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        if self.fixture.is_some() || self.child.is_some() || self.reader.is_some() {
            let cleanup_succeeded = !self.shutdown_attempted && self.shutdown().is_ok();
            if !cleanup_succeeded
                || self.child.is_some()
                || self.reader.is_some()
                || self.fixture.is_some()
            {
                if let Some(permit) = self.cleanup_permit.take() {
                    let queue = permit.queue.clone();
                    queue
                        .pending
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .push(RetainedSession {
                            fixture: self.fixture.take(),
                            child: self.child.take(),
                            reader: self.reader.take(),
                            rx: std::mem::replace(&mut self.rx, mpsc::channel().1),
                            gate: self.config.reap_poll_gate.clone(),
                            last_error: None,
                            _permit: permit,
                        });
                    queue.wake.notify_one();
                }
            }
        }
    }
}

fn hard_terminate(child: &mut (dyn Child + Send + Sync)) -> Result<(), String> {
    #[cfg(unix)]
    {
        let pid = child
            .process_id()
            .ok_or("child process id unavailable for bounded hard abort")?;
        // SAFETY: this PID belongs to the owned direct Child handle; no process-group/global signal.
        if unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(format!("hard termination failed: {error}"));
            }
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        child
            .clone_killer()
            .kill()
            .map_err(|e| format!("hard termination failed: {e}"))
    }
}

const MAX_CLEANUP_OWNERS: usize = 64;
struct CleanupQueue {
    pending: Mutex<Vec<RetainedSession>>,
    wake: Condvar,
    owners: AtomicUsize,
}
struct CleanupPermit {
    queue: Arc<CleanupQueue>,
}
impl CleanupPermit {
    fn acquire() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        static MANAGER: OnceLock<Mutex<Option<Arc<CleanupQueue>>>> = OnceLock::new();
        let mut manager = MANAGER
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if manager.is_none() {
            let queue = Arc::new(CleanupQueue {
                pending: Mutex::new(Vec::with_capacity(MAX_CLEANUP_OWNERS)),
                wake: Condvar::new(),
                owners: AtomicUsize::new(0),
            });
            let worker = queue.clone();
            // Start before spawning any child: thread-creation failure cannot abandon live resources.
            std::thread::Builder::new()
                .name("ira-parity-cleanup".into())
                .spawn(move || worker.run())?;
            *manager = Some(queue);
        }
        let queue = manager
            .as_ref()
            .ok_or("cleanup manager unavailable")?
            .clone();
        let mut owners = queue.owners.load(Ordering::Acquire);
        loop {
            if owners >= MAX_CLEANUP_OWNERS {
                return Err("cleanup capacity exhausted; retained sessions must finish before spawning more".into());
            }
            match queue.owners.compare_exchange_weak(
                owners,
                owners + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => owners = actual,
            }
        }
        Ok(Self { queue })
    }
}
impl Drop for CleanupPermit {
    fn drop(&mut self) {
        self.queue.owners.fetch_sub(1, Ordering::AcqRel);
    }
}
impl CleanupQueue {
    fn run(&self) {
        let mut active = Vec::with_capacity(MAX_CLEANUP_OWNERS);
        loop {
            {
                let mut inbox = self.pending.lock().unwrap_or_else(|p| p.into_inner());
                while inbox.is_empty() && active.is_empty() {
                    inbox = self.wake.wait(inbox).unwrap_or_else(|p| p.into_inner());
                }
                active.extend(inbox.drain(..));
            } // Never hold the enqueue lock across child polling, joining or filesystem work.
            let mut i = 0;
            while i < active.len() {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| active[i].poll()));
                match result {
                    Ok(true) => {
                        active.swap_remove(i);
                    }
                    Ok(false) => i += 1,
                    Err(_) => {
                        active[i].record_error(
                            "deferred cleanup poll panicked; ownership retained".into(),
                        );
                        i += 1
                    }
                }
            }
            let inbox = self.pending.lock().unwrap_or_else(|p| p.into_inner());
            if inbox.is_empty() && !active.is_empty() {
                drop(
                    self.wake
                        .wait_timeout(inbox, Duration::from_millis(50))
                        .unwrap_or_else(|p| p.into_inner()),
                );
            }
        }
    }
}

struct RetainedSession {
    fixture: Option<tempfile::TempDir>,
    child: Option<Box<dyn Child + Send + Sync>>,
    reader: Option<JoinHandle<()>>,
    rx: Receiver<ReaderMessage>,
    gate: Option<Arc<AtomicBool>>,
    last_error: Option<String>,
    _permit: CleanupPermit,
}
impl RetainedSession {
    fn record_error(&mut self, error: String) {
        if self.last_error.as_ref() != Some(&error) {
            eprintln!("ira-parity deferred cleanup: {error}");
            self.last_error = Some(error);
        }
    }
    fn poll(&mut self) -> bool {
        // Drain queued output so an abandoned session never accumulates bytes after Drop.
        for _ in 0..4096 {
            if self.rx.try_recv().is_err() {
                break;
            }
        }
        if self
            .gate
            .as_ref()
            .is_some_and(|g| !g.load(Ordering::Acquire))
        {
            return false;
        }
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.child.take();
                }
                Ok(None) => {
                    if let Err(e) = hard_terminate(child.as_mut()) {
                        self.record_error(e);
                    }
                    return false;
                }
                Err(e) => {
                    self.record_error(format!("deferred child reap failed: {e}"));
                    return false;
                }
            }
        }
        if let Some(reader) = self.reader.as_ref() {
            if !reader.is_finished() {
                return false;
            }
            if self.reader.take().is_some_and(|r| r.join().is_err()) {
                self.record_error("deferred output reader panicked".into());
            }
        }
        if let Some(fixture) = self.fixture.as_ref() {
            match std::fs::remove_dir_all(fixture.path()) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    self.record_error(format!("deferred fixture removal failed: {e}"));
                    return false;
                }
            }
            self.fixture.take();
        }
        true
    }
}
