//! One background owner; foreground reads never wait for the actor.
use ira_core::{
    application::App,
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

#[derive(Clone, Debug)]
pub enum Command {
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
pub struct Runtime {
    sender: SyncSender<Envelope>,
    latest: Arc<Latest<Publication>>,
    completions: Arc<Mutex<Receiver<Completion>>>,
    attached_window: Arc<AtomicU64>,
    stopping: Arc<AtomicBool>,
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
        let latest = Arc::new(Latest::default());
        let stopping = Arc::new(AtomicBool::new(false));
        let publication = latest.clone();
        let stop = stopping.clone();
        let attached_window = Arc::new(AtomicU64::new(window_generation));
        let attached = attached_window.clone();
        thread::spawn(move || {
            let (mut app, loader, font_family) = factory();
            app.attach_window(window_generation);
            let (editor_tx, editor_rx) = mpsc::channel::<(u64, HostRequest)>();
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
            thread::spawn(move || {
                let mut sessions = std::collections::HashMap::new();
                while let Ok((epoch, request)) = editor_rx.recv() {
                    if editor_stop.load(Ordering::Acquire) {
                        break;
                    }
                    if editor_attached.load(Ordering::Acquire) != epoch {
                        continue;
                    }
                    let result = match request {
                        HostRequest::OpenEditor(request) => {
                            let result = ira_core::editor::open_document(
                                request.document_id,
                                &request.target.path,
                            );
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
                                .and_then(|session| session.save(&snapshot));
                            EditorResult::Saved(
                                epoch,
                                snapshot.document_id,
                                snapshot.edit_revision,
                                result,
                            )
                        }
                        _ => continue,
                    };
                    if editor_done.send(result).is_err() {
                        break;
                    }
                }
            });
            let mut last_tick = Instant::now();
            publish(&app, &publication, &loader, &font_family);
            while !stop.load(Ordering::Acquire) && app.running {
                if synchronize_attachment(
                    &mut app,
                    attached.load(Ordering::Acquire),
                    &editor_tx,
                    &drive_tx,
                    &completed,
                ) {
                    publish(&app, &publication, &loader, &font_family);
                }
                for result in editor_results.try_iter() {
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
                    publish(&app, &publication, &loader, &font_family);
                }
                match commands.recv_timeout(Duration::from_millis(20)) {
                    Ok(envelope) => {
                        synchronize_attachment(
                            &mut app,
                            attached.load(Ordering::Acquire),
                            &editor_tx,
                            &drive_tx,
                            &completed,
                        );
                        let sequence = envelope.sequence;
                        let window_generation = envelope.window_generation;
                        if let Err(reason) = apply(&mut app, envelope) {
                            let _ = completed.send(Completion::Rejected {
                                sequence,
                                window_generation,
                                reason,
                            });
                        }
                        for request in app.take_host_requests() {
                            route_host(&app, request, sequence, &editor_tx, &drive_tx, &completed);
                        }
                        publish(&app, &publication, &loader, &font_family);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                if last_tick.elapsed() >= Duration::from_millis(500) {
                    let context = app.input_context();
                    app.tick();
                    if context != app.input_context() {
                        app.focus_generation = app.focus_generation.wrapping_add(1);
                    }
                    for request in app.take_host_requests() {
                        route_host(
                            &app,
                            request,
                            app.ack_sequence,
                            &editor_tx,
                            &drive_tx,
                            &completed,
                        );
                    }
                    publish(&app, &publication, &loader, &font_family);
                    last_tick = Instant::now();
                }
            }
            stop.store(true, Ordering::Release);
            app.cancel_pending_work();
            cancel_all(&app);
            app.persist_state();
            // Persistence drain is off both GPUI and command lanes and has an explicit bound.
            let _ = app
                .persistence_barrier()
                .recv_timeout(Duration::from_secs(2));
            let _ = completed.send(Completion::Closed);
        });
        Self {
            sender,
            latest,
            completions: Arc::new(Mutex::new(completions)),
            attached_window,
            stopping,
            window_generation,
            next_sequence: Arc::new(AtomicU64::new(1)),
            pending: VecDeque::new(),
        }
    }
    pub fn enqueue(&mut self, command: Command, input_generation: Option<(u64, u64)>) -> u64 {
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
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
    /// Does not enqueue, lock or join. Known worker controls are canceled immediately.
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
    if previous_context != app.input_context() {
        app.focus_generation = app.focus_generation.wrapping_add(1);
    }
    app.ack_sequence = envelope.sequence;
    app.revision = app.revision.wrapping_add(1);
    Ok(())
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
    editor: &mpsc::Sender<(u64, HostRequest)>,
    drives: &mpsc::Sender<u64>,
    completed: &mpsc::Sender<Completion>,
) {
    if matches!(request, HostRequest::RefreshDrives) {
        let _ = drives.send(app.window_generation);
    } else if matches!(
        request,
        HostRequest::OpenEditor(_) | HostRequest::SaveEditor(_)
    ) {
        let _ = editor.send((app.window_generation, request));
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
    editor: &mpsc::Sender<(u64, HostRequest)>,
    drives: &mpsc::Sender<u64>,
    completed: &mpsc::Sender<Completion>,
) -> bool {
    if app.window_generation == epoch {
        return false;
    }
    let old_window = app.window_generation;
    let abandoned = app.take_host_requests();
    app.attach_window(epoch);
    for request in abandoned {
        if matches!(request, HostRequest::RefreshDrives) {
            route_host(app, request, app.ack_sequence, editor, drives, completed);
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
