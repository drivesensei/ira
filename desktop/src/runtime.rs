//! One background owner; foreground reads never wait for the actor.
use ira_core::{
    application::{App, PersistenceFailure, WorkSettlement},
    editor::{EditorDocument, EditorError, EditorSession, SaveCompletion},
    input::{Input, KeyCode, KeyEvent, KeyModifiers},
    model::{EntryTarget, HostRequest, OpenEditorRequest},
    observable::Snapshot,
    services::transfer::JobControl,
    theme::{Loader, Theme, ThemeCapabilities},
};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

// One finite unit is admitted by an RMW on the same irreversible stop latch.
// This does not authenticate settlement of asynchronous work started by that unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActorUnit {
    Attachment,
    Completion,
    Command,
    Tick,
    ChooserDrain,
    Host,
    Publication,
}
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActorPhase {
    BeforeAdmission,
    Admitted,
    Returned,
    Canceled,
}
#[cfg(test)]
type ActorHook = Arc<dyn Fn(ActorUnit, ActorPhase, &App) + Send + Sync>;
#[cfg(test)]
thread_local! { static ACTOR_HOOK: std::cell::RefCell<Option<ActorHook>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
type EditorHook = Arc<dyn Fn() + Send + Sync>;
#[cfg(test)]
thread_local! { static EDITOR_HOOK: std::cell::RefCell<Option<EditorHook>> = const { std::cell::RefCell::new(None) }; }
struct ActorAdmission {
    stop: Arc<AtomicBool>,
    #[cfg(test)]
    hook: Option<ActorHook>,
}
impl ActorAdmission {
    fn admit(&self, app: &mut App, unit: ActorUnit) -> bool {
        #[cfg(not(test))]
        let _ = unit;
        #[cfg(test)]
        if let Some(hook) = &self.hook {
            hook(unit, ActorPhase::BeforeAdmission, app);
        }
        if !app.running {
            self.stop.store(true, Ordering::Release);
        }
        // Strong no-op CAS cannot reset true and establishes one-unit admission.
        let admitted = self
            .stop
            .compare_exchange(false, false, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        #[cfg(test)]
        if admitted {
            if let Some(hook) = &self.hook {
                hook(unit, ActorPhase::Admitted, app);
            }
        }
        if !admitted {
            self.finish(app, unit);
        }
        admitted
    }
    fn finish(&self, app: &mut App, unit: ActorUnit) -> bool {
        #[cfg(not(test))]
        let _ = unit;
        #[cfg(test)]
        if let Some(hook) = &self.hook {
            hook(unit, ActorPhase::Returned, app);
        }
        if !app.running {
            self.stop.store(true, Ordering::Release);
        }
        if self.stop.load(Ordering::Acquire) {
            app.cancel_pending_work();
            cancel_all(app); // Includes controls created by this unit, never published.
            #[cfg(test)]
            if let Some(hook) = &self.hook {
                hook(unit, ActorPhase::Canceled, app);
            }
            false
        } else {
            true
        }
    }
}

#[derive(Clone, Debug)]
pub enum Command {
    Browse(crate::platform::chooser::ChooserKind),
    ChooserResult {
        ticket: crate::platform::chooser::ChooserTicket,
        outcome: Arc<crate::platform::chooser::ChooserOutcome>,
    },
    Input(Input),
    Accessibility(crate::platform::accessibility::ResolvedAction),
    FocusPane(usize),
    Wheel {
        pane: usize,
        listing_generation: u64,
        next: bool,
    },
    Job {
        id: u64,
        verb: JobVerb,
    },
    Place {
        kind: PlaceKind,
        path: String,
    },
    PlaceExact {
        kind: PlaceKind,
        path: String,
        shortcut: char,
        occurrence: usize,
    },
    Target {
        target: EntryTarget,
        verb: TargetVerb,
    },
    Draft {
        text: String,
        cursor: usize,
    },
    EditorDraft {
        document_id: u64,
        revision: u64,
        text: String,
    },
    SaveDraft {
        document_id: u64,
        revision: u64,
        text: String,
    },
    ClipboardResult(bool),
    HostResult(Result<(), String>),
    SetFocus(u64),
}
#[derive(Clone, Copy, Debug)]
pub enum JobVerb {
    Focus,
    Pause,
    Cancel,
}
#[derive(Clone, Copy, Debug)]
pub enum PlaceKind {
    Drive,
    Common,
    Bookmark,
}
#[derive(Clone, Copy, Debug)]
pub enum TargetVerb {
    Focus,
    Toggle,
    Selected(bool),
    SelectOnly,
    Open,
}
#[derive(Clone, Debug)]
pub struct Envelope {
    pub sequence: u64,
    pub window_generation: u64,
    /// Only document-specific callbacks bind these. Ordinary keys resolve in FIFO actor state.
    pub input_generation: Option<(u64, u64)>,
    pub command: Command,
}
pub struct Publication {
    pub snapshot: Arc<Snapshot>,
    pub cancellation: Vec<Arc<JobControl>>,
    pub theme: Theme,
    /// Worker-computed light counterpart; None preserves configured preset behavior.
    pub light_theme: Option<Theme>,
    pub font_family: Option<String>,
}
pub enum Completion {
    Rejected {
        sequence: u64,
        window_generation: u64,
        reason: String,
    },
    Host {
        sequence: u64,
        window_generation: u64,
        input_generation: Option<(u64, u64)>,
        request: HostRequest,
    },
    Closed,
}
/// Latest snapshots can be coalesced. Semantic completions use a separate FIFO.
pub struct Latest<T>(Mutex<Option<T>>);
impl<T> Default for Latest<T> {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}
impl<T> Latest<T> {
    pub fn publish(&self, value: T) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(value);
        }
    }
    pub fn try_take(&self) -> Option<T> {
        self.0.try_lock().ok()?.take()
    }
}
#[derive(Clone, Debug)]
pub enum ShutdownState {
    Running,
    Pending,
    Success { epoch: u64 },
    Failure(Arc<PersistenceFailure>),
    Disconnected,
    WorkError(String),
}
pub struct Runtime {
    sender: SyncSender<Envelope>,
    latest: Arc<Latest<Publication>>,
    completions: Arc<Mutex<Receiver<Completion>>>,
    attached_window: Arc<AtomicU64>,
    stopping: Arc<AtomicBool>,
    shutdown_complete: Arc<AtomicBool>,
    shutdown: Arc<Mutex<ShutdownState>>,
    retry_shutdown: SyncSender<()>,
    chooser_events: Arc<Mutex<Receiver<chooser_runtime::ChooserEvent>>>,
    chooser_permit: Arc<Mutex<Option<AdmittedChooser>>>,
    chooser_focus: Arc<Mutex<Option<ChooserFocusPermit>>>,
    pub window_generation: u64,
    next_sequence: Arc<AtomicU64>,
    pending: VecDeque<Envelope>,
}
impl Runtime {
    pub fn start(window_generation: u64) -> Self {
        Self::with_factory(window_generation, App::new)
    }
    pub fn start_with_fonts(window_generation: u64, text: Arc<gpui::TextSystem>) -> Self {
        Self::with_resources(
            window_generation,
            true,
            Box::new(crate::platform::drives),
            move || {
                let mut app = App::new();
                #[cfg(target_os = "windows")]
                app.set_transfer_provider(Arc::new(crate::platform::rename_no_replace));
                let font_family = text.all_font_names().into_iter().find(|family| {
                    let id = text.resolve_font(&gpui::font(family.clone()));
                    ['\u{f07b}', '\u{e718}', '\u{e0b6}']
                        .into_iter()
                        .all(|glyph| text.advance(id, gpui::px(14.), glyph).is_ok())
                });
                let caps = ThemeCapabilities {
                    truecolor: true,
                    nerd_font: font_family.is_some(),
                    wide_emoji: true,
                };
                let mut loader = Loader::from_env(caps);
                app.icons = loader.resolve_icons(std::env::var("IRA_ICONS").ok().as_deref());
                loader.set_icon_set(app.icons);
                // Seed TOML/default before drain_startup applies an optional persisted state key.
                app.theme_preset = loader.resolve_preset(None).0;
                (app, loader, font_family)
            },
        )
    }
    pub fn with_factory(
        window_generation: u64,
        factory: impl FnOnce() -> App + Send + 'static,
    ) -> Self {
        Self::with_resources(
            window_generation,
            false,
            Box::new(crate::platform::drives),
            move || {
                (
                    factory(),
                    Loader::from_toml("", ThemeCapabilities::default()),
                    None,
                )
            },
        )
    }
    pub fn with_drive_probe(
        window_generation: u64,
        factory: impl FnOnce() -> App + Send + 'static,
        probe: impl FnMut() -> std::io::Result<Vec<ira_core::domain::data::Folder>> + Send + 'static,
    ) -> Self {
        Self::with_resources(window_generation, true, Box::new(probe), move || {
            (
                factory(),
                Loader::from_toml("", ThemeCapabilities::default()),
                None,
            )
        })
    }
    fn with_resources(
        window_generation: u64,
        poll_drives: bool,
        mut drive_probe: Box<
            dyn FnMut() -> std::io::Result<Vec<ira_core::domain::data::Folder>> + Send,
        >,
        factory: impl FnOnce() -> (App, Loader, Option<String>) + Send + 'static,
    ) -> Self {
        let (sender, commands) = mpsc::sync_channel::<Envelope>(256);
        let (completed, completions) = mpsc::channel();
        let (chooser_tx, chooser_rx) = mpsc::channel();
        let chooser_permit = Arc::new(Mutex::new(None));
        let actor_chooser_permit = chooser_permit.clone();
        let chooser_focus = Arc::new(Mutex::new(None));
        let actor_chooser_focus = chooser_focus.clone();
        let latest = Arc::new(Latest::default());
        let stopping = Arc::new(AtomicBool::new(false));
        let shutdown_complete = Arc::new(AtomicBool::new(false));
        let actor_shutdown_complete = shutdown_complete.clone();
        let shutdown = Arc::new(Mutex::new(ShutdownState::Running));
        let actor_shutdown = shutdown.clone();
        let (retry_shutdown, retry_requests) = mpsc::sync_channel(1);
        #[cfg(test)]
        let fixture = Some(crate::test_support::current().expect(
            "unit-test Runtime requires test_support::enter before actor creation; default persistence paths are forbidden",
        ));
        #[cfg(test)]
        if let Some(fixture) = &fixture {
            fixture.register(stopping.clone(), shutdown_complete.clone());
        }
        let publication = latest.clone();
        let stop = stopping.clone();
        let attached_window = Arc::new(AtomicU64::new(window_generation));
        let attached = attached_window.clone();
        #[cfg(test)]
        let hook = ACTOR_HOOK.with(|slot| slot.borrow().clone());
        #[cfg(test)]
        let editor_hook = EDITOR_HOOK.with(|slot| slot.borrow().clone());
        thread::spawn(move || {
            let admission = ActorAdmission {
                stop: stop.clone(),
                #[cfg(test)]
                hook,
            };
            let (mut app, loader, font_family) = factory();
            #[cfg(test)]
            if let Some(fixture) = fixture {
                if app.state_path.is_none() {
                    app.state_path = Some(fixture.directory.join("state"));
                }
                if app.bookmarks_path.is_none() {
                    app.bookmarks_path = Some(fixture.directory.join("bookmarks"));
                }
            }
            if admission.admit(&mut app, ActorUnit::Attachment) {
                app.attach_window(window_generation);
                admission.finish(&mut app, ActorUnit::Attachment);
            }
            let (editor_sender, editor_rx) = mpsc::channel::<(u64, u64, HostRequest)>();
            let editor_settlement = Arc::new(EditorSettlement {
                #[cfg(test)]
                hook: editor_hook,
                ..EditorSettlement::default()
            });
            let editor_tx = EditorDispatch {
                sender: std::cell::RefCell::new(Some(editor_sender)),
                issued: std::cell::Cell::new(0),
                error: std::cell::RefCell::new(None),
                settlement: editor_settlement.clone(),
            };
            let (editor_done, editor_results) = mpsc::channel();
            let (drive_tx, drive_rx) = mpsc::channel::<u64>();
            let drive_stop = stop.clone();
            let drive_attached = attached.clone();
            let drive_done = editor_done.clone();
            thread::spawn(move || {
                let mut generation = 0u64;
                while !drive_stop.load(Ordering::Acquire) {
                    let epoch = match drive_rx.recv_timeout(Duration::from_secs(2)) {
                        Ok(epoch) => epoch,
                        Err(mpsc::RecvTimeoutError::Timeout) if poll_drives => {
                            drive_attached.load(Ordering::Acquire)
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    if epoch == 0
                        || drive_attached.load(Ordering::Acquire) != epoch
                        || drive_stop.load(Ordering::Acquire)
                    {
                        continue;
                    }
                    generation = generation.wrapping_add(1);
                    let result = drive_probe().map_err(|error| error.to_string());
                    if drive_done
                        .send(EditorResult::Drives(epoch, generation, result))
                        .is_err()
                    {
                        break;
                    }
                }
            });
            let mut drive_generation = 0;
            let editor_stop = stop.clone();
            let editor_attached = attached.clone();
            if let Err(error) = thread::Builder::new().spawn(move || {
                editor_worker(
                    editor_rx,
                    editor_done,
                    editor_stop,
                    editor_attached,
                    editor_settlement,
                );
            }) {
                *editor_tx.error.borrow_mut() =
                    Some(format!("Editor worker spawn failed: {error}"));
                editor_tx.sender.borrow_mut().take();
            }
            let mut chooser = chooser_runtime::ActorChooser::default();
            let (chooser_pending_tx, chooser_pending_rx) = mpsc::channel();
            let mut last_tick = Instant::now();
            if admission.admit(&mut app, ActorUnit::Publication) {
                *actor_chooser_permit.lock().unwrap() = chooser.permit(&app);
                *actor_chooser_focus.lock().unwrap() = app.existing_path_focus_stamp();
                for event in chooser_pending_rx.try_iter() {
                    if !admission.finish(&mut app, ActorUnit::Publication) {
                        break;
                    }
                    let _ = chooser_tx.send(event);
                }
                if admission.finish(&mut app, ActorUnit::Publication) {
                    publish(&app, &publication, &loader, &font_family);
                }
                admission.finish(&mut app, ActorUnit::Publication);
            }
            'actor: while !stop.load(Ordering::Acquire) && app.running {
                if !admission.admit(&mut app, ActorUnit::Attachment) {
                    break;
                }
                let changed = synchronize_attachment(
                    &mut app,
                    attached.load(Ordering::Acquire),
                    &editor_tx,
                    &drive_tx,
                    &completed,
                    &admission,
                );
                if !admission.finish(&mut app, ActorUnit::Attachment) {
                    break;
                }
                if changed {
                    if !admission.admit(&mut app, ActorUnit::Publication) {
                        break;
                    }
                    *actor_chooser_permit.lock().unwrap() = chooser.permit(&app);
                    *actor_chooser_focus.lock().unwrap() = app.existing_path_focus_stamp();
                    for event in chooser_pending_rx.try_iter() {
                        if !admission.finish(&mut app, ActorUnit::Publication) {
                            break;
                        }
                        let _ = chooser_tx.send(event);
                    }
                    if admission.finish(&mut app, ActorUnit::Publication) {
                        publish(&app, &publication, &loader, &font_family);
                    }
                    if !admission.finish(&mut app, ActorUnit::Publication) {
                        break;
                    }
                }
                for result in editor_results.try_iter() {
                    if !admission.admit(&mut app, ActorUnit::Completion) {
                        break 'actor;
                    }
                    match result {
                        EditorResult::Drives(epoch, generation, result)
                            if epoch == app.window_generation
                                && epoch == attached.load(Ordering::Acquire)
                                && generation > drive_generation =>
                        {
                            drive_generation = generation;
                            match result {
                                Ok(drives) if app.drives.as_ref() != Some(&drives) => {
                                    app.apply_drives(drives)
                                }
                                _ => {}
                            }
                        }
                        EditorResult::Opened(epoch, request, result)
                            if epoch == app.window_generation
                                && epoch == attached.load(Ordering::Acquire) =>
                        {
                            app.apply_open_editor(request, result);
                        }
                        EditorResult::Saved(epoch, id, revision, result)
                            if epoch == app.window_generation
                                && epoch == attached.load(Ordering::Acquire) =>
                        {
                            app.apply_save_result(id, revision, result);
                        }
                        _ => {}
                    }
                    if !admission.finish(&mut app, ActorUnit::Completion) {
                        break 'actor;
                    }
                    if !admission.admit(&mut app, ActorUnit::Publication) {
                        break 'actor;
                    }
                    *actor_chooser_permit.lock().unwrap() = chooser.permit(&app);
                    *actor_chooser_focus.lock().unwrap() = app.existing_path_focus_stamp();
                    for event in chooser_pending_rx.try_iter() {
                        if !admission.finish(&mut app, ActorUnit::Publication) {
                            break;
                        }
                        let _ = chooser_tx.send(event);
                    }
                    if admission.finish(&mut app, ActorUnit::Publication) {
                        publish(&app, &publication, &loader, &font_family);
                    }
                    if !admission.finish(&mut app, ActorUnit::Publication) {
                        break 'actor;
                    }
                }
                match commands.recv_timeout(Duration::from_millis(20)) {
                    Ok(envelope) => {
                        // Dequeue is not admission. Attachment and dispatch each cross the latch.
                        let sequence = envelope.sequence;
                        let window_generation = envelope.window_generation;
                        if !admission.admit(&mut app, ActorUnit::Attachment) {
                            let _ = completed.send(Completion::Rejected {
                                sequence,
                                window_generation,
                                reason: "Actor is stopping".into(),
                            });
                            break;
                        }
                        synchronize_attachment(
                            &mut app,
                            attached.load(Ordering::Acquire),
                            &editor_tx,
                            &drive_tx,
                            &completed,
                            &admission,
                        );
                        if !admission.finish(&mut app, ActorUnit::Attachment) {
                            break;
                        }
                        if !admission.admit(&mut app, ActorUnit::Command) {
                            let _ = completed.send(Completion::Rejected {
                                sequence,
                                window_generation,
                                reason: "Actor is stopping".into(),
                            });
                            break;
                        }
                        if let Err(reason) = chooser.apply(&mut app, envelope, &chooser_pending_tx)
                        {
                            let _ = completed.send(Completion::Rejected {
                                sequence,
                                window_generation,
                                reason,
                            });
                        }
                        if !admission.finish(&mut app, ActorUnit::Command) {
                            break;
                        }
                        for request in app.take_host_requests() {
                            if !admission.admit(&mut app, ActorUnit::Host) {
                                break 'actor;
                            }
                            route_host(&app, request, sequence, &editor_tx, &drive_tx, &completed);
                            if !admission.finish(&mut app, ActorUnit::Host) {
                                break 'actor;
                            }
                        }
                        if !admission.admit(&mut app, ActorUnit::Publication) {
                            break;
                        }
                        *actor_chooser_permit.lock().unwrap() = chooser.permit(&app);
                        *actor_chooser_focus.lock().unwrap() = app.existing_path_focus_stamp();
                        for event in chooser_pending_rx.try_iter() {
                            if !admission.finish(&mut app, ActorUnit::Publication) {
                                break;
                            }
                            let _ = chooser_tx.send(event);
                        }
                        if admission.finish(&mut app, ActorUnit::Publication) {
                            publish(&app, &publication, &loader, &font_family);
                        }
                        if !admission.finish(&mut app, ActorUnit::Publication) {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                if last_tick.elapsed() >= Duration::from_millis(500) {
                    if !admission.admit(&mut app, ActorUnit::Tick) {
                        break;
                    }
                    let context = app.input_context();
                    let focus = app.focus_generation;
                    app.tick();
                    if context != app.input_context() && focus == app.focus_generation {
                        app.focus_generation = app.focus_generation.wrapping_add(1);
                    }
                    if !admission.finish(&mut app, ActorUnit::Tick) {
                        break;
                    }
                    if !admission.admit(&mut app, ActorUnit::ChooserDrain) {
                        break;
                    }
                    chooser.drain(&mut app, &chooser_pending_tx);
                    if !admission.finish(&mut app, ActorUnit::ChooserDrain) {
                        break;
                    }
                    for request in app.take_host_requests() {
                        if !admission.admit(&mut app, ActorUnit::Host) {
                            break 'actor;
                        }
                        route_host(
                            &app,
                            request,
                            app.ack_sequence,
                            &editor_tx,
                            &drive_tx,
                            &completed,
                        );
                        if !admission.finish(&mut app, ActorUnit::Host) {
                            break 'actor;
                        }
                    }
                    if !admission.admit(&mut app, ActorUnit::Publication) {
                        break;
                    }
                    *actor_chooser_permit.lock().unwrap() = chooser.permit(&app);
                    *actor_chooser_focus.lock().unwrap() = app.existing_path_focus_stamp();
                    for event in chooser_pending_rx.try_iter() {
                        if !admission.finish(&mut app, ActorUnit::Publication) {
                            break;
                        }
                        let _ = chooser_tx.send(event);
                    }
                    if admission.finish(&mut app, ActorUnit::Publication) {
                        publish(&app, &publication, &loader, &font_family);
                    }
                    if !admission.finish(&mut app, ActorUnit::Publication) {
                        break;
                    }
                    last_tick = Instant::now();
                }
            }
            crate::lifecycle_trace(if app.running {
                "actor terminating: stop flag or command channel disconnected"
            } else {
                "actor terminating: core running false"
            });
            stop.store(true, Ordering::Release);
            let core_seal = app.begin_shutdown_settlement();
            cancel_all(&app);
            let editor_cutoff = editor_tx.seal();
            *actor_shutdown.lock().unwrap() = ShutdownState::Pending;
            // Original startup, active jobs and both serial lanes are independent.
            // Poll all of them fairly; never tick, route hosts or apply stale results.
            loop {
                let core = app.poll_shutdown_settlement(&core_seal, 64);
                let editor = editor_tx.poll(editor_cutoff);
                let error = match (&core, &editor) {
                    (WorkSettlement::Error(error), _) => Some(error.clone()),
                    (_, Err(error)) => Some(error.clone()),
                    _ => None,
                };
                if let Some(error) = error {
                    *actor_shutdown.lock().unwrap() = ShutdownState::WorkError(error);
                } else if matches!(&core, WorkSettlement::Settled(receipt) if core_seal.accepts(receipt))
                    && matches!(editor, Ok(true))
                {
                    break;
                } else {
                    *actor_shutdown.lock().unwrap() = ShutdownState::Pending;
                }
                // Discard only bounded ordinary-result records; these confer no
                // settlement authority and cannot resurrect business state/effects.
                for _ in 0..64 {
                    if editor_results.try_recv().is_err() {
                        break;
                    }
                }
                // A retry during work wait cannot manufacture an acknowledgement.
                let _ = retry_requests.try_recv();
                thread::sleep(Duration::from_millis(20));
            }
            app.persist_state();
            // Keep this ORIGINAL receipt and owner after timeout; only a checked
            // write success acknowledges the final accepted snapshot.
            let mut receipt = app.checked_persistence_barrier();
            *actor_shutdown.lock().unwrap() = ShutdownState::Pending;
            let mut failed = None;
            loop {
                match receipt.recv_timeout(Duration::from_millis(20)) {
                    Ok(Ok(success)) => {
                        *actor_shutdown.lock().unwrap() = ShutdownState::Success {
                            epoch: success.epoch,
                        };
                        actor_shutdown_complete.store(true, Ordering::Release);
                        let _ = completed.send(Completion::Closed);
                        break;
                    }
                    Ok(Err(error)) => {
                        let error = Arc::new(error);
                        *actor_shutdown.lock().unwrap() = ShutdownState::Failure(error.clone());
                        failed = Some(error);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        if failed.is_none() {
                            *actor_shutdown.lock().unwrap() = ShutdownState::Disconnected;
                        }
                        // No automatic success, state recapture, or business replay.
                        thread::sleep(Duration::from_millis(20));
                    }
                }
                if retry_requests.try_recv().is_ok()
                    && let Some(error) = failed.take()
                {
                    receipt = error.retry.retry();
                    *actor_shutdown.lock().unwrap() = ShutdownState::Pending;
                }
            }
        });
        Self {
            sender,
            latest,
            completions: Arc::new(Mutex::new(completions)),
            attached_window,
            stopping,
            shutdown_complete,
            shutdown,
            retry_shutdown,
            chooser_events: Arc::new(Mutex::new(chooser_rx)),
            chooser_permit,
            chooser_focus,
            window_generation,
            next_sequence: Arc::new(AtomicU64::new(1)),
            pending: VecDeque::new(),
        }
    }
    pub fn enqueue(&mut self, command: Command, input_generation: Option<(u64, u64)>) -> u64 {
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
        if self.is_stopping() {
            return sequence;
        }
        self.pending.push_back(Envelope {
            sequence,
            window_generation: self.window_generation,
            input_generation,
            command,
        });
        self.flush();
        sequence
    }
    pub fn flush(&mut self) {
        if self.is_stopping() {
            self.pending.clear();
            return;
        }
        while let Some(envelope) = self.pending.pop_front() {
            match self.sender.try_send(envelope) {
                Ok(()) => {}
                Err(TrySendError::Full(envelope)) => {
                    self.pending.push_front(envelope);
                    break;
                }
                Err(TrySendError::Disconnected(envelope)) => {
                    self.pending.push_front(envelope);
                    break;
                }
            }
        }
    }
    /// Reopen attaches a new UI epoch to the same actor; there is no second persistence writer.
    pub fn attach(&self, window_generation: u64) -> Self {
        self.attached_window
            .store(window_generation, Ordering::Release);
        Self {
            sender: self.sender.clone(),
            latest: self.latest.clone(),
            completions: self.completions.clone(),
            attached_window: self.attached_window.clone(),
            stopping: self.stopping.clone(),
            shutdown_complete: self.shutdown_complete.clone(),
            shutdown: self.shutdown.clone(),
            retry_shutdown: self.retry_shutdown.clone(),
            chooser_events: self.chooser_events.clone(),
            chooser_permit: self.chooser_permit.clone(),
            chooser_focus: self.chooser_focus.clone(),
            window_generation,
            next_sequence: self.next_sequence.clone(),
            pending: VecDeque::new(),
        }
    }
    pub fn detach(&self) {
        let _ = self.attached_window.compare_exchange(
            self.window_generation,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
    pub fn cancel_jobs(&self, controls: &[Arc<JobControl>]) {
        for control in controls {
            control.request_cancel();
        }
    }
    pub fn backlog(&self) -> usize {
        self.pending.len()
    }
    pub fn try_snapshot(&self) -> Option<Publication> {
        if self.attached_window.load(Ordering::Acquire) != self.window_generation {
            return None;
        }
        self.latest.try_take()
    }
    pub fn effect_guard(&self, window_generation: u64) -> EffectGuard {
        EffectGuard {
            attached: self.attached_window.clone(),
            stopping: self.stopping.clone(),
            window_generation,
        }
    }
    pub fn try_completion(&self) -> Option<Completion> {
        if self.attached_window.load(Ordering::Acquire) != self.window_generation {
            return None;
        }
        let completion = self.completions.try_lock().ok()?.try_recv().ok()?;
        if let Completion::Host {
            sequence,
            window_generation,
            ..
        } = &completion
            && (*window_generation != self.window_generation
                || !self.effect_guard(*window_generation).is_current())
        {
            return Some(Completion::Rejected {
                sequence: *sequence,
                window_generation: *window_generation,
                reason: "Native request belongs to a replaced window".into(),
            });
        }
        Some(completion)
    }
    pub fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
    }
    pub fn shutdown_state(&self) -> Option<ShutdownState> {
        self.shutdown.try_lock().ok().map(|state| state.clone())
    }
    pub fn retry_shutdown(&self) -> bool {
        matches!(self.shutdown_state(), Some(ShutdownState::Failure(_)))
            && self.retry_shutdown.try_send(()).is_ok()
    }
    /// Cheap acknowledgment after actor cancellation and a successful checked persistence receipt.
    /// Clones share it across window epochs; reading it never consumes UI completions.
    pub fn shutdown_complete(&self) -> bool {
        self.shutdown_complete.load(Ordering::Acquire)
    }
    /// Does not enqueue business work, wait on App/session locks, do I/O or join workers.
    /// Cancelling known controls briefly synchronizes each pause predicate; it may contend.
    pub fn stop(&self, controls: &[Arc<JobControl>]) {
        self.stopping.store(true, Ordering::Release);
        for control in controls {
            control.request_cancel();
        }
    }
}

fn cancel_all(app: &App) {
    for job in &app.jobs {
        job.control.request_cancel();
    }
    if let Some(deletion) = &app.deletion {
        deletion.control.request_cancel();
    }
}
fn publish(app: &App, latest: &Latest<Publication>, loader: &Loader, font_family: &Option<String>) {
    // Build the owned projection without holding the mailbox lock.
    let snapshot = Arc::new(app.snapshot());
    let mut cancellation: Vec<_> = app.jobs.iter().map(|job| job.control.clone()).collect();
    if let Some(deletion) = &app.deletion {
        cancellation.push(deletion.control.clone());
    }
    latest.publish(Publication {
        snapshot,
        cancellation,
        theme: loader.theme_for(app.theme_preset),
        light_theme: loader.desktop_light_theme_for(app.theme_preset),
        font_family: font_family.clone(),
    });
}
pub fn apply(app: &mut App, envelope: Envelope) -> Result<(), String> {
    if envelope.sequence <= app.ack_sequence {
        return Err("Command sequence was already acknowledged".into());
    }
    if envelope.window_generation != app.window_generation {
        return Err("Window was replaced".into());
    }
    if let Some((document, focus)) = envelope.input_generation
        && (document != app.document_generation || focus != app.focus_generation)
    {
        return Err("Input belongs to a closed document or focus".into());
    }
    let previous_context = app.input_context();
    let previous_focus = app.focus_generation;
    let command = match envelope.command {
        Command::Accessibility(action) => {
            if (
                action.stamp.window,
                action.stamp.document,
                action.stamp.focus,
                action.stamp.revision,
            ) != (
                app.window_generation,
                app.document_generation,
                app.focus_generation,
                app.revision,
            ) {
                return Err("Accessibility action belongs to an older semantic frame".into());
            }
            crate::views::accessibility::actor_command(action)?
        }
        command => command,
    };
    match command {
        Command::Browse(_) | Command::ChooserResult { .. } => {
            return Err("Chooser command requires actor admission".into());
        }
        Command::Accessibility(_) => unreachable!("resolved above"),
        Command::FocusPane(pane) => {
            if pane >= 2
                || (pane == 1 && !app.split)
                || !matches!(
                    app.input_context(),
                    ira_core::input::InputContext::Pane(_) | ira_core::input::InputContext::Search
                )
            {
                return Err("Pane is unavailable or modal input owns focus".into());
            }
            app.active_pane = pane;
        }
        Command::Input(input) => app.dispatch(input).map_err(|error| error.to_string())?,
        Command::Wheel {
            pane,
            listing_generation,
            next,
        } => {
            if !matches!(
                app.input_context(),
                ira_core::input::InputContext::Pane(_) | ira_core::input::InputContext::Search
            ) {
                return Err("An input or dialog owns focus".into());
            }
            if pane >= 2
                || (pane == 1 && !app.split)
                || app.panes[pane].listing_generation != listing_generation
            {
                return Err("Listing changed during wheel input".into());
            }
            app.active_pane = pane;
            app.dispatch(Input::Key(KeyEvent::new(
                if next { KeyCode::Down } else { KeyCode::Up },
                KeyModifiers::NONE,
            )))
            .map_err(|error| error.to_string())?;
        }
        Command::Job { id, verb } => {
            if !matches!(
                app.input_context(),
                ira_core::input::InputContext::Pane(_) | ira_core::input::InputContext::Board
            ) || !app.copy_board
            {
                return Err("An input or dialog owns focus".into());
            }
            let index = app
                .jobs
                .iter()
                .position(|job| job.id == id)
                .ok_or_else(|| "Job is no longer available".to_string())?;
            app.board_focused = true;
            app.copy_board_state.select(Some(index));
            match verb {
                JobVerb::Focus => {}
                JobVerb::Pause => app.toggle_selected_job_pause(),
                JobVerb::Cancel => app.cancel_selected_job(),
            }
        }
        Command::PlaceExact {
            kind,
            path,
            shortcut,
            occurrence,
        } => {
            if !matches!(
                app.input_context(),
                ira_core::input::InputContext::Pane(_) | ira_core::input::InputContext::Search
            ) {
                return Err("An input or dialog owns focus".into());
            }
            let folders = match kind {
                PlaceKind::Drive => &app.drives,
                PlaceKind::Common => &app.folders,
                PlaceKind::Bookmark => &app.bookmarks,
            };
            let index = folders
                .as_ref()
                .and_then(|folders| {
                    folders
                        .iter()
                        .enumerate()
                        .filter(|(_, folder)| folder.path == path && folder.shortcut == shortcut)
                        .nth(occurrence)
                        .map(|(index, _)| index)
                })
                .ok_or_else(|| "Place is no longer available".to_string())?;
            app.apply_command(match kind {
                PlaceKind::Drive => ira_core::input::Command::Drive(index),
                PlaceKind::Common => ira_core::input::Command::CommonFolder(index),
                PlaceKind::Bookmark => ira_core::input::Command::Bookmark(index),
            });
        }
        Command::Place { kind, path } => {
            if !matches!(
                app.input_context(),
                ira_core::input::InputContext::Pane(_) | ira_core::input::InputContext::Search
            ) {
                return Err("An input or dialog owns focus".into());
            }
            let folders = match kind {
                PlaceKind::Drive => &app.drives,
                PlaceKind::Common => &app.folders,
                PlaceKind::Bookmark => &app.bookmarks,
            };
            let index = folders
                .as_ref()
                .and_then(|folders| folders.iter().position(|folder| folder.path == path))
                .ok_or_else(|| "Place is no longer available".to_string())?;
            app.apply_command(match kind {
                PlaceKind::Drive => ira_core::input::Command::Drive(index),
                PlaceKind::Common => ira_core::input::Command::CommonFolder(index),
                PlaceKind::Bookmark => ira_core::input::Command::Bookmark(index),
            });
        }
        Command::Target { target, verb } => {
            if !matches!(
                app.input_context(),
                ira_core::input::InputContext::Pane(_) | ira_core::input::InputContext::Search
            ) {
                return Err("An input or dialog owns focus".into());
            }
            if app.resolve_target(&target).is_none() {
                return Err("Listing changed; select the entry again".into());
            }
            // Resolve against current visible projection, never the row sent by the view.
            let cursor = app
                .pane_visible_rows(target.pane)
                .iter()
                .position(|(_, entry)| Path::new(&entry.path) == target.path)
                .ok_or_else(|| "Entry is no longer visible".to_string())?;
            app.active_pane = target.pane;
            app.panes[target.pane].state.select(Some(cursor));
            match verb {
                TargetVerb::Focus => {}
                TargetVerb::Toggle => app.toggle_select_current(),
                TargetVerb::Selected(selected) => {
                    let index = app.panes[target.pane]
                        .files
                        .iter()
                        .position(|entry| Path::new(&entry.path) == target.path)
                        .ok_or("Entry is no longer available")?;
                    if let Some(slot) = app.panes[target.pane].selected.get_mut(index) {
                        *slot = selected;
                    }
                    app.invalidate_pane_projection(target.pane);
                }
                TargetVerb::SelectOnly => {
                    let index = app.panes[target.pane]
                        .files
                        .iter()
                        .position(|entry| Path::new(&entry.path) == target.path)
                        .ok_or("Entry is no longer available")?;
                    app.panes[target.pane].selected.fill(false);
                    if let Some(slot) = app.panes[target.pane].selected.get_mut(index) {
                        *slot = true;
                    }
                    app.invalidate_pane_projection(target.pane);
                }
                TargetVerb::Open => app
                    .dispatch(Input::Key(KeyEvent::new(
                        KeyCode::Right,
                        KeyModifiers::NONE,
                    )))
                    .map_err(|error| error.to_string())?,
            }
        }
        Command::Draft { text, cursor } => {
            if let Some(prompt) = &mut app.renaming {
                prompt.text = text.chars().collect();
                prompt.cursor = cursor.min(prompt.text.len());
            } else if let Some(prompt) = &mut app.new_entry {
                prompt.text = text.chars().collect();
                prompt.cursor = cursor.min(prompt.text.len());
            } else if app.goto_prompt.is_some() {
                app.goto_prompt = Some(text);
            } else if app.search_query.is_some() {
                app.search_query = Some(text);
                app.panes[app.active_pane].state.select(Some(0));
            } else {
                return Err("Input mode closed".into());
            }
        }
        Command::EditorDraft {
            document_id,
            revision,
            text,
        } => {
            if !app.update_editor_draft(document_id, revision, text) {
                return Err("Editor draft belongs to a closed or read-only document".into());
            }
        }
        Command::SaveDraft {
            document_id,
            revision,
            text,
        } => {
            if !app.update_editor_draft(document_id, revision, text) {
                return Err("Editor save belongs to a closed or read-only document".into());
            }
            app.save_edit();
        }
        Command::ClipboardResult(success) => app.apply_clipboard_result(success),
        Command::HostResult(Err(error)) => app.set_status(error, true),
        Command::HostResult(Ok(())) => {}
        Command::SetFocus(generation) => app.focus_generation = generation,
    }
    if previous_context != app.input_context() && previous_focus == app.focus_generation {
        app.focus_generation = app.focus_generation.wrapping_add(1);
    }
    app.ack_sequence = envelope.sequence;
    app.revision = app.revision.wrapping_add(1);
    Ok(())
}

// One actor-owned FIFO issuer and independent worker publication. Epochs remain
// business-result guards; they never identify acknowledgements.
#[derive(Default)]
struct EditorSettlement {
    #[cfg(test)]
    hook: Option<EditorHook>,
    through: AtomicU64,
    exited: AtomicBool,
}
struct EditorExit(Arc<EditorSettlement>);
impl Drop for EditorExit {
    fn drop(&mut self) {
        self.0.exited.store(true, Ordering::Release);
    }
}
struct EditorDispatch {
    sender: std::cell::RefCell<Option<mpsc::Sender<(u64, u64, HostRequest)>>>,
    issued: std::cell::Cell<u64>,
    error: std::cell::RefCell<Option<String>>,
    settlement: Arc<EditorSettlement>,
}
impl EditorDispatch {
    fn submit(&self, epoch: u64, request: HostRequest) {
        if self.error.borrow().is_some() {
            return;
        }
        let Some(sequence) = self.issued.get().checked_add(1) else {
            *self.error.borrow_mut() = Some("Editor settlement sequence exhausted".into());
            self.sender.borrow_mut().take();
            return;
        };
        // Accounting precedes send; a failed delivery never leaves a silent hole.
        self.issued.set(sequence);
        let delivered = self
            .sender
            .borrow()
            .as_ref()
            .is_some_and(|sender| sender.send((sequence, epoch, request)).is_ok());
        if !delivered {
            *self.error.borrow_mut() = Some("Editor request channel disconnected or sealed".into());
            self.sender.borrow_mut().take();
        }
    }
    fn seal(&self) -> u64 {
        self.sender.borrow_mut().take();
        self.issued.get()
    }
    fn poll(&self, cutoff: u64) -> Result<bool, String> {
        if let Some(error) = &*self.error.borrow() {
            return Err(error.clone());
        }
        if self.settlement.through.load(Ordering::Acquire) >= cutoff {
            return Ok(true);
        }
        if self.settlement.exited.load(Ordering::Acquire)
            && self.settlement.through.load(Ordering::Acquire) < cutoff
        {
            return Err("Editor worker exited before sealed acknowledgement".into());
        }
        Ok(false)
    }
}
fn editor_worker(
    requests: Receiver<(u64, u64, HostRequest)>,
    results: mpsc::Sender<EditorResult>,
    stop: Arc<AtomicBool>,
    attached: Arc<AtomicU64>,
    settlement: Arc<EditorSettlement>,
) {
    let _exit = EditorExit(settlement.clone());
    let mut sessions = std::collections::HashMap::new();
    let mut expected = 1u64;
    while let Ok((sequence, epoch, request)) = requests.recv() {
        if sequence != expected {
            break;
        }
        let result = if stop.load(Ordering::Acquire) || attached.load(Ordering::Acquire) != epoch {
            None // Queued stopped/stale work is skipped, then acknowledged in FIFO order.
        } else {
            Some(match request {
                HostRequest::OpenEditor(request) => {
                    let result =
                        ira_core::editor::open_document(request.document_id, &request.target.path);
                    sessions.clear();
                    if let Ok(document) = &result {
                        sessions.insert(document.id, EditorSession::new(document.clone()));
                    }
                    EditorResult::Opened(epoch, request, result)
                }
                HostRequest::SaveEditor(snapshot) => {
                    let result = sessions
                        .get(&snapshot.document_id)
                        .ok_or_else(|| EditorError("Editor session closed".into()))
                        .and_then(|session| {
                            #[cfg(test)]
                            if let Some(hook) = &settlement.hook {
                                hook();
                            }
                            session.save(&snapshot)
                        });
                    EditorResult::Saved(epoch, snapshot.document_id, snapshot.edit_revision, result)
                }
                _ => break,
            })
        };
        // Save returned after staging rename/cleanup and post-save metadata.
        settlement.through.store(sequence, Ordering::Release);
        if let Some(result) = result {
            // Even a disconnected business-result consumer cannot strand queued
            // stopped items; the receipt lane remains independent of this channel.
            let _ = results.send(result);
        }
        let Some(next) = expected.checked_add(1) else {
            break;
        };
        expected = next;
    }
}

enum EditorResult {
    Drives(
        u64,
        u64,
        Result<Vec<ira_core::domain::data::Folder>, String>,
    ),
    Opened(u64, OpenEditorRequest, Result<EditorDocument, EditorError>),
    Saved(u64, u64, u64, Result<SaveCompletion, EditorError>),
}
fn route_host(
    app: &App,
    request: HostRequest,
    sequence: u64,
    editor: &EditorDispatch,
    drives: &mpsc::Sender<u64>,
    completed: &mpsc::Sender<Completion>,
) {
    if matches!(request, HostRequest::RefreshDrives) {
        let _ = drives.send(app.window_generation);
    } else if matches!(
        request,
        HostRequest::OpenEditor(_) | HostRequest::SaveEditor(_)
    ) {
        editor.submit(app.window_generation, request);
    } else {
        let input_generation = matches!(
            request,
            HostRequest::EditorKey { .. } | HostRequest::EditorPaste { .. }
        )
        .then_some((app.document_generation, app.focus_generation));
        let _ = completed.send(Completion::Host {
            sequence,
            window_generation: app.window_generation,
            input_generation,
            request,
        });
    }
}

#[derive(Clone)]
pub struct EffectGuard {
    attached: Arc<AtomicU64>,
    stopping: Arc<AtomicBool>,
    window_generation: u64,
}
impl EffectGuard {
    pub fn is_current(&self) -> bool {
        self.attached.load(Ordering::Acquire) == self.window_generation
            && !self.stopping.load(Ordering::Acquire)
    }
}

fn synchronize_attachment(
    app: &mut App,
    epoch: u64,
    editor: &EditorDispatch,
    drives: &mpsc::Sender<u64>,
    completed: &mpsc::Sender<Completion>,
    admission: &ActorAdmission,
) -> bool {
    if app.window_generation == epoch {
        return false;
    }
    let old_window = app.window_generation;
    let abandoned = app.take_host_requests();
    app.attach_window(epoch);
    for request in abandoned {
        if matches!(request, HostRequest::RefreshDrives) {
            if !admission.admit(app, ActorUnit::Host) {
                return true;
            }
            route_host(app, request, app.ack_sequence, editor, drives, completed);
            if !admission.finish(app, ActorUnit::Host) {
                return true;
            }
        } else {
            let _ = completed.send(Completion::Rejected {
                sequence: app.ack_sequence,
                window_generation: old_window,
                reason: "Native request belongs to a replaced window".into(),
            });
        }
    }
    true
}

#[path = "runtime_chooser.rs"]
mod chooser_runtime;
pub use chooser_runtime::{AdmittedChooser, ChooserEvent, ChooserFocusPermit};
impl Runtime {
    /// None means transient lock pressure; a host must defer rather than cancel.
    pub fn chooser_is_current(&self, request: &AdmittedChooser) -> Option<bool> {
        if !self.effect_guard(request.context.window).is_current() {
            return Some(false);
        }
        Some(self.chooser_permit.try_lock().ok()?.as_ref() == Some(request))
    }
    pub fn chooser_focus_is_current(&self, permit: &ChooserFocusPermit) -> Option<bool> {
        if !self.effect_guard(permit.window).is_current() {
            return Some(false);
        }
        Some(self.chooser_focus.try_lock().ok()?.as_ref() == Some(permit))
    }
    pub fn enqueue_current(&mut self, command: Command) -> u64 {
        self.window_generation = self.attached_epoch();
        self.enqueue(command, None)
    }
    pub fn attached_epoch(&self) -> u64 {
        self.attached_window.load(Ordering::Acquire)
    }
    pub fn try_chooser_event(&self) -> Option<ChooserEvent> {
        self.chooser_events.try_lock().ok()?.try_recv().ok()
    }
}

#[cfg(test)]
#[path = "runtime_stop_admission_tests.rs"]
mod stop_admission_tests;

#[cfg(test)]
#[path = "runtime_settlement_tests.rs"]
mod settlement_tests;
