pub mod accessibility;
pub mod accessibility_bridge;
pub mod accessibility_prepared;
pub mod accessibility_retirement;
pub mod accessibility_worker;
mod appearance;
pub mod chrome;
pub mod preview;
pub mod rows;
use crate::platform::accessibility::{
    ResolvedAction,
    model::{Action as AxAction, Capability as AxCapability, Role as AxRole, Target as AxTarget},
};
use crate::{
    actions,
    components::text_input::{
        self, InputEvent, InputEventKind, InputOptions, SearchNavigation, TextInput,
    },
    focus::{self, InputMode},
    keymap,
    runtime::{Command, Completion, JobVerb, PlaceKind, Runtime, TargetVerb},
};
use gpui::{
    ClickEvent, ClipboardItem, Context, Entity, FocusHandle, Focusable, IntoElement, KeyDownEvent,
    Render, ScrollStrategy, ScrollWheelEvent, Subscription, Task, UniformListScrollHandle, Window,
    div, prelude::*, px, uniform_list,
};
use ira_core::{
    input::{Input, KeyCode, KeyEvent, KeyModifiers},
    model::{EntryTarget, HostRequest},
    observable::Snapshot,
    services::transfer::JobControl,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, mpsc},
    time::Duration,
};

pub struct Desktop {
    runtime: Runtime,
    accessibility: accessibility::Host,
    preview: preview::Host,
    accessibility_actions: Vec<ResolvedAction>,
    snapshot: Option<Arc<Snapshot>>,
    controls: Vec<Arc<JobControl>>,
    focus: FocusHandle,
    place_focus: BTreeMap<AxTarget, FocusHandle>,
    scroll: [UniformListScrollHandle; 2],
    grid_columns: [usize; 2],
    input: Option<Entity<TextInput>>,
    input_mode: Option<InputMode>,
    native_text_cache: Option<crate::platform::accessibility::model::NativeTextSnapshot>,
    forwarded_input: Option<text_input::InputStamp>,
    input_generation: u64,
    input_subscription: Option<Subscription>,
    polling: Option<Task<()>>,
    feedback: Option<String>,
    theme: ira_core::theme::Theme,
    palette: appearance::Palette,
    appearance_subscription: Option<Subscription>,
    font_family: Option<String>,
    geometry: crate::platform::geometry::Writer,
    geometry_subscription: Option<Subscription>,
    host_tx: mpsc::Sender<Result<(), String>>,
    host_rx: mpsc::Receiver<Result<(), String>>,
}
impl Desktop {
    pub fn new(
        runtime: Runtime,
        geometry: crate::platform::geometry::Writer,
        cx: &mut Context<Self>,
    ) -> Self {
        let (host_tx, host_rx) = mpsc::channel();
        let polling = cx.spawn(async move |entity, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(30))
                    .await;
                if entity.update(cx, |this, cx| this.poll(cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            runtime,
            accessibility: if cx.has_global::<accessibility_retirement::Retirement>() {
                accessibility::Host::with_retirement(
                    cx.global::<accessibility_retirement::Retirement>().clone(),
                )
            } else {
                accessibility::Host::headless()
            },
            preview: preview::Host::new(),
            accessibility_actions: Vec::new(),
            snapshot: None,
            controls: Vec::new(),
            focus: cx.focus_handle(),
            place_focus: BTreeMap::new(),
            scroll: std::array::from_fn(|_| UniformListScrollHandle::new()),
            grid_columns: [1; 2],
            input: None,
            input_mode: None,
            native_text_cache: None,
            forwarded_input: None,
            input_generation: 0,
            input_subscription: None,
            polling: Some(polling),
            feedback: None,
            theme: ira_core::theme::ThemePreset::default().theme(),
            palette: appearance::Palette::default(),
            appearance_subscription: None,
            font_family: None,
            geometry,
            geometry_subscription: None,
            host_tx,
            host_rx,
        }
    }
    pub fn restore_chooser_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.runtime.is_stopping() {
            window.focus(&self.focus);
            cx.notify();
        }
    }
    pub fn focus_main(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        self.palette.appearance = window.appearance();
        self.theme = self.palette.selected();
        self.appearance_subscription =
            Some(cx.observe_window_appearance(window, |this, window, cx| {
                this.palette.appearance = window.appearance();
                this.theme = this.palette.selected();
                cx.notify();
            }));
        self.geometry_subscription = Some(cx.observe_window_bounds(window, |this, window, _| {
            if this.runtime.is_stopping() {
                return;
            }
            this.geometry
                .save(crate::platform::geometry::Geometry::from_bounds(
                    window.window_bounds(),
                ));
        }));
        cx.notify();
    }
    pub fn close(&mut self) {
        crate::lifecycle_trace("Desktop close/detach");
        self.preview.close();
        if let Some(text) = self.native_text_cache.take() {
            self.accessibility.retire_native_text(text);
        }
        if let Some(snapshot) = self.snapshot.take() {
            self.accessibility.retire_snapshot(snapshot);
        }
        self.accessibility.close();
        self.accessibility_actions.clear();
        self.runtime.detach();
        self.runtime.cancel_jobs(&self.controls);
        self.polling.take();
        self.appearance_subscription.take();
    }
    pub fn shutdown_feedback(&mut self, message: String, cx: &mut Context<Self>) {
        self.runtime.stop(&self.controls);
        if let Some(input) = &self.input {
            input.update(cx, |input, cx| {
                input.set_access(text_input::InputAccess::Disabled, cx)
            });
        }
        if self.feedback.as_ref() != Some(&message) {
            self.feedback = Some(message);
            cx.notify();
        }
    }
    pub fn is_stopping(&self) -> bool {
        self.runtime.is_stopping()
    }
    fn poll(&mut self, cx: &mut Context<Self>) {
        if self.accessibility.poll() {
            cx.notify();
        }
        if self.preview.poll(self.snapshot.as_deref()) {
            cx.notify();
        }
        self.runtime.flush();
        if let Some(error) = self.geometry.try_error() {
            self.runtime.enqueue(Command::HostResult(Err(error)), None);
        }
        let mut changed = false;
        if self.accessibility.can_accept_snapshot()
            && let Some(publication) = self.runtime.try_snapshot()
            && publication.snapshot.window_generation == self.runtime.window_generation
        {
            for (index, pane) in publication.snapshot.panes.iter().enumerate() {
                if self.snapshot.as_ref().is_none_or(|old| {
                    old.panes[index].cursor != pane.cursor
                        || old.panes[index].listing_generation != pane.listing_generation
                }) && let Some(cursor) = pane.cursor
                {
                    self.scroll[index].scroll_to_item(
                        if pane.preview_mode == ira_core::model::PreviewMode::Grid {
                            cursor / self.grid_columns[index]
                        } else {
                            cursor
                        },
                        ScrollStrategy::Center,
                    );
                }
            }
            self.palette.configured = publication.theme;
            self.palette.light = publication.light_theme;
            self.theme = self.palette.selected();
            self.font_family = publication.font_family;
            if let Some(old) = self.snapshot.replace(publication.snapshot) {
                self.accessibility.retire_snapshot(old);
            }
            self.controls = publication.cancellation;
            changed = true;
        }
        // Bounded batches preserve foreground responsiveness under a busy producer.
        for _ in 0..64 {
            let Some(completion) = self.runtime.try_completion() else {
                break;
            };
            match completion {
                Completion::Rejected {
                    window_generation,
                    reason,
                    ..
                } => {
                    if window_generation == self.runtime.window_generation {
                        self.feedback = Some(reason);
                    }
                }
                Completion::Host {
                    window_generation,
                    input_generation,
                    request,
                    ..
                } => {
                    if input_generation.is_some_and(|generation| {
                        self.snapshot.as_ref().is_none_or(|snapshot| {
                            generation != (snapshot.document_generation, snapshot.focus_generation)
                        })
                    }) {
                        continue;
                    }
                    self.host(request, self.runtime.effect_guard(window_generation), cx)
                }
                Completion::Closed => {
                    // A checked business receipt is complete. AppGlobal owns the
                    // geometry receipt/native token and is the only quit approver.
                }
            }
            changed = true;
        }
        for _ in 0..64 {
            let Ok(result) = self.host_rx.try_recv() else {
                break;
            };
            self.runtime.enqueue(Command::HostResult(result), None);
            changed = true;
        }
        if self.runtime.backlog() > 0 {
            self.feedback = Some(format!(
                "{} commands waiting for the worker",
                self.runtime.backlog()
            ));
            changed = true;
        }
        for _ in 0..64 {
            let Some(action) = self
                .accessibility
                .receiver
                .as_ref()
                .and_then(|receiver| receiver.try_next())
            else {
                break;
            };
            match action {
                Ok(action) => self.accessibility_actions.push(action),
                Err(error) => {
                    self.feedback = Some(format!("Accessibility request rejected: {error:?}"))
                }
            }
            changed = true;
        }
        if changed {
            cx.notify();
        }
    }
    fn host(
        &mut self,
        request: HostRequest,
        guard: crate::runtime::EffectGuard,
        cx: &mut Context<Self>,
    ) {
        if !guard.is_current() {
            return;
        }
        match request {
            HostRequest::CopyText(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.runtime.enqueue(Command::ClipboardResult(true), None);
            }
            HostRequest::RefreshDrives => {
                self.feedback = Some("Drive refresh adapter pending".into());
            }
            HostRequest::OpenEditor(_) | HostRequest::SaveEditor(_) => {
                unreachable!("editor requests stay on actor worker lane")
            }
            HostRequest::InvalidatePreview(path) => {
                self.preview.invalidate(&path);
            }
            HostRequest::EditorKey { .. } | HostRequest::EditorPaste { .. } => {
                self.feedback = Some("Native editor input requires a current document".into());
            }
            request => {
                let tx = self.host_tx.clone();
                std::thread::spawn(move || {
                    if guard.is_current() {
                        let _ = tx.send(crate::platform::execute(request));
                    }
                });
            }
        }
    }
    fn dispatch(&mut self, code: KeyCode, cx: &mut Context<Self>) {
        if self.runtime.is_stopping() {
            return;
        }
        self.runtime
            .enqueue(Command::Input(actions::input(code)), None);
        cx.notify();
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.runtime.is_stopping() {
            cx.stop_propagation();
            return;
        }
        if self.input.is_some() {
            // Input widgets own composition/caret keys. Ctrl+A in search remains a file action.
            if self.input_mode == Some(InputMode::Search)
                && event.keystroke.modifiers.control
                && !event.keystroke.modifiers.alt
                && !event.keystroke.modifiers.shift
                && !event.keystroke.modifiers.platform
                && event.keystroke.key.eq_ignore_ascii_case("a")
            {
                self.runtime.enqueue(
                    Command::Input(Input::Key(keymap::decode(&event.keystroke))),
                    None,
                );
                cx.stop_propagation();
            }
            return;
        }
        let key = keymap::decode(&event.keystroke);
        if key.modifiers.is_empty()
            && let Some((target, _)) = self
                .place_focus
                .iter()
                .find(|(_, handle)| handle.is_focused(window))
        {
            match key.code {
                KeyCode::Enter | KeyCode::Right => {
                    if let AxTarget::Place {
                        kind,
                        path,
                        shortcut,
                        occurrence,
                    } = target
                    {
                        let kind = match kind {
                            crate::platform::accessibility::model::PlaceKind::Drive => {
                                PlaceKind::Drive
                            }
                            crate::platform::accessibility::model::PlaceKind::Common => {
                                PlaceKind::Common
                            }
                            crate::platform::accessibility::model::PlaceKind::Bookmark => {
                                PlaceKind::Bookmark
                            }
                        };
                        self.runtime.enqueue(
                            Command::PlaceExact {
                                kind,
                                path: path.to_string_lossy().into_owned(),
                                shortcut: *shortcut,
                                occurrence: *occurrence,
                            },
                            None,
                        );
                    }
                    window.focus(&self.focus);
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                KeyCode::Esc | KeyCode::Tab => {
                    window.focus(&self.focus);
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                _ => {}
            }
        }
        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
            crate::lifecycle_trace("pane control-c quit");
            self.runtime.stop(&self.controls);
            cx.stop_propagation();
            return;
        }
        self.runtime.enqueue(Command::Input(Input::Key(key)), None);
        cx.stop_propagation();
        cx.notify();
    }
    fn sync_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let mode = focus::input_mode(snapshot);
        // Editor context starts while the actor's open worker still owns a focus
        // ticket. Creating/focusing an empty native buffer here would invalidate
        // that ticket before its document completion can be applied.
        if mode == Some(InputMode::Editor) && snapshot.edit.is_none() {
            self.input = None;
            if let Some(text) = self.native_text_cache.take() {
                self.accessibility.retire_native_text(text);
            }
            self.input_mode = None;
            self.input_subscription = None;
            window.focus(&self.focus);
            return;
        }
        if mode == self.input_mode
            && self.input.as_ref().is_none_or(|input| {
                input.read(cx).stamp().document_generation == snapshot.document_generation
                    && self.input_generation >= snapshot.focus_generation
            })
        {
            return;
        }
        self.input_generation = self.input_generation.max(snapshot.focus_generation) + 1;
        self.runtime
            .enqueue(Command::SetFocus(self.input_generation), None);
        self.input_mode = mode;
        if let Some(text) = self.native_text_cache.take() {
            self.accessibility.retire_native_text(text);
        }
        self.input = None;
        self.input_subscription = None;
        let Some(mode) = mode else {
            window.focus(&self.focus);
            return;
        };
        let text = match mode {
            InputMode::Rename => snapshot
                .renaming
                .as_ref()
                .map(|p| p.text.clone())
                .unwrap_or_default(),
            InputMode::Create => snapshot
                .new_entry
                .as_ref()
                .map(|p| p.text.clone())
                .unwrap_or_default(),
            InputMode::Goto => snapshot.goto_prompt.clone().unwrap_or_default(),
            InputMode::Search => snapshot.search_query.clone().unwrap_or_default(),
            InputMode::Editor => snapshot
                .edit
                .as_ref()
                .map(|e| e.content.clone())
                .unwrap_or_default(),
        };
        let options = InputOptions {
            access: if mode == InputMode::Editor
                && snapshot.edit.as_ref().is_some_and(|edit| edit.read_only)
            {
                text_input::InputAccess::ReadOnly
            } else {
                text_input::InputAccess::Editable
            },
            multiline: mode == InputMode::Editor,
            search: mode == InputMode::Search,
            document_generation: snapshot.document_generation,
            focus_generation: self.input_generation,
            ..Default::default()
        };
        let read_only = options.access == text_input::InputAccess::ReadOnly;
        let input = cx.new(|cx| TextInput::new(text, options, cx));
        self.input_subscription = Some(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
            this.input_event(event, cx)
        }));
        input.update(cx, |input, cx| input.focus(window, cx));
        self.native_text_cache = Some(accessibility::native_text(&input, mode, read_only, cx));
        self.forwarded_input = Some(input.read(cx).stamp());
        self.input = Some(input);
    }
    fn input_event(&mut self, event: &InputEvent, cx: &mut Context<Self>) {
        if event.stamp.focus_generation != self.input_generation {
            return;
        }
        let generation = Some((
            event.stamp.document_generation,
            event.stamp.focus_generation,
        ));
        let selection_only = matches!(&event.kind, InputEventKind::Changed(_))
            && self.forwarded_input == Some(event.stamp);
        if let InputEventKind::Changed(buffer) = &event.kind {
            let range = |range: std::ops::Range<usize>| {
                buffer.text[..range.start].encode_utf16().count()
                    ..buffer.text[..range.end].encode_utf16().count()
            };
            let text = self
                .native_text_cache
                .as_ref()
                .filter(|old| {
                    old.revision == event.stamp.value_revision
                        && old.document_generation == event.stamp.document_generation
                })
                .map(|old| old.text.clone())
                .unwrap_or_else(|| Arc::<str>::from(buffer.text.as_str()));
            let native = crate::platform::accessibility::model::NativeTextSnapshot {
                document_generation: event.stamp.document_generation,
                focus_generation: event.stamp.focus_generation,
                revision: event.stamp.value_revision,
                text,
                selection_utf16: range(buffer.selection.clone()),
                marked_utf16: buffer.marked.clone().map(range),
                read_only: self
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.edit.as_ref())
                    .is_some_and(|e| e.read_only),
                disabled: false,
                multiline: self.input_mode == Some(InputMode::Editor),
            };
            if let Some(old) = self.native_text_cache.replace(native) {
                self.accessibility.retire_native_text(old);
            }
        }
        if selection_only && self.input_mode == Some(InputMode::Editor) {
            cx.notify();
            return;
        }
        let command = match &event.kind {
            InputEventKind::Changed(buffer) if self.input_mode == Some(InputMode::Editor) => {
                let Some(edit) = self
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.edit.as_ref())
                else {
                    return;
                };
                Command::EditorDraft {
                    document_id: edit.document_id,
                    revision: event.stamp.value_revision,
                    text: buffer.text.clone(),
                }
            }
            InputEventKind::Changed(buffer) => Command::Draft {
                text: buffer.text.clone(),
                cursor: buffer.text[..buffer.selection.end.min(buffer.text.len())]
                    .chars()
                    .count(),
            },
            InputEventKind::Submit(text) => {
                self.runtime.enqueue(
                    Command::Draft {
                        text: text.clone(),
                        cursor: text.chars().count(),
                    },
                    generation,
                );
                Command::Input(actions::input(KeyCode::Enter))
            }
            InputEventKind::Cancel => Command::Input(actions::input(KeyCode::Esc)),
            InputEventKind::Quit => {
                crate::lifecycle_trace("native text input Quit event");
                self.runtime.stop(&self.controls);
                cx.notify();
                return;
            }
            InputEventKind::Traverse => Command::Input(actions::input(KeyCode::Tab)),
            InputEventKind::Save(text) => {
                let Some(edit) = self
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.edit.as_ref())
                else {
                    return;
                };
                Command::SaveDraft {
                    document_id: edit.document_id,
                    revision: event.stamp.value_revision,
                    text: text.clone(),
                }
            }
            InputEventKind::SearchNavigate(direction) => {
                Command::Input(Input::Key(match direction {
                    SearchNavigation::Open => KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
                    SearchNavigation::Previous => KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
                    SearchNavigation::Next => KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
                    SearchNavigation::Top => KeyEvent::new(KeyCode::Up, KeyModifiers::ALT),
                    SearchNavigation::Bottom => KeyEvent::new(KeyCode::Down, KeyModifiers::ALT),
                }))
            }
        };
        if matches!(&event.kind, InputEventKind::Changed(_)) {
            self.forwarded_input = Some(event.stamp);
        }
        self.runtime.enqueue(command, generation);
        cx.notify();
    }
    fn refresh_native_text(&mut self, cx: &gpui::App) {
        let Some(input) = &self.input else { return };
        let Some(mode) = self.input_mode else { return };
        let native = accessibility::native_text(
            input,
            mode,
            self.snapshot
                .as_ref()
                .and_then(|s| s.edit.as_ref())
                .is_some_and(|e| e.read_only),
            cx,
        );
        if let Some(old) = self.native_text_cache.replace(native) {
            self.accessibility.retire_native_text(old);
        }
    }
    fn native_text(
        &self,
        cx: &gpui::App,
    ) -> Option<crate::platform::accessibility::model::NativeTextSnapshot> {
        let _ = cx;
        self.native_text_cache.clone()
    }
    fn rendered_footer(&self) -> Arc<str> {
        self.feedback
            .clone()
            .or_else(|| {
                self.snapshot
                    .as_ref()
                    .and_then(|s| s.status.as_ref().map(|s| s.text.clone()))
            })
            .unwrap_or_else(|| "Enter: rename · Right: open · /: search · Space: select".into())
            .into()
    }
    fn accessibility_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.runtime.is_stopping() {
            self.accessibility_actions.clear();
            return;
        }
        for action in std::mem::take(&mut self.accessibility_actions) {
            let Some(snapshot) = &self.snapshot else {
                continue;
            };
            let text = self.native_text(cx);
            let footer = self.rendered_footer();
            let key = self.accessibility.observe(snapshot, text.as_ref(), footer);
            let prepared_matches = action.prepared_key == Some(key);
            if (!prepared_matches && !self.accessibility.is_compatibility())
                || (
                    action.stamp.window,
                    action.stamp.document,
                    action.stamp.focus,
                    action.stamp.revision,
                    action.stamp.text_revision,
                ) != (
                    snapshot.window_generation,
                    snapshot.document_generation,
                    snapshot.focus_generation,
                    snapshot.revision,
                    text.as_ref().map_or(0, |t| t.revision),
                )
            {
                crate::lifecycle_trace(&format!(
                    "AX action rejected: window={} document={} focus={} semantic={} native_text={}",
                    action.stamp.window != snapshot.window_generation,
                    action.stamp.document != snapshot.document_generation,
                    action.stamp.focus != snapshot.focus_generation,
                    action.stamp.revision != snapshot.revision,
                    action.stamp.text_revision != text.as_ref().map_or(0, |t| t.revision),
                ));
                self.feedback = Some("Accessibility request belongs to an older frame".into());
                continue;
            }
            match (&action.target, &action.action) {
                (target @ AxTarget::Place { .. }, AxAction::Focus) => {
                    if let Some(handle) = self.place_focus.get(target) {
                        window.focus(handle);
                        cx.notify();
                    }
                }
                (AxTarget::Text { document }, AxAction::Focus)
                    if *document == snapshot.document_generation =>
                {
                    if let Some(input) = &self.input {
                        input.update(cx, |input, cx| input.focus(window, cx));
                    }
                }
                (AxTarget::Text { document }, AxAction::SetValue(value))
                    if *document == snapshot.document_generation =>
                {
                    if let Some(input) = &self.input {
                        let result =
                            input.update(cx, |input, cx| input.edit_value(value, window, cx));
                        if let Err(error) = result {
                            self.feedback = Some(format!("Accessibility text edit: {error:?}"));
                        } else {
                            self.refresh_native_text(cx);
                        }
                    }
                }
                (AxTarget::Text { document }, AxAction::SetSelection(range))
                    if *document == snapshot.document_generation =>
                {
                    if let Some(input) = &self.input {
                        let result = input.update(cx, |input, cx| {
                            input.set_selection_utf16(range.clone(), false, window, cx)
                        });
                        if let Err(error) = result {
                            self.feedback =
                                Some(format!("Accessibility text selection: {error:?}"));
                        } else {
                            self.refresh_native_text(cx);
                        }
                    }
                }
                (
                    AxTarget::Entry {
                        pane,
                        path,
                        listing_generation,
                    },
                    AxAction::Reveal,
                ) => {
                    if snapshot
                        .panes
                        .get(*pane)
                        .is_some_and(|p| p.listing_generation == *listing_generation)
                        && let Some(index) = snapshot.panes[*pane]
                            .rows
                            .iter()
                            .position(|r| std::path::Path::new(&r.entry.path) == path)
                    {
                        self.scroll[*pane].scroll_to_item(
                            if snapshot.panes[*pane].preview_mode
                                == ira_core::model::PreviewMode::Grid
                            {
                                index / self.grid_columns[*pane]
                            } else {
                                index
                            },
                            ScrollStrategy::Center,
                        );
                    }
                }
                _ => {
                    self.runtime.enqueue(Command::Accessibility(action), None);
                }
            }
        }
    }
    fn target(
        &mut self,
        target: EntryTarget,
        verb: TargetVerb,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus);
        self.runtime.enqueue(Command::Target { target, verb }, None);
        cx.notify();
    }
    fn button(
        &self,
        label: &'static str,
        code: KeyCode,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let button = div()
            .id(label)
            .px_3()
            .py_1()
            .rounded(px(4.))
            .border_1()
            .border_color(chrome::to_gpui_color(self.theme.border))
            .bg(native_color(self.theme.surface_alt))
            .hover(|s| s.bg(chrome::to_gpui_color(self.theme.selection)))
            .text_size(px(12.))
            .text_color(chrome::to_gpui_color(self.theme.text))
            .cursor_pointer()
            .child(label)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.dispatch(code, cx)));
        let capability = if matches!(label, "Confirm" | "y: Yes") {
            Some(AxCapability::Activate)
        } else if matches!(label, "Cancel" | "n: No" | "Esc: Close") {
            Some(AxCapability::Dismiss)
        } else {
            None
        };
        if capability.is_some() {
            self.accessibility
                .measured(button, AxTarget::Modal, Some(AxRole::Button), capability)
        } else {
            button.into_any_element()
        }
    }
    fn file_row(
        &self,
        index: usize,
        row_index: usize,
        grid: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(snapshot) = &self.snapshot else {
            return div().into_any_element();
        };
        let pane = &snapshot.panes[index];
        let Some(row) = pane.rows.get(row_index) else {
            return div().into_any_element();
        };
        let target = EntryTarget {
            pane: index,
            path: PathBuf::from(&row.entry.path),
            listing_generation: pane.listing_generation,
        };
        let (glyph, category) = ira_core::theme::icons::icon_for(&row.entry, snapshot.icons);
        let icon_str = if row.deleting || row.size_walk_active {
            chrome::spinner_char(std::time::Instant::now()).to_string()
        } else {
            glyph.to_string()
        };
        let dir_bytes = snapshot
            .sizes
            .iter()
            .find(|(p, s)| s.complete && p == std::path::Path::new(&row.entry.path))
            .map(|(_, s)| s.bytes)
            .or_else(|| (row.entry.is_dir && row.entry.size > 0).then_some(row.entry.size));
        let size_note = dir_bytes
            .map(|b| format!(" ({})", ira_core::services::file_info::human(b)))
            .unwrap_or_default();
        let current = pane.cursor == Some(row_index) && snapshot.active_pane == index;
        let foreground = if current {
            self.theme.cursor_fg
        } else if row.entry.is_dir {
            self.theme.dir
        } else if row.entry.label.starts_with('.') {
            self.theme.hidden
        } else {
            self.theme.color_for(category)
        };
        let mut element = div()
            .id(("row", row_index))
            .px_3()
            .flex()
            .bg(native_color(if current {
                self.theme.cursor_bg
            } else if row.selected {
                self.theme.selection
            } else {
                self.theme.bg
            }))
            .text_color(native_color(foreground))
            .cursor_pointer();
        if grid {
            element = element
                .flex_col()
                .w(px(172.))
                .h(px(160.))
                .rounded(px(4.))
                .justify_between()
                .py_2();
            let thumbnail = preview::Host::grid_key(snapshot, index, row_index)
                .and_then(|key| self.preview.content(&key));
            let thumb_border = chrome::to_gpui_color(if current {
                self.theme.border_active
            } else if row.selected {
                self.theme.accent
            } else {
                self.theme.border
            });
            element = match thumbnail {
                Some(preview::Content::Image(image)) => element.child(
                    div()
                        .w_full()
                        .h(px(112.))
                        .rounded(px(3.))
                        .border_1()
                        .border_color(thumb_border)
                        .bg(native_color(self.theme.surface_alt))
                        .overflow_hidden()
                        .child(
                            gpui::img(image.clone())
                                .w_full()
                                .h(px(110.))
                                .object_fit(gpui::ObjectFit::Contain),
                        ),
                ),
                _ => element.child(
                    div()
                        .w_full()
                        .h(px(112.))
                        .rounded(px(3.))
                        .border_1()
                        .border_color(thumb_border)
                        .bg(native_color(self.theme.surface_alt))
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xl()
                        .child(glyph),
                ),
            };
            let mut footer_row = div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(4.))
                .min_w_0();
            if row.selected {
                footer_row = footer_row.child(chrome::badge(
                    "✓",
                    chrome::to_gpui_color(self.theme.key_fg),
                    chrome::to_gpui_color(self.theme.accent),
                ));
            }
            footer_row = footer_row
                .child(div().flex_shrink_0().text_size(px(12.)).child(icon_str))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .child(format!("{}{size_note}", row.entry.label)),
                );
            element = element.child(footer_row);
        } else {
            element = element
                .h(px(30.))
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .w(px(2.))
                        .h(px(18.))
                        .rounded(px(1.))
                        .bg(if current {
                            chrome::to_gpui_color(self.theme.accent)
                        } else {
                            gpui::rgba(0x0000_0000)
                        }),
                )
                .child(
                    div()
                        .w(px(22.))
                        .flex_shrink_0()
                        .text_size(px(11.))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(chrome::to_gpui_color(if current {
                            self.theme.cursor_fg
                        } else if row.selected {
                            self.theme.accent
                        } else {
                            self.theme.text_muted
                        }))
                        .child(if row.selected { "[*]" } else { "[ ]" }),
                )
                .child(
                    div()
                        .w(px(18.))
                        .flex_shrink_0()
                        .text_color(native_color(foreground))
                        .child(icon_str),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(13.))
                        .font_weight(if row.entry.is_dir {
                            gpui::FontWeight::BOLD
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .child(format!("{}{size_note}", row.entry.label)),
                );
            if pane.preview_mode == ira_core::model::PreviewMode::Details {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                element = element
                    .child(
                        div()
                            .w(px(90.))
                            .flex_shrink_0()
                            .text_right()
                            .text_sm()
                            .text_color(native_color(self.theme.text_muted))
                            .child(rows::detail_size(&row.entry)),
                    )
                    .child(
                        div()
                            .w(px(132.))
                            .flex_shrink_0()
                            .text_right()
                            .text_sm()
                            .text_color(native_color(self.theme.text_muted))
                            .child(rows::modified_ago(row.entry.modified, now)),
                    );
            }
        }
        let element = element.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            let double = matches!(event,ClickEvent::Mouse(e) if e.down.click_count>=2);
            let modifiers = event.modifiers();
            let verb = if double {
                TargetVerb::Open
            } else if modifiers.platform || modifiers.control {
                TargetVerb::Toggle
            } else {
                TargetVerb::Focus
            };
            this.target(target.clone(), verb, window, cx);
        }));
        self.accessibility.measured(
            element,
            AxTarget::Entry {
                pane: index,
                path: PathBuf::from(&row.entry.path),
                listing_generation: pane.listing_generation,
            },
            Some(AxRole::Row),
            None,
        )
    }
    fn pane(&self, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(snapshot) = &self.snapshot else {
            return div().flex_1().child("Loading…").into_any_element();
        };
        let pane = &snapshot.panes[index];
        let folder_label = pane
            .folder
            .as_ref()
            .map(|f| f.label.clone())
            .unwrap_or_else(|| "No folder".into());
        let folder_path = pane
            .folder
            .as_ref()
            .map(|f| f.path.clone())
            .unwrap_or_default();

        let (folder_glyph, _) = ira_core::theme::icons::icon_for(
            &ira_core::services::list_files::FEntry {
                path: folder_path.clone(),
                label: folder_label.clone(),
                is_dir: true,
                size: 0,
                modified: None,
            },
            snapshot.icons,
        );
        let mut title_elements = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(chrome::to_gpui_color(self.theme.dir))
                    .child(folder_glyph),
            )
            .child(
                div()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_size(px(12.))
                    .text_color(chrome::to_gpui_color(self.theme.text))
                    .child(folder_label),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(chrome::to_gpui_color(self.theme.text_muted))
                    .child(folder_path),
            );

        if pane.sort_mode > 0 || pane.preview_mode == ira_core::model::PreviewMode::Details {
            title_elements = title_elements.child(chrome::badge(
                format!("sort: {}", rows::sort_mode_label(pane.sort_mode)),
                chrome::to_gpui_color(self.theme.text_muted),
                chrome::to_gpui_color(self.theme.surface),
            ));
        }

        if snapshot.show_hidden {
            title_elements = title_elements.child(chrome::badge(
                ". hidden",
                chrome::to_gpui_color(self.theme.accent),
                chrome::to_gpui_color(self.theme.surface),
            ));
        }

        if let Some(filter) = &pane.filter_query {
            title_elements = title_elements
                .child(
                    div()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_size(px(11.))
                        .text_color(chrome::to_gpui_color(self.theme.accent))
                        .child(format!("/{filter}")),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(chrome::to_gpui_color(self.theme.text_muted))
                        .child("(Esc clears)"),
                );
        }

        if pane.preview_mode == ira_core::model::PreviewMode::Details {
            title_elements = title_elements.child(
                div()
                    .text_size(px(11.))
                    .text_color(chrome::to_gpui_color(self.theme.text_muted))
                    .child("(details)"),
            );
        } else if pane.preview_mode == ira_core::model::PreviewMode::Grid {
            title_elements = title_elements.child(
                div()
                    .text_size(px(11.))
                    .text_color(chrome::to_gpui_color(self.theme.text_muted))
                    .child("(grid)"),
            );
        }

        let is_active = snapshot.active_pane == index && !snapshot.board_focused && !snapshot.edit_focus;
        let border_color = if is_active {
            chrome::to_gpui_color(self.theme.border_active)
        } else {
            chrome::to_gpui_color(self.theme.border)
        };

        let mut list_container = div().flex().flex_1().min_h_0().min_w_0();
        if pane.rows.is_empty() {
            list_container = list_container.child(div().p_4().child(if !pane.listing_settled {
                "Loading…"
            } else if pane.filter_query.is_some() {
                "No matches"
            } else {
                "Empty folder"
            }));
        } else if pane.preview_mode == ira_core::model::PreviewMode::Grid {
            let columns = self.grid_columns[index];
            list_container = list_container.child(
                uniform_list(
                    ("grid-files", index),
                    pane.rows.len().div_ceil(columns),
                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                        let Some(snapshot) = this.snapshot.clone() else {
                            return Vec::new();
                        };
                        this.preview.visible_grid(
                            &snapshot,
                            index,
                            range.start * columns..range.end * columns,
                        );
                        range
                            .map(|grid_row| {
                                let mut row = div().h(px(168.)).flex().gap_2().px_2();
                                for tile in grid_row * columns
                                    ..((grid_row + 1) * columns)
                                        .min(snapshot.panes[index].rows.len())
                                {
                                    row = row.child(this.file_row(index, tile, true, cx));
                                }
                                row.into_any_element()
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.scroll[index].clone())
                .flex_1()
                .min_h_0(),
            );
        } else {
            list_container = list_container.child(
                uniform_list(
                    ("files", index),
                    pane.rows.len(),
                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|row_index| this.file_row(index, row_index, false, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(self.scroll[index].clone())
                .flex_1()
                .min_h_0(),
            );
        }

        let mut interior = div().flex().flex_1().min_h_0().min_w_0();
        if pane.preview_mode == ira_core::model::PreviewMode::Column {
            let is_text = snapshot.pane_shows_text_preview[index];
            let list_flex = if is_text { 0.34 } else { 0.62 };
            let preview_flex = if is_text { 0.66 } else { 0.38 };

            interior = interior.child(
                div()
                    .flex_basis(gpui::relative(list_flex))
                    .h_full()
                    .min_w_0()
                    .child(list_container),
            );

            let mut col = div()
                .flex_basis(gpui::relative(preview_flex))
                .h_full()
                .min_w(px(220.))
                .p_3()
                .border_l_1()
                .border_color(chrome::to_gpui_color(self.theme.border))
                .flex()
                .flex_col()
                .gap_2()
                .bg(native_color(self.theme.surface_alt));

            let is_editing = snapshot.edit_focus
                && snapshot.active_pane == index
                && self.input_mode == Some(InputMode::Editor)
                && self.input.is_some();
            let is_read_only = snapshot
                .edit
                .as_ref()
                .is_some_and(|e| e.pane_index == index && e.read_only);
            let is_dirty = snapshot
                .edit
                .as_ref()
                .is_some_and(|e| e.pane_index == index && e.dirty);

            let col_title = if is_editing {
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .pb(px(4.))
                    .border_b_1()
                    .border_color(chrome::to_gpui_color(self.theme.border))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_size(px(11.))
                                    .text_color(chrome::to_gpui_color(if is_dirty {
                                        self.theme.warning
                                    } else {
                                        self.theme.accent
                                    }))
                                    .child(if is_dirty { "* Editing" } else { "Editing" }),
                            ),
                    )
                    .child(chrome::hint_row(
                        &[("Ctrl+S", "save"), ("Esc", "exit")],
                        &self.theme,
                    ))
            } else if is_read_only {
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .pb(px(4.))
                    .border_b_1()
                    .border_color(chrome::to_gpui_color(self.theme.border))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(chrome::to_gpui_color(self.theme.warning))
                            .child("Preview (read-only)"),
                    )
                    .child(chrome::hint_pill("v", "mode", &self.theme))
            } else {
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .pb(px(4.))
                    .border_b_1()
                    .border_color(chrome::to_gpui_color(self.theme.border))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(chrome::to_gpui_color(self.theme.text_muted))
                            .child("Preview"),
                    )
                    .child(chrome::hint_pill("v", "cycle", &self.theme))
            };
            col = col.child(col_title);

            if is_editing && let Some(input) = &self.input {
                col = col.child(
                    div()
                        .id(("preview-editor", index))
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(self.accessibility.measured(
                            div().w_full().child(input.clone()),
                            AxTarget::Text {
                                document: snapshot.document_generation,
                            },
                            None,
                            None,
                        )),
                );
            } else if let Some(key) = preview::Host::key(snapshot, index) {
                col = match self.preview.content(&key) {
                    Some(preview::Content::Image(image)) => col.child(
                        gpui::img(image.clone())
                            .w_full()
                            .max_h(px(500.))
                            .object_fit(gpui::ObjectFit::Contain),
                    ),
                    Some(preview::Content::Text(text)) => col.child(
                        div()
                            .id(("preview-text", index))
                            .overflow_y_scroll()
                            .flex_1()
                            .min_h_0()
                            .child(text.clone()),
                    ),
                    Some(preview::Content::Error(error)) => col.child(
                        div()
                            .text_color(native_color(self.theme.text_muted))
                            .child(error.clone()),
                    ),
                    None => col.child("Loading preview…"),
                };
            } else {
                let selected_is_dir = pane
                    .cursor
                    .and_then(|c| pane.rows.get(c))
                    .is_some_and(|r| r.entry.is_dir);
                let placeholder = if selected_is_dir {
                    "Folders have no image preview"
                } else {
                    "Select a file to preview"
                };
                col = col.child(
                    div()
                        .text_size(px(12.))
                        .text_color(chrome::to_gpui_color(self.theme.text_muted))
                        .child(placeholder),
                );
            }
            interior = interior.child(col);
        } else {
            interior = interior.child(list_container);
        }

        let mut body = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .rounded(px(4.))
            .border_1()
            .border_color(border_color)
            .bg(native_color(self.theme.surface));

        let header = div()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(border_color)
            .bg(native_color(self.theme.surface_alt))
            .child(title_elements);

        if index == snapshot.active_pane
            && self.input_mode == Some(InputMode::Search)
            && let Some(input) = &self.input
        {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px_3()
                    .py_1()
                    .bg(native_color(self.theme.surface_alt))
                    .border_b_1()
                    .border_color(border_color)
                    .child(chrome::key_pill("/", &self.theme))
                    .child(self.accessibility.measured(
                        div().flex_1().child(input.clone()),
                        AxTarget::Text {
                            document: snapshot.document_generation,
                        },
                        None,
                        None,
                    ))
                    .child(chrome::hint_pill("Esc", "cancel", &self.theme)),
            );
        } else {
            body = body.child(header);
        }
        body = body.child(interior);

        body = body.on_scroll_wheel(cx.listener(move |this, event: &ScrollWheelEvent, _, cx| {
            let delta = event.delta.pixel_delta(px(30.)).y;
            if delta == px(0.) {
                return;
            }
            if let Some(snapshot) = &this.snapshot {
                this.runtime.enqueue(
                    Command::Wheel {
                        pane: index,
                        listing_generation: snapshot.panes[index].listing_generation,
                        next: delta < px(0.),
                    },
                    None,
                );
                cx.stop_propagation();
                cx.notify();
            }
        }));

        self.accessibility
            .measured(body, AxTarget::Pane(index), Some(AxRole::List), None)
    }

    fn render_drives_row(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(snapshot) = &self.snapshot else {
            return div().into_any_element();
        };
        let Some(drives) = &snapshot.drives else {
            return div().into_any_element();
        };

        let theme = &self.theme;
        let mut occurrences = BTreeMap::new();
        let mut items = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .overflow_x_hidden()
            .flex_1()
            .min_w_0();

        for (i, drive) in drives.iter().enumerate() {
            let key = if drive.shortcut.is_ascii_alphanumeric() {
                drive.shortcut.to_string()
            } else {
                (i + 1).to_string()
            };
            let (glyph, _) = ira_core::theme::icons::icon_for(
                &ira_core::services::list_files::FEntry {
                    path: drive.path.clone(),
                    label: drive.label.clone(),
                    is_dir: true,
                    size: 0,
                    modified: None,
                },
                snapshot.icons,
            );
            let space_str = if let Some(space) = drive.space {
                match space.available {
                    Some(avail) => format!(
                        " {} free / {}",
                        ira_core::services::file_info::human(avail),
                        ira_core::services::file_info::human(space.total)
                    ),
                    None => format!(" {}", ira_core::services::file_info::human(space.total)),
                }
            } else {
                String::new()
            };

            let count = occurrences
                .entry((drive.path.clone(), drive.shortcut))
                .or_insert(0usize);
            let occurrence = *count;
            *count += 1;
            let ax_target = AxTarget::Place {
                kind: crate::platform::accessibility::model::PlaceKind::Drive,
                path: PathBuf::from(&drive.path),
                shortcut: drive.shortcut,
                occurrence,
            };
            let handle = self
                .place_focus
                .entry(ax_target.clone())
                .or_insert_with(|| cx.focus_handle())
                .clone();
            let path = drive.path.clone();
            let shortcut = drive.shortcut;

            let drive_element = div()
                .id(("drive-item", i))
                .track_focus(&handle)
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(5.))
                .py(px(2.))
                .rounded(px(4.))
                .hover(|s| s.bg(chrome::to_gpui_color(theme.selection)))
                .cursor_pointer()
                .child(chrome::key_pill(&key, theme))
                .child(
                    div()
                        .text_color(chrome::to_gpui_color(theme.dir))
                        .child(glyph),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(chrome::to_gpui_color(theme.text))
                        .child(drive.label.clone()),
                )
                .children((!space_str.is_empty()).then(|| {
                    div()
                        .text_size(px(11.))
                        .text_color(chrome::to_gpui_color(theme.text_muted))
                        .child(space_str)
                }))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.runtime.enqueue(
                        Command::PlaceExact {
                            kind: PlaceKind::Drive,
                            path: path.clone(),
                            shortcut,
                            occurrence,
                        },
                        None,
                    );
                    cx.notify();
                }));

            items = items.child(self.accessibility.measured(
                drive_element,
                ax_target,
                Some(AxRole::Button),
                Some(AxCapability::Activate),
            ));
        }

        let title_left = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_size(px(12.))
                    .text_color(chrome::to_gpui_color(theme.text))
                    .child("Drives"),
            )
            .child(chrome::key_pill("-", theme))
            .child(
                div()
                    .id("unmount-drive-btn")
                    .px(px(4.))
                    .py(px(1.))
                    .rounded(px(3.))
                    .hover(|s| s.bg(chrome::to_gpui_color(theme.selection)))
                    .text_size(px(11.))
                    .text_color(chrome::to_gpui_color(theme.text_muted))
                    .cursor_pointer()
                    .child("unmount the current drive")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.dispatch(KeyCode::Char('-'), cx);
                    })),
            );

        chrome::panel_block(title_left, None::<gpui::Div>, true, theme)
            .id("ira-drives-row")
            .debug_selector(|| "ira-drives-row".into())
            .w_full()
            .px(px(8.))
            .py(px(4.))
            .child(items)
            .into_any_element()
    }

    fn render_common_folders_row(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(snapshot) = &self.snapshot else {
            return div().into_any_element();
        };
        let Some(folders) = &snapshot.folders else {
            return div().into_any_element();
        };

        let theme = &self.theme;
        let mut occurrences = BTreeMap::new();
        let mut items = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .overflow_x_hidden()
            .flex_1()
            .min_w_0();

        for (i, folder) in folders.iter().enumerate() {
            let key = folder.shortcut.to_string();
            let (glyph, _) = ira_core::theme::icons::icon_for(
                &ira_core::services::list_files::FEntry {
                    path: folder.path.clone(),
                    label: folder.label.clone(),
                    is_dir: true,
                    size: 0,
                    modified: None,
                },
                snapshot.icons,
            );

            let count = occurrences
                .entry((folder.path.clone(), folder.shortcut))
                .or_insert(0usize);
            let occurrence = *count;
            *count += 1;
            let ax_target = AxTarget::Place {
                kind: crate::platform::accessibility::model::PlaceKind::Common,
                path: PathBuf::from(&folder.path),
                shortcut: folder.shortcut,
                occurrence,
            };
            let handle = self
                .place_focus
                .entry(ax_target.clone())
                .or_insert_with(|| cx.focus_handle())
                .clone();
            let path = folder.path.clone();
            let shortcut = folder.shortcut;

            let folder_element = div()
                .id(("common-folder", i))
                .track_focus(&handle)
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(5.))
                .py(px(2.))
                .rounded(px(4.))
                .hover(|s| s.bg(chrome::to_gpui_color(theme.selection)))
                .cursor_pointer()
                .child(chrome::key_pill(&key, theme))
                .child(
                    div()
                        .text_color(chrome::to_gpui_color(theme.dir))
                        .child(glyph),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(chrome::to_gpui_color(theme.text))
                        .child(folder.label.clone()),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.runtime.enqueue(
                        Command::PlaceExact {
                            kind: PlaceKind::Common,
                            path: path.clone(),
                            shortcut,
                            occurrence,
                        },
                        None,
                    );
                    cx.notify();
                }));

            items = items.child(self.accessibility.measured(
                folder_element,
                ax_target,
                Some(AxRole::Button),
                Some(AxCapability::Activate),
            ));
        }

        let title = div()
            .font_weight(gpui::FontWeight::BOLD)
            .text_size(px(12.))
            .text_color(chrome::to_gpui_color(theme.text))
            .child("Common folders");

        chrome::panel_block(title, None::<gpui::Div>, true, theme)
            .id("ira-common-folders-row")
            .debug_selector(|| "ira-common-folders-row".into())
            .w_full()
            .px(px(8.))
            .py(px(4.))
            .child(items)
            .into_any_element()
    }

    fn render_bookmarks_and_actions_row(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(snapshot) = &self.snapshot else {
            return div().into_any_element();
        };

        let theme = &self.theme;
        let mut occurrences = BTreeMap::new();
        let mut bookmark_items = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .overflow_x_hidden()
            .flex_1()
            .min_w_0();

        if let Some(bookmarks) = &snapshot.bookmarks
            && !bookmarks.is_empty()
        {
            for (i, bookmark) in bookmarks.iter().enumerate() {
                let key = bookmark.shortcut.to_string();
                let count = occurrences
                    .entry((bookmark.path.clone(), bookmark.shortcut))
                    .or_insert(0usize);
                let occurrence = *count;
                *count += 1;
                let ax_target = AxTarget::Place {
                    kind: crate::platform::accessibility::model::PlaceKind::Bookmark,
                    path: PathBuf::from(&bookmark.path),
                    shortcut: bookmark.shortcut,
                    occurrence,
                };
                let handle = self
                    .place_focus
                    .entry(ax_target.clone())
                    .or_insert_with(|| cx.focus_handle())
                    .clone();
                let path = bookmark.path.clone();
                let shortcut = bookmark.shortcut;

                let bm_element = div()
                    .id(("bookmark-item", i))
                    .track_focus(&handle)
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(5.))
                    .py(px(2.))
                    .rounded(px(4.))
                    .hover(|s| s.bg(chrome::to_gpui_color(theme.selection)))
                    .cursor_pointer()
                    .child(chrome::key_pill(&key, theme))
                    .child(
                        div()
                            .text_color(chrome::to_gpui_color(theme.accent))
                            .child("★"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(chrome::to_gpui_color(theme.text))
                            .child(bookmark.label.clone()),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.runtime.enqueue(
                            Command::PlaceExact {
                                kind: PlaceKind::Bookmark,
                                path: path.clone(),
                                shortcut,
                                occurrence,
                            },
                            None,
                        );
                        cx.notify();
                    }));

                bookmark_items = bookmark_items.child(self.accessibility.measured(
                    bm_element,
                    ax_target,
                    Some(AxRole::Button),
                    Some(AxCapability::Activate),
                ));
            }
        } else {
            bookmark_items = bookmark_items.child(
                div()
                    .text_size(px(11.))
                    .text_color(chrome::to_gpui_color(theme.text_muted))
                    .child("No bookmarks (press b to bookmark)"),
            );
        }

        let bm_title = div()
            .font_weight(gpui::FontWeight::BOLD)
            .text_size(px(12.))
            .text_color(chrome::to_gpui_color(theme.text))
            .child("Bookmarks");

        let bookmarks_panel = chrome::panel_block(bm_title, None::<gpui::Div>, true, theme)
            .flex_1()
            .min_w_0()
            .px(px(8.))
            .py(px(4.))
            .child(bookmark_items);

        let actions_title = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_size(px(12.))
                    .text_color(chrome::to_gpui_color(theme.text))
                    .child("Actions"),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(chrome::to_gpui_color(theme.text_muted))
                    .child(format!("· {}", snapshot.theme_preset.label())),
            );

        let action_chips = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(chrome::key_hint(
                "action-split",
                "+",
                "Split Pane",
                theme,
                cx.listener(|this, _: &ClickEvent, _, cx| this.dispatch(KeyCode::Char('+'), cx)),
            ))
            .child(chrome::key_hint(
                "action-copyboard",
                "`",
                "Copy Board",
                theme,
                cx.listener(|this, _: &ClickEvent, _, cx| this.dispatch(KeyCode::Char('`'), cx)),
            ))
            .child(chrome::key_hint(
                "action-terminal",
                "0",
                "Terminal",
                theme,
                cx.listener(|this, _: &ClickEvent, _, cx| this.dispatch(KeyCode::Char('0'), cx)),
            ))
            .child(chrome::key_hint(
                "action-theme",
                "\\",
                "Theme",
                theme,
                cx.listener(|this, _: &ClickEvent, _, cx| this.dispatch(KeyCode::Char('\\'), cx)),
            ));

        let actions_panel = chrome::panel_block(actions_title, None::<gpui::Div>, true, theme)
            .flex_shrink_0()
            .px(px(8.))
            .py(px(4.))
            .child(action_chips);

        div()
            .id("ira-bookmarks-and-actions-row")
            .debug_selector(|| "ira-bookmarks-and-actions-row".into())
            .flex()
            .w_full()
            .gap(px(8.))
            .child(bookmarks_panel)
            .child(actions_panel)
            .into_any_element()
    }

    fn contextual_hints(&self) -> Vec<(&'static str, &'static str)> {
        let Some(snapshot) = &self.snapshot else {
            return Vec::new();
        };
        if snapshot.hint_bar_blocked || snapshot.status.is_some() {
            return Vec::new();
        }
        if snapshot
            .panes
            .get(snapshot.active_pane)
            .is_some_and(|p| p.preview_mode == ira_core::model::PreviewMode::Grid)
        {
            vec![
                ("arrows", "navigate"),
                ("Enter", "open"),
                ("F2", "rename"),
                ("c", "copy"),
                ("m", "move"),
                (".", "hidden"),
                (",", "sort"),
                ("v", "preview"),
                ("b", "bookmark"),
                ("n", "new"),
                ("/", "search"),
            ]
        } else {
            vec![
                ("↑/↓", "move"),
                ("←/→", "open/leave"),
                ("c", "copy"),
                ("m", "move"),
                (".", "hidden"),
                (",", "sort"),
                ("v", "preview"),
                ("b", "bookmark"),
                ("n", "new"),
                ("/", "search"),
            ]
        }
    }

    fn render_bottom_bar(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = &self.theme;
        let Some(snapshot) = &self.snapshot else {
            return div().into_any_element();
        };

        let mut left_area = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .overflow_x_hidden()
            .flex_1()
            .min_w_0();

        if let Some(feedback) = &self.feedback {
            left_area = left_area
                .child(
                    div()
                        .text_color(chrome::to_gpui_color(theme.accent))
                        .text_size(px(13.))
                        .child("●"),
                )
                .child(
                    div()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_size(px(12.))
                        .text_color(chrome::to_gpui_color(theme.text))
                        .child(feedback.clone()),
                );
        } else if let Some(status) = &snapshot.status {
            let dot_color = if status.is_error {
                theme.error
            } else {
                theme.accent
            };
            left_area = left_area
                .child(
                    div()
                        .text_color(chrome::to_gpui_color(dot_color))
                        .text_size(px(13.))
                        .child("●"),
                )
                .child(
                    div()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_size(px(12.))
                        .text_color(chrome::to_gpui_color(theme.text))
                        .child(status.text.clone()),
                );
        } else if !snapshot.hint_bar_blocked {
            for (key, label) in self.contextual_hints() {
                left_area = left_area.child(chrome::hint_pill(key, label, theme));
            }
        }

        let mut bar = div()
            .id("ira-status-footer")
            .debug_selector(|| "ira-status-footer".into())
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .px(px(8.))
            .py(px(4.))
            .bg(native_color(theme.surface_alt))
            .border_1()
            .border_color(chrome::to_gpui_color(theme.border))
            .child(left_area);

        if !snapshot.hint_bar_blocked {
            let keybindings_btn = div()
                .id("keybindings-btn")
                .debug_selector(|| "keybindings-btn".into())
                .cursor_pointer()
                .flex_shrink_0()
                .child(chrome::key_pill("* Keybindings", theme))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.dispatch(KeyCode::Char('*'), cx);
                }));
            bar = bar.child(keybindings_btn);
        }

        self.accessibility.measured(
            bar,
            AxTarget::Window,
            Some(AxRole::Status),
            None,
        )
    }
    fn modal(&self, height: gpui::Pixels, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let snapshot = self.snapshot.as_ref()?;
        let mut modal = div()
            .id("ira-modal")
            .debug_selector(|| "ira-modal".into())
            .absolute()
            .top(px(90.))
            .left(px(220.))
            .right(px(90.))
            .p_5()
            .rounded_lg()
            .bg(native_color(self.theme.surface))
            .border_1()
            .border_color(native_color(self.theme.border_active))
            .flex()
            .flex_col()
            .gap_3();
        if let Some(input) = &self.input {
            if self.input_mode == Some(InputMode::Search)
                || (self.input_mode == Some(InputMode::Editor)
                    && snapshot
                        .panes
                        .get(snapshot.active_pane)
                        .is_some_and(|p| p.preview_mode == ira_core::model::PreviewMode::Column))
            {
                return None;
            }
            let label = match self.input_mode {
                Some(InputMode::Rename) => "Rename",
                Some(InputMode::Create) => "Create file or folder",
                Some(InputMode::Goto) => "Go to path",
                Some(InputMode::Editor) => "Text editor",
                _ => "Input",
            };
            modal = modal
                .child(chrome::dialog_header(
                    label,
                    chrome::DialogKind::Input,
                    &self.theme,
                ))
                .child(self.accessibility.measured(
                    div()
                        .p_2()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(chrome::to_gpui_color(self.theme.border_active))
                        .bg(native_color(self.theme.surface_alt))
                        .child(input.clone()),
                    AxTarget::Text {
                        document: snapshot.document_generation,
                    },
                    None,
                    None,
                ));
            if self.input_mode == Some(InputMode::Create) {
                modal = modal.child(
                    div()
                        .text_size(px(11.))
                        .text_color(chrome::to_gpui_color(self.theme.accent))
                        .child(rows::new_entry_kind_label(
                            snapshot.new_entry.as_ref().map_or("", |p| p.text.as_str()),
                        )),
                );
            }
            modal = modal.child(chrome::hint_row(
                &[("Enter", "confirm"), ("Esc", "cancel")],
                &self.theme,
            ));
            return Some(
                self.accessibility
                    .measured(modal, AxTarget::Modal, Some(AxRole::Dialog), None),
            );
        }
        if let Some(confirm) = &snapshot.confirming {
            let verb = match confirm.action {
                ira_core::model::ConfirmAction::Delete => "Delete",
                ira_core::model::ConfirmAction::Copy => "Copy",
                ira_core::model::ConfirmAction::Move => "Move",
            };
            let prompt = match &confirm.dest_dir {
                Some(dest) => {
                    let dest_name = dest
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| dest.display().to_string());
                    format!("{verb} {} → {dest_name}?", confirm.label)
                }
                None => format!("{verb} {}?", confirm.label),
            };
            let policy_btn_label = match confirm.policy {
                ira_core::services::transfer::OverwritePolicy::AutoRename => "o: auto-rename",
                ira_core::services::transfer::OverwritePolicy::Overwrite => "o: overwrite",
                ira_core::services::transfer::OverwritePolicy::SkipExisting => "o: skip",
            };

            let mut buttons = div()
                .flex()
                .items_center()
                .gap_3()
                .child(self.button("y: Yes", KeyCode::Char('y'), cx))
                .child(self.button("n: No", KeyCode::Char('n'), cx));

            if !matches!(confirm.action, ira_core::model::ConfirmAction::Delete) {
                buttons = buttons.child(self.button(
                    policy_btn_label,
                    KeyCode::Char('o'),
                    cx,
                ));
            }

            modal = modal
                .child(chrome::dialog_header(
                    "Confirm",
                    if matches!(confirm.action, ira_core::model::ConfirmAction::Delete) {
                        chrome::DialogKind::Danger
                    } else {
                        chrome::DialogKind::Confirm
                    },
                    &self.theme,
                ))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(chrome::to_gpui_color(self.theme.text))
                        .child(prompt),
                )
                .child(buttons);
        } else if let Some(status) = snapshot.status.as_ref().filter(|s| s.is_error) {
            modal = modal
                .child(chrome::dialog_header(
                    "Error",
                    chrome::DialogKind::Error,
                    &self.theme,
                ))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(chrome::to_gpui_color(self.theme.text))
                        .child(status.text.clone()),
                )
                .child(chrome::hint_row(&[("any key", "dismiss")], &self.theme));
        } else if let Some(info) = &snapshot.info {
            let mut list = div()
                .id("info-scroll")
                .max_h(px((f32::from(height) - 200.).max(60.)))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_1();
            for line in &info.lines {
                if let Some((label, value, is_partial)) = rows::split_info_line(line) {
                    list = list.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(12.))
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(chrome::to_gpui_color(self.theme.text_muted))
                                    .child(format!("{label}:")),
                            )
                            .child(
                                div()
                                    .text_color(chrome::to_gpui_color(if is_partial {
                                        self.theme.accent
                                    } else {
                                        self.theme.text
                                    }))
                                    .child(value.to_string()),
                            ),
                    );
                } else {
                    list = list.child(
                        div()
                            .text_size(px(12.))
                            .text_color(chrome::to_gpui_color(self.theme.text))
                            .child(line.clone()),
                    );
                }
            }
            if !info.lines.iter().any(|l| l.starts_with("Size:")) {
                let cached = snapshot
                    .sizes
                    .iter()
                    .find(|(p, _)| p == &info.path)
                    .map(|(_, s)| s);
                let spinner = ira_core::services::file_info::spinner_char(info.started);
                let dynamic_size = match (info.pending, cached) {
                    (true, Some(si)) => {
                        Some(ira_core::services::file_info::size_line_partial(si, spinner))
                    }
                    (true, None) => {
                        Some(ira_core::services::file_info::size_line_started(spinner))
                    }
                    (false, Some(si)) if !si.complete => {
                        Some(ira_core::services::file_info::size_line_cancelled(si))
                    }
                    (false, Some(si)) => {
                        Some(ira_core::services::file_info::size_line_final(si))
                    }
                    (false, None) => None,
                };
                if let Some(size_line) = dynamic_size {
                    list = list.child(
                        div()
                            .text_size(px(12.))
                            .text_color(chrome::to_gpui_color(self.theme.accent))
                            .child(size_line),
                    );
                }
            }
            modal = modal
                .child(chrome::dialog_header(
                    "Entry info",
                    chrome::DialogKind::Info,
                    &self.theme,
                ))
                .child(list)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(self.button("r: Recalculate", KeyCode::Char('r'), cx))
                        .child(self.button("x: Cancel walk", KeyCode::Char('x'), cx))
                        .child(self.button("Esc: Close", KeyCode::Esc, cx)),
                )
                .child(chrome::hint_row(&[("any key", "close")], &self.theme));
        } else if let Some(info) = &snapshot.multi_info {
            let (sel_line, size_line) = rows::format_multi_info(
                info.folders,
                info.files,
                info.aggregate,
                chrome::spinner_char(info.started),
            );
            modal = modal
                .child(chrome::dialog_header(
                    "Selection info",
                    chrome::DialogKind::Info,
                    &self.theme,
                ))
                .child(
                    div()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_size(px(13.))
                        .text_color(chrome::to_gpui_color(self.theme.text))
                        .child(sel_line),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(chrome::to_gpui_color(self.theme.text))
                        .child(size_line),
                )
                .child(chrome::hint_row(&[("any key", "close")], &self.theme));
        } else if let Some(deletion) = snapshot.deletion.as_ref().filter(|d| !d.hidden) {
            let frac = if deletion.total > 0 {
                deletion.done as f32 / deletion.total as f32
            } else {
                0.0
            };
            modal = modal
                .child(chrome::dialog_header(
                    "Deleting",
                    chrome::DialogKind::Progress,
                    &self.theme,
                ))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(chrome::to_gpui_color(self.theme.text))
                        .child(format!("Deleting {} / {}", deletion.done, deletion.total)),
                );
            if let Some(current) = &deletion.current {
                let name = current
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| current.display().to_string());
                modal = modal.child(
                    div()
                        .text_size(px(11.))
                        .text_color(chrome::to_gpui_color(self.theme.text_muted))
                        .child(name),
                );
            }
            modal = modal
                .child(chrome::progress_bar(
                    frac,
                    320.,
                    chrome::to_gpui_color(self.theme.info),
                    chrome::to_gpui_color(self.theme.border),
                ))
                .child(chrome::hint_row(
                    &[("any key", "hide (deletion continues)")],
                    &self.theme,
                ));
        } else if snapshot.keybindings_visible {
            let theme = &self.theme;
            let bind_rows: &[(&str, &str, &str, &str)] = &[
                (
                    "Arrows",
                    "navigate; ←/→ open/leave",
                    "z / x",
                    "top / bottom",
                ),
                ("Space", "multi-select entry", "c", "copy to other pane"),
                (
                    "m",
                    "move to other pane",
                    "Enter / F2",
                    "rename entry (open/rename in grid)",
                ),
                ("Del", "delete (with confirm)", "n", "new folder / file"),
                (".", "toggle hidden files", ",", "cycle sort mode"),
                ("v", "cycle image preview", "Tab", "switch pane"),
                ("Ctrl+A", "select / clear all", "Alt+I", "invert selection"),
                ("+", "split pane", "`", "copy board"),
                ("b", "bookmark this folder", "/", "fuzzy search"),
                ("Esc", "clear search filter", "?", "entry info"),
                ("[", "go to path", "]", "copy folder path"),
                (
                    "0 / -",
                    "terminal here / file browser",
                    "Ctrl+-",
                    "eject drive",
                ),
                ("*", "this help", "q", "quit"),
            ];

            let mut table = div().flex().flex_col().gap_1();
            for (k1, v1, k2, v2) in bind_rows {
                let row = div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .w(px(270.))
                            .child(chrome::key_pill(k1, theme))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(chrome::to_gpui_color(theme.text))
                                    .child(*v1),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .w(px(270.))
                            .child(chrome::key_pill(k2, theme))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(chrome::to_gpui_color(theme.text))
                                    .child(*v2),
                            ),
                    );
                table = table.child(row);
            }

            modal = modal
                .child(chrome::dialog_header(
                    "Keybindings",
                    chrome::DialogKind::Info,
                    theme,
                ))
                .child(table)
                .child(chrome::hint_row(&[("any key", "close")], theme));
        } else {
            return None;
        }
        Some(
            self.accessibility
                .measured(modal, AxTarget::Modal, Some(AxRole::Dialog), None),
        )
    }
}
impl Focusable for Desktop {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Render for Desktop {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let visible_panes = if self.snapshot.as_ref().is_some_and(|s| s.split) {
            2.0
        } else {
            1.0
        };
        let columns = ((f32::from(window.bounds().size.width) / visible_panes)
            / 180.)
            .floor()
            .max(1.) as usize;
        for index in 0..2 {
            if self.grid_columns[index] != columns {
                self.grid_columns[index] = columns;
                if let Some(pane) = self.snapshot.as_ref().map(|s| &s.panes[index])
                    && pane.preview_mode == ira_core::model::PreviewMode::Grid
                    && let Some(cursor) = pane.cursor
                {
                    self.scroll[index].scroll_to_item(cursor / columns, ScrollStrategy::Center);
                }
            }
        }
        self.sync_input(window, cx);
        self.accessibility.focused_target = self
            .place_focus
            .iter()
            .find(|(_, handle)| handle.is_focused(window))
            .map(|(target, _)| target.clone())
            .or_else(|| {
                self.input
                    .as_ref()
                    .filter(|input| input.focus_handle(cx).is_focused(window))
                    .map(|_| AxTarget::Text {
                        document: self.snapshot.as_ref().map_or(0, |s| s.document_generation),
                    })
            });
        self.accessibility_actions(window, cx);
        if let Some(snapshot) = &self.snapshot {
            let text = self.native_text(cx);
            self.accessibility
                .begin_prepared(snapshot.clone(), text, self.rendered_footer());
        }
        let mut panes = div().flex().flex_1().min_h_0().gap(px(8.));
        let p0_text = self.snapshot.as_ref().is_some_and(|s| s.pane_shows_text_preview[0]);
        let p1_text = self.snapshot.as_ref().is_some_and(|s| s.pane_shows_text_preview[1]);
        let is_split = self.snapshot.as_ref().is_some_and(|s| s.split);

        if is_split {
            if p0_text && !p1_text {
                panes = panes
                    .child(div().flex_basis(gpui::relative(0.66)).h_full().child(self.pane(0, cx)))
                    .child(div().flex_basis(gpui::relative(0.34)).h_full().child(self.pane(1, cx)));
            } else if p1_text && !p0_text {
                panes = panes
                    .child(div().flex_basis(gpui::relative(0.34)).h_full().child(self.pane(0, cx)))
                    .child(div().flex_basis(gpui::relative(0.66)).h_full().child(self.pane(1, cx)));
            } else {
                panes = panes
                    .child(div().flex_1().h_full().child(self.pane(0, cx)))
                    .child(div().flex_1().h_full().child(self.pane(1, cx)));
            }
        } else {
            panes = panes.child(div().flex_1().h_full().child(self.pane(0, cx)));
        }

        let mut files_area = div().flex().flex_1().min_h_0().min_w_0();
        files_area = files_area.child(panes);

        if let Some(snapshot) = &self.snapshot
            && snapshot.copy_board
        {
            let mut board = div()
                .id("ira-copy-board")
                .debug_selector(|| "ira-copy-board".into())
                .w(px(360.))
                .max_h(px(240.))
                .flex_shrink_0()
                .overflow_y_scroll()
                .p_3()
                .border_1()
                .border_color(chrome::to_gpui_color(if snapshot.board_focused {
                    self.theme.border_active
                } else {
                    self.theme.border
                }))
                .bg(native_color(self.theme.surface_alt))
                .child(chrome::dialog_header(
                    "Copy Board",
                    chrome::DialogKind::Info,
                    &self.theme,
                ));

            if snapshot.jobs.is_empty() {
                board = board.child(
                    div()
                        .p_2()
                        .text_sm()
                        .text_color(chrome::to_gpui_color(self.theme.text_muted))
                        .child("No copy/move jobs yet (press c to copy)"),
                );
            }

            for (index, job) in snapshot.jobs.iter().enumerate() {
                let id = job.id;
                let (status_icon, status_color, lifecycle_str) = match &job.status {
                    ira_core::services::transfer::JobStatus::Running => {
                        ("▶", self.theme.accent, "Running")
                    }
                    ira_core::services::transfer::JobStatus::Paused => {
                        ("⏸", self.theme.warning, "Paused")
                    }
                    ira_core::services::transfer::JobStatus::Done => {
                        ("✓", self.theme.success, "Done")
                    }
                    ira_core::services::transfer::JobStatus::Failed(_) => {
                        ("!", self.theme.error, "Failed")
                    }
                    ira_core::services::transfer::JobStatus::Cancelled => {
                        ("✕", self.theme.text_muted, "Cancelled")
                    }
                    ira_core::services::transfer::JobStatus::Queued => {
                        ("·", self.theme.text_muted, "Queued")
                    }
                };
                let kind_str = match job.kind {
                    ira_core::services::transfer::JobKind::Copy => "COPY",
                    ira_core::services::transfer::JobKind::Move => "MOVE",
                };
                let (fraction, pct, bytes_text) =
                    rows::format_transfer_bytes(job.copied_bytes, job.total_bytes);
                let mut row = div()
                    .id(("job", index))
                    .p_2()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .items_start()
                    .w_full()
                    .min_w_0()
                    .bg(native_color(
                        if snapshot.board_focused && snapshot.copy_board_cursor == Some(index) {
                            self.theme.cursor_bg
                        } else {
                            self.theme.surface_alt
                        },
                    ))
                    .child(format!(
                        "[{status_icon}] {kind_str} · {} · {lifecycle_str} · {pct}% ({bytes_text})",
                        job.label
                    ))
                    .child(chrome::progress_bar(
                        fraction,
                        310.,
                        chrome::to_gpui_color(status_color),
                        chrome::to_gpui_color(self.theme.border),
                    ))
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        window.focus(&this.focus);
                        this.runtime.enqueue(
                            Command::Job {
                                id,
                                verb: JobVerb::Focus,
                            },
                            None,
                        );
                        cx.notify();
                    }));
                if let ira_core::services::transfer::JobStatus::Failed(error) = &job.status {
                    row = row.child(
                        div()
                            .w_full()
                            .whitespace_normal()
                            .text_sm()
                            .text_color(chrome::to_gpui_color(self.theme.error))
                            .child(error.clone()),
                    );
                }
                for (label, verb) in [
                    ("Pause / Resume", JobVerb::Pause),
                    ("Cancel", JobVerb::Cancel),
                ] {
                    row = row.child(
                        self.accessibility.measured(
                            div()
                                .id((label, index))
                                .px_2()
                                .py(px(1.))
                                .rounded(px(3.))
                                .border_1()
                                .border_color(chrome::to_gpui_color(self.theme.border))
                                .bg(native_color(self.theme.surface))
                                .hover(|s| s.bg(chrome::to_gpui_color(self.theme.selection)))
                                .text_size(px(11.))
                                .cursor_pointer()
                                .child(label)
                                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                                    window.focus(&this.focus);
                                    this.runtime.enqueue(Command::Job { id, verb }, None);
                                    cx.stop_propagation();
                                    cx.notify();
                                })),
                            AxTarget::Job(id),
                            Some(AxRole::Button),
                            Some(if matches!(verb, JobVerb::Pause) {
                                AxCapability::Pause
                            } else {
                                AxCapability::Cancel
                            }),
                        ),
                    );
                }
                board = board.child(self.accessibility.measured(
                    row,
                    AxTarget::Job(id),
                    Some(AxRole::Row),
                    None,
                ));
            }

            let footer_hints: &[(&str, &str)] = if snapshot.board_focused {
                &[("p/Space", "pause"), ("x", "cancel"), ("Esc", "close")]
            } else {
                &[("Tab", "focus board")]
            };
            board = board.child(
                div()
                    .mt_auto()
                    .pt_2()
                    .child(chrome::hint_row(footer_hints, &self.theme)),
            );

            files_area = files_area.child(board);
        }

        let mut content = div()
            .id("ira-files-area")
            .debug_selector(|| "ira-files-area".into())
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0();
        content = content.child(files_area);

        let drives_row = self.render_drives_row(cx);
        let common_row = self.render_common_folders_row(cx);
        let bookmarks_and_actions_row = self.render_bookmarks_and_actions_row(cx);
        self.place_focus.retain(|target, _| {
            self.accessibility
                .id(target, Some(AxRole::Button), Some(AxCapability::Focus))
                .is_some()
        });
        let footer_row = self.render_bottom_bar(cx);

        let mut root = div()
            .id("ira-root")
            .key_context("IraDesktop")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .p(px(6.))
            .gap(px(6.))
            .bg(native_color(self.theme.bg))
            .text_color(native_color(self.theme.text))
            .on_key_down(cx.listener(Self::key))
            .on_action(cx.listener(|_, _: &actions::Quit, _, cx| {
                crate::lifecycle_trace("menu/keybinding Quit action");
                cx.propagate();
            }))
            .on_action(
                cx.listener(|this, _: &actions::Rename, _, cx| this.dispatch(KeyCode::Enter, cx)),
            )
            .on_action(
                cx.listener(|this, _: &actions::Open, _, cx| this.dispatch(KeyCode::Right, cx)),
            )
            .on_action(
                cx.listener(|this, _: &actions::Parent, _, cx| this.dispatch(KeyCode::Left, cx)),
            )
            .on_action(
                cx.listener(|this, _: &actions::Search, _, cx| {
                    this.dispatch(KeyCode::Char('/'), cx)
                }),
            )
            .on_action(cx.listener(|this, _: &actions::NewEntry, _, cx| {
                this.dispatch(KeyCode::Char('n'), cx)
            }))
            .on_action(
                cx.listener(|this, _: &actions::Copy, _, cx| this.dispatch(KeyCode::Char('c'), cx)),
            )
            .on_action(
                cx.listener(|this, _: &actions::Move, _, cx| this.dispatch(KeyCode::Char('m'), cx)),
            )
            .on_action(
                cx.listener(|this, _: &actions::Delete, _, cx| this.dispatch(KeyCode::Delete, cx)),
            )
            .on_action(cx.listener(|this, _: &actions::ToggleSplit, _, cx| {
                this.dispatch(KeyCode::Char('+'), cx)
            }))
            .on_action(cx.listener(|this, _: &actions::ToggleBoard, _, cx| {
                this.dispatch(KeyCode::Char('`'), cx)
            }))
            .on_action(
                cx.listener(|this, _: &actions::Sort, _, cx| this.dispatch(KeyCode::Char(','), cx)),
            )
            .on_action(
                cx.listener(|this, _: &actions::Hidden, _, cx| {
                    this.dispatch(KeyCode::Char('.'), cx)
                }),
            )
            .on_action(cx.listener(|this, _: &actions::Bookmark, _, cx| {
                this.dispatch(KeyCode::Char('b'), cx)
            }))
            .on_action(
                cx.listener(|this, _: &actions::Info, _, cx| this.dispatch(KeyCode::Char('?'), cx)),
            )
            .on_action(
                cx.listener(|this, _: &actions::Theme, _, cx| {
                    this.dispatch(KeyCode::Char('\\'), cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &actions::FocusNext, _, cx| this.dispatch(KeyCode::Tab, cx)),
            )
            .child(drives_row)
            .child(common_row)
            .child(bookmarks_and_actions_row)
            .child(content)
            .child(footer_row);
        if let Some(family) = &self.font_family {
            root = root.font_family(family.clone());
        }
        if let Some(modal) = self.modal(window.bounds().size.height, cx) {
            root = root.child(modal);
        }
        let entity = cx.entity().downgrade();
        let expected_frame = self.accessibility.frame.clone();
        let root = self
            .accessibility
            .measured(root, AxTarget::Window, Some(AxRole::Window), None);
        div().relative().size_full().child(root).child(
            gpui::canvas(
                |_, _, _| {},
                move |_, _, window, cx| {
                    let _ = entity.update(cx, |this, cx| {
                        if !std::rc::Rc::ptr_eq(&expected_frame, &this.accessibility.frame) {
                            return;
                        }
                        let text = this.native_text(cx);
                        if let Some(snapshot) = &this.snapshot
                            && let Err(error) =
                                this.accessibility.publish(snapshot, text.as_ref(), window)
                        {
                            this.feedback = Some(error);
                        }
                    });
                },
            )
            .absolute()
            .inset_0(),
        )
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        crate::lifecycle_trace("Desktop entity dropped");
        self.close();
    }
}

/// The UI-neutral palette owns quantization and named-color resolution.
pub fn native_color(color: ira_core::theme::Color) -> gpui::Rgba {
    let c = color.rgba(ira_core::theme::Rgba {
        r: 30,
        g: 30,
        b: 46,
        a: 255,
    });
    gpui::rgba(
        (u32::from(c.r) << 24) | (u32::from(c.g) << 16) | (u32::from(c.b) << 8) | u32::from(c.a),
    )
}

#[cfg(test)]
mod crossing_tests {
    use super::*;
    use crate::platform::accessibility::{
        AccessibilityIntent,
        model::{Action, Role, Target},
    };
    use gpui::TestAppContext;
    use ira_core::{application::App, domain::data::Folder, services::list_files::list_files};
    #[gpui::test]
    fn recovery_board_survives_status_expiry_and_later_transfer_progress(cx: &mut TestAppContext) {
        let _fixture_scope = crate::test_support::enter();
        use ira_core::{
            model::{STATUS_TTL, Status},
            services::{
                list_files::FEntry,
                transfer::{Job, JobKind, JobStatus, OverwritePolicy},
            },
        };
        let fixture =
            std::env::temp_dir().join(format!("ira-native-recovery-{}", std::process::id()));
        std::fs::create_dir_all(fixture.join("dest")).unwrap();
        let source = fixture.join("source.txt");
        std::fs::write(&source, vec![b'x'; 16384]).unwrap();
        let recovery = format!(
            "destination WAS published at /fixture/target; retained recovery paths: /fixture/{}; remaining batch items were not processed",
            "nested/".repeat(100)
        );
        let expected = recovery.clone();
        let mut app = App::default();
        app.window_generation = 7;
        app.copy_board = true;
        app.split = true;
        app.panes[0].folder = Some(Folder::new(
            "Source".into(),
            fixture.to_string_lossy().into_owned(),
            '#',
        ));
        app.panes[1].folder = Some(Folder::new(
            "Destination".into(),
            fixture.join("dest").to_string_lossy().into_owned(),
            'd',
        ));
        app.panes[0].files = vec![FEntry {
            path: source.to_string_lossy().into_owned(),
            label: "source.txt".into(),
            is_dir: false,
            size: 16384,
            modified: None,
        }];
        app.panes[0].selected = vec![false];
        app.panes[0].state.select(Some(0));
        app.panes[0].listing_settled = true;
        app.jobs.push(Job {
            id: 40,
            kind: JobKind::Move,
            overwrite: OverwritePolicy::Overwrite,
            paths: vec!["/fixture/prior-source".into()],
            dest_dir: "/fixture/target".into(),
            label: "Earlier committed recovery".into(),
            total_bytes: Some(10),
            copied_bytes: 10,
            current: String::new(),
            status: JobStatus::Failed(recovery),
            started_at: std::time::Instant::now(),
            control: JobControl::new(),
        });
        let runtime = Runtime::with_factory(7, move || {
            app.status = Some(Status {
                text: "Transient notice".into(),
                is_error: false,
                raised: std::time::Instant::now() - STATUS_TTL + Duration::from_millis(200),
            });
            app
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let publication = loop {
            if let Some(p) = runtime.try_snapshot() {
                break p;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let mut view = Desktop::new(runtime, crate::platform::geometry::Writer::new(None), cx);
            view.accessibility = accessibility::Host::compatibility_headless();
            view.snapshot = Some(publication.snapshot);
            view
        });
        loop {
            let expired = view.update_in(cx, |view, _, cx| {
                view.poll(cx);
                view.snapshot.as_ref().unwrap().status.is_none()
            });
            if expired {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        view.update_in(cx, |view, window, cx| {
            window.focus(&view.focus);
            view.dispatch(KeyCode::Char('c'), cx);
        });
        loop {
            let confirming = view.update_in(cx, |view, _, cx| {
                view.poll(cx);
                view.snapshot.as_ref().unwrap().confirming.is_some()
            });
            if confirming {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        view.update_in(cx, |view, _, cx| view.dispatch(KeyCode::Enter, cx));
        loop {
            let done = view.update_in(cx, |view, _, cx| {
                view.poll(cx);
                view.snapshot.as_ref().unwrap().jobs.iter().any(|job| {
                    job.id != 40 && job.status == JobStatus::Done && job.copied_bytes > 0
                })
            });
            if done {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        view.update_in(cx, |view, _, _| {
            let job = view
                .snapshot
                .as_ref()
                .unwrap()
                .jobs
                .iter()
                .find(|j| j.id == 40)
                .unwrap();
            assert_eq!(job.status, JobStatus::Failed(expected.clone()));
            let tree = view.accessibility.tree.as_ref().unwrap();
            let node = tree
                .nodes
                .values()
                .find(|n| n.target == Target::Job(40) && n.role == Role::Row)
                .unwrap();
            let geometry = node.geometry.unwrap();
            assert!(
                geometry.bounds.height > 90.,
                "recovery error must wrap beyond one line"
            );
            assert!(geometry.visible.valid());
            assert!(node.name.contains("retained recovery paths"));
            view.runtime.stop(&view.controls);
            view.close();
        });
        let footer = cx
            .debug_bounds("ira-status-footer")
            .expect("actual footer rendered even without transient Status semantic node");
        let board = cx
            .debug_bounds("ira-copy-board")
            .expect("actual recovery board");
        assert!(
            board.size.height <= px(240.),
            "recovery board must remain bounded and scrollable"
        );
        let window_height = view.update_in(cx, |_, window, _| window.bounds().size.height);
        assert!(
            footer.origin.y >= px(0.) && footer.bottom() <= window_height,
            "footer must remain reachable below recovery board"
        );
        assert_eq!(
            std::fs::read(fixture.join("dest/source.txt"))
                .unwrap()
                .len(),
            16384
        );
        // Retain the task fixture until checked write receipts and worker retirement
        // prove cleanup safe; the legacy processing barrier is insufficient.
    }
    #[gpui::test]
    fn grid_viewport_mouse_and_mode_changes_keep_path_targets(cx: &mut TestAppContext) {
        let _fixture_scope = crate::test_support::enter();
        use ira_core::{model::PreviewMode, services::list_files::FEntry};
        let mut app = App::default();
        app.window_generation = 7;
        app.panes[0].preview_mode = PreviewMode::Grid;
        app.panes[0].folder = Some(Folder::new(
            "Grid fixture".into(),
            "/tmp/ira-grid-headless".into(),
            '#',
        ));
        app.panes[0].files = (0..200)
            .map(|i| FEntry {
                path: format!("/tmp/ira-grid-headless/file-{i:03}.txt"),
                label: format!("file-{i:03}.txt"),
                is_dir: false,
                size: 20,
                modified: None,
            })
            .collect();
        app.panes[0].selected = vec![false; 200];
        app.panes[0].listing_settled = true;
        app.panes[0].listing_generation = 3;
        app.panes[0].state.select(Some(0));
        let runtime = Runtime::with_factory(7, move || app);
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        let publication = loop {
            if let Some(p) = runtime.try_snapshot() {
                break p;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let mut view = Desktop::new(runtime, crate::platform::geometry::Writer::new(None), cx);
            view.accessibility = accessibility::Host::compatibility_headless();
            view.snapshot = Some(publication.snapshot);
            view
        });
        let (id,point)=view.update_in(cx,|view,window,_|{
            window.focus(&view.focus);
            let tree=view.accessibility.tree.as_ref().unwrap();let rows:Vec<_>=tree.nodes.values().filter(|n|n.role==Role::Row).collect();assert_eq!(rows.len(),200);
            assert!(rows.iter().filter(|n|n.geometry.is_some()).count()<200);
            let first=rows.iter().find(|n|matches!(&n.target,Target::Entry{path,..} if path.ends_with("file-000.txt"))).unwrap();
            let geometry=first.geometry.unwrap();assert_eq!(geometry.bounds.height,160.);assert!(view.grid_columns[0]>1);
            (first.id,gpui::point(px((geometry.visible.x+geometry.visible.width/2.)as f32),px((geometry.visible.y+geometry.visible.height/2.)as f32)))
        });
        cx.simulate_click(
            point,
            gpui::Modifiers {
                control: true,
                ..Default::default()
            },
        );
        loop {
            let ready = view.update_in(cx, |view, _, cx| {
                view.poll(cx);
                view.snapshot.as_ref().unwrap().panes[0].rows[0].selected
            });
            if ready {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        cx.simulate_keystrokes("v");
        loop {
            let ready = view.update_in(cx, |view, _, cx| {
                view.poll(cx);
                view.snapshot.as_ref().unwrap().panes[0].preview_mode == PreviewMode::Off
            });
            if ready {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        view.update_in(cx, |view, _, _| {
            let node = &view.accessibility.tree.as_ref().unwrap().nodes[&id];
            assert_eq!(node.geometry.unwrap().bounds.height, 30.);
            assert!(matches!(&node.target,Target::Entry{path,..}if path.ends_with("file-000.txt")));
            assert!(view.snapshot.as_ref().unwrap().panes[0].rows[0].selected);
        });
        cx.simulate_keystrokes("v");
        loop {
            let ready = view.update_in(cx, |view, _, cx| {
                view.poll(cx);
                view.snapshot.as_ref().unwrap().panes[0].preview_mode == PreviewMode::Details
            });
            if ready {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        view.update_in(cx, |view, _, _| {
            assert_eq!(
                view.accessibility.tree.as_ref().unwrap().nodes[&id]
                    .geometry
                    .unwrap()
                    .bounds
                    .height,
                30.
            );
            view.runtime.stop(&view.controls);
            view.close();
        });
    }
    #[gpui::test]
    fn duplicate_place_focus_uses_real_handle_without_navigation(cx: &mut TestAppContext) {
        let _fixture_scope = crate::test_support::enter();
        let mut app = App::default();
        app.window_generation = 7;
        app.bookmarks = Some(vec![
            Folder::new("First".into(), "/tmp".into(), 'b'),
            Folder::new("Duplicate".into(), "/tmp".into(), 'b'),
        ]);
        let runtime = Runtime::with_factory(7, move || app);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let publication = loop {
            if let Some(p) = runtime.try_snapshot() {
                break p;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let mut view = Desktop::new(runtime, crate::platform::geometry::Writer::new(None), cx);
            view.accessibility = accessibility::Host::compatibility_headless();
            view.snapshot = Some(publication.snapshot);
            view
        });
        view.update_in(cx, |view, window, cx| {
            let tree = view.accessibility.tree.as_ref().unwrap();
            let nodes: Vec<_> = tree
                .nodes
                .values()
                .filter(|n| {
                    matches!(
                        &n.target,
                        Target::Place {
                            kind: crate::platform::accessibility::model::PlaceKind::Bookmark,
                            ..
                        }
                    )
                })
                .collect();
            assert_eq!(nodes.len(), 2);
            assert_ne!(nodes[0].id, nodes[1].id);
            assert!(nodes.iter().all(|n| n.geometry.is_some()));
            let node = nodes
                .iter()
                .find(|n| matches!(n.target, Target::Place { occurrence: 1, .. }))
                .unwrap();
            let target = node.target.clone();
            let id = node.id;
            let revision = view.snapshot.as_ref().unwrap().revision;
            view.accessibility
                .sink_for_test()
                .try_dispatch(AccessibilityIntent {
                    node: id,
                    stamp: tree.stamp,
                    action: Action::Focus,
                })
                .unwrap();
            view.poll(cx);
            view.accessibility_actions(window, cx);
            assert!(view.place_focus.get(&target).unwrap().is_focused(window));
            assert!(!view.focus.is_focused(window));
            assert_eq!(
                view.snapshot.as_ref().unwrap().revision,
                revision,
                "focus must not navigate or mutate actor state"
            );
            view.accessibility.focused_target = Some(target);
            let snapshot = view.snapshot.as_ref().unwrap();
            view.accessibility.begin(snapshot, None);
            assert_eq!(view.accessibility.tree.as_ref().unwrap().focused, Some(id));
            window.focus(&view.focus);
            view.accessibility.focused_target = None;
            view.accessibility.begin(snapshot, None);
            assert_ne!(view.accessibility.tree.as_ref().unwrap().focused, Some(id));
            view.runtime.stop(&view.controls);
            view.close();
        });
    }
    #[gpui::test]
    fn real_virtualized_desktop_frame_records_clipped_path_geometry(cx: &mut TestAppContext) {
        let _fixture_scope = crate::test_support::enter();
        let fixture = std::env::temp_dir().join(format!("ira-ax-frame-{}", std::process::id()));
        std::fs::create_dir_all(&fixture).unwrap();
        for index in 0..200 {
            std::fs::write(fixture.join(format!("file-{index:03}.txt")), b"fixture").unwrap();
        }
        let mut app = App::default();
        app.window_generation = 7;
        app.panes[0].folder = Some(Folder::new(
            "Fixture".into(),
            fixture.to_string_lossy().into_owned(),
            '#',
        ));
        app.panes[0].files = list_files(fixture.to_str().unwrap()).unwrap();
        app.panes[0].selected = vec![false; 200];
        app.panes[0].listing_settled = true;
        app.panes[0].listing_generation = 3;
        app.panes[0].state.select(Some(0));
        let runtime = Runtime::with_factory(7, move || app);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let publication = loop {
            if let Some(p) = runtime.try_snapshot() {
                break p;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let mut view = Desktop::new(runtime, crate::platform::geometry::Writer::new(None), cx);
            view.accessibility = accessibility::Host::compatibility_headless();
            view.snapshot = Some(publication.snapshot);
            view
        });
        view.update_in(cx,|view,_,cx|{
            let tree=view.accessibility.tree.as_ref().expect("actual root paint published frame");
            let rows:Vec<_>=tree.nodes.values().filter(|n|n.role==Role::Row).collect();
            assert_eq!(rows.len(),200);
            let materialized:Vec<_>=rows.iter().filter_map(|n|n.geometry).collect();
            assert!(!materialized.is_empty());assert!(materialized.len()<200);
            for geometry in materialized {assert!(geometry.visible.valid());assert_eq!(geometry.bounds.intersection(geometry.visible),Some(geometry.visible));}
            let first=rows.iter().find(|n|matches!(&n.target,Target::Entry{path,..} if path.ends_with("file-000.txt"))).unwrap();
            let bounds=first.geometry.expect("first actual row painted").visible;
            assert_eq!(tree.hit_test(bounds.x+bounds.width/2.,bounds.y+bounds.height/2.),Some(first.id));
            let intent=AccessibilityIntent{node:first.id,stamp:tree.stamp,action:Action::SetSelected(true)};
            view.accessibility.sink_for_test().try_dispatch(intent).unwrap();
            view.poll(cx);
            assert_eq!(view.accessibility_actions.len(),1);
            view.runtime.stop(&view.controls);view.close();
        });
        // Retain the task fixture until checked write receipts and worker retirement
        // prove cleanup safe; the legacy processing barrier is insufficient.
    }
    #[gpui::test]
    fn measured_help_modal_preserves_absolute_overlay_and_visible_ax_geometry(
        cx: &mut TestAppContext,
    ) {
        let _fixture_scope = crate::test_support::enter();
        let mut app = App::default();
        app.window_generation = 7;
        app.show_keybindings();
        let runtime = Runtime::with_factory(7, move || app);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let publication = loop {
            if let Some(value) = runtime.try_snapshot() {
                break value;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let mut view = Desktop::new(runtime, crate::platform::geometry::Writer::new(None), cx);
            view.accessibility = accessibility::Host::compatibility_headless();
            view.snapshot = Some(publication.snapshot);
            view
        });
        let bounds = cx
            .debug_bounds("ira-modal")
            .expect("actual common modal painted");
        assert_eq!(
            bounds.origin.y,
            px(90.),
            "measurement must preserve caller absolute top"
        );
        assert_eq!(
            bounds.origin.x,
            px(220.),
            "measurement must preserve caller absolute left"
        );
        view.update_in(cx, |view, window, _| {
            assert!(
                bounds.bottom() <= window.bounds().size.height,
                "modal must remain on-screen"
            );
            let tree = view.accessibility.tree.as_ref().unwrap();
            let modal = tree
                .nodes
                .values()
                .find(|n| n.role == AxRole::Dialog && n.target == AxTarget::Modal)
                .unwrap();
            assert!(
                modal.geometry.is_some(),
                "actual AX dialog must have visible clipped bounds"
            );
            view.runtime.stop(&view.controls);
            view.close();
        });
    }

    #[gpui::test]
    fn pending_editor_does_not_create_empty_native_input_or_advance_ticket(
        cx: &mut TestAppContext,
    ) {
        let _fixture_scope = crate::test_support::enter();
        cx.update(text_input::register);
        let mut app = App::default();
        app.window_generation = 7;
        app.panes[0].files = vec![ira_core::services::list_files::FEntry {
            path: "/task-fixture/pending.txt".into(),
            label: "pending.txt".into(),
            is_dir: false,
            size: 1,
            modified: None,
        }];
        app.panes[0].selected = vec![false];
        app.panes[0].state.select(Some(0));
        app.panes[0].preview_mode = ira_core::model::PreviewMode::Column;
        crate::runtime::apply(
            &mut app,
            crate::runtime::Envelope {
                sequence: 1,
                window_generation: 7,
                input_generation: None,
                command: Command::Input(actions::input(KeyCode::Tab)),
            },
        )
        .unwrap();
        let snapshot = Arc::new(app.snapshot());
        assert_eq!(
            snapshot.input_context,
            ira_core::input::InputContext::Editor
        );
        assert!(snapshot.edit.is_none());
        let (release, blocked) = mpsc::channel();
        let runtime = Runtime::with_factory(7, move || {
            let _ = blocked.recv();
            app
        });
        let (view, cx) = cx.add_window_view(move |window, cx| {
            let mut view = Desktop::new(runtime, crate::platform::geometry::Writer::new(None), cx);
            view.accessibility = accessibility::Host::compatibility_headless();
            view.snapshot = Some(snapshot);
            view.focus_main(window, cx);
            view
        });
        view.update_in(cx, |view, window, cx| {
            view.sync_input(window, cx);
            assert!(
                view.input.is_none(),
                "pending editor must not expose an empty editable draft"
            );
            assert_eq!(
                view.input_generation, 0,
                "pending focus ticket must remain actor-owned"
            );
            view.runtime.stop(&view.controls);
            view.close();
        });
        release.send(()).unwrap();
    }

    #[gpui::test]
    fn column_text_tab_reaches_native_editor_and_escape_returns_to_pane(cx: &mut TestAppContext) {
        let _fixture_scope = crate::test_support::enter();
        cx.update(text_input::register);
        let fixture = std::env::temp_dir().join(format!("ira-tab-editor-{}", std::process::id()));
        std::fs::create_dir_all(&fixture).unwrap();
        std::fs::write(
            fixture.join("fixture.txt"),
            "文😀e\u{301}\nfixture".as_bytes(),
        )
        .unwrap();
        let mut app = App::default();
        app.window_generation = 7;
        app.state_path = Some(fixture.join("state"));
        app.panes[0].folder = Some(Folder::new(
            "Fixture".into(),
            fixture.to_string_lossy().into_owned(),
            '#',
        ));
        app.panes[0].files = list_files(fixture.to_str().unwrap()).unwrap();
        app.panes[0].selected = vec![false];
        app.panes[0].listing_settled = true;
        app.panes[0].listing_generation = 3;
        app.panes[0].preview_mode = ira_core::model::PreviewMode::Column;
        app.panes[0].state.select(Some(0));
        assert!(app.pane_shows_text_preview(0));
        let runtime = Runtime::with_factory(7, move || app);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let publication = loop {
            if let Some(value) = runtime.try_snapshot() {
                break value;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        let (view, cx) = cx.add_window_view(move |window, cx| {
            let mut view = Desktop::new(runtime, crate::platform::geometry::Writer::new(None), cx);
            view.accessibility = accessibility::Host::headless();
            view.snapshot = Some(publication.snapshot);
            view.focus_main(window, cx);
            view
        });
        cx.simulate_keystrokes("tab");
        loop {
            let ready = view.update_in(cx, |view, _, cx| {
                view.poll(cx);
                view.input_mode == Some(InputMode::Editor)
                    && view.input.is_some()
                    && view.snapshot.as_ref().is_some_and(|s| {
                        s.edit.is_some() && s.focus_generation == view.input_generation
                    })
            });
            if ready {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Tab did not reach native editor"
            );
            std::thread::yield_now();
        }
        loop {
            let ready = view.update_in(cx, |view, _, cx| {
                view.poll(cx);
                view.accessibility
                    .prepared_frame_for_test()
                    .is_some_and(|f| {
                        f.publication_seq > 0
                            && Some(f.key.request) == view.accessibility.authoritative_key()
                            && view
                                .accessibility
                                .id(
                                    &Target::Text {
                                        document: view
                                            .snapshot
                                            .as_ref()
                                            .unwrap()
                                            .document_generation,
                                    },
                                    None,
                                    Some(AxCapability::Value),
                                )
                                .is_some()
                    })
            });
            if ready {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        view.update_in(cx, |view, _, cx| {
            let native = view.native_text(cx).unwrap();
            assert_eq!(native.text.as_ref(), "文😀e\u{301}\nfixture");
            assert!(native.multiline);
            let tree = view.accessibility.tree.as_ref().unwrap();
            assert!(
                tree.nodes
                    .values()
                    .any(|n| matches!(n.target, Target::Text { .. })
                        && n.capabilities.contains(&AxCapability::Value))
            );
        });
        let before = view.update_in(cx, |view, _, _| {
            view.accessibility.authoritative_key().unwrap()
        });
        view.update_in(cx, |view, window, cx| {
            let tree = view.accessibility.tree.as_ref().unwrap();
            let id = view
                .accessibility
                .id(
                    &Target::Text {
                        document: view.snapshot.as_ref().unwrap().document_generation,
                    },
                    None,
                    Some(AxCapability::Value),
                )
                .unwrap();
            view.accessibility
                .sink_for_test()
                .try_dispatch(AccessibilityIntent {
                    node: id,
                    stamp: tree.stamp,
                    action: Action::SetValue("STALE SHOULD NOT APPLY".into()),
                })
                .unwrap();
            let input = view.input.as_ref().unwrap().clone();
            input
                .update(cx, |input, cx| {
                    input.set_selection_utf16(3..3, false, window, cx)
                })
                .unwrap();
            view.refresh_native_text(cx);
            view.poll(cx);
            view.accessibility_actions(window, cx);
            assert_eq!(
                view.native_text(cx).unwrap().text.as_ref(),
                "文😀e\u{301}\nfixture"
            );
            assert!(!view.snapshot.as_ref().unwrap().edit.as_ref().unwrap().dirty);
            let after = view.accessibility.authoritative_key().unwrap();
            assert_eq!(after.native_text_revision, before.native_text_revision);
            assert_eq!(after.semantic_revision, before.semantic_revision);
            assert_ne!(after.host_focus_revision, before.host_focus_revision);
            input.update(cx, |input, cx| {
                gpui::EntityInputHandler::replace_and_mark_text_in_range(
                    input,
                    Some(3..5),
                    "e\u{301}",
                    Some(2..2),
                    window,
                    cx,
                )
            });
            view.refresh_native_text(cx);
            let text = view.native_text(cx).unwrap();
            assert_eq!(text.text.as_ref(), "文😀e\u{301}\nfixture");
            assert_eq!(text.revision, before.native_text_revision);
            assert_eq!(text.marked_utf16, Some(3..5));
            let marked = view.accessibility.observe(
                view.snapshot.as_ref().unwrap(),
                Some(&text),
                view.rendered_footer(),
            );
            assert_ne!(marked.host_focus_revision, after.host_focus_revision);
        });
        cx.simulate_keystrokes("escape");
        loop {
            let done = view.update_in(cx, |view, _, cx| {
                view.poll(cx);
                view.input.is_none() && view.snapshot.as_ref().is_some_and(|s| s.edit.is_none())
            });
            if done {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        let shutdown = view.update_in(cx, |view, _, _| {
            let witness = view.runtime.attach(view.runtime.window_generation);
            view.runtime.stop(&view.controls);
            view.close();
            witness
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !shutdown.shutdown_complete() {
            assert!(
                std::time::Instant::now() < deadline,
                "fixture persistence processing drain must complete before the fixture scope exits"
            );
            std::thread::yield_now();
        }
        assert_eq!(
            std::fs::read(fixture.join("fixture.txt")).unwrap(),
            "文😀e\u{301}\nfixture".as_bytes()
        );
        // Retain the task fixture until checked write receipts and worker retirement
        // prove cleanup safe; the legacy processing barrier is insufficient.
    }

    #[gpui::test]
    fn accessibility_rename_value_uses_native_draft_and_same_confirmation_gateway(
        cx: &mut TestAppContext,
    ) {
        let _fixture_scope = crate::test_support::enter();
        cx.update(text_input::register);
        let fixture = std::env::temp_dir().join(format!("ira-ax-draft-{}", std::process::id()));
        std::fs::create_dir_all(&fixture).unwrap();
        std::fs::write(fixture.join("original.txt"), b"fixture").unwrap();
        let mut app = App::default();
        app.window_generation = 7;
        app.panes[0].folder = Some(Folder::new(
            "Fixture".into(),
            fixture.to_string_lossy().into_owned(),
            '#',
        ));
        app.panes[0].files = list_files(fixture.to_str().unwrap()).unwrap();
        app.panes[0].selected = vec![false];
        app.panes[0].listing_settled = true;
        app.panes[0].listing_generation = 3;
        app.panes[0].state.select(Some(0));
        app.dispatch(Input::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )))
        .unwrap();
        let runtime = Runtime::with_factory(7, move || app);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let publication = loop {
            if let Some(p) = runtime.try_snapshot() {
                break p;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let mut view = Desktop::new(runtime, crate::platform::geometry::Writer::new(None), cx);
            view.accessibility = accessibility::Host::compatibility_headless();
            view.snapshot = Some(publication.snapshot);
            view
        });
        loop {
            let ready = view.update_in(cx, |view, _, cx| {
                view.poll(cx);
                view.snapshot
                    .as_ref()
                    .is_some_and(|s| s.focus_generation == view.input_generation)
            });
            if ready {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        view.update_in(cx, |view, window, cx| {
            let tree = view.accessibility.tree.as_ref().unwrap();
            let node = tree
                .nodes
                .values()
                .find(|n| matches!(n.target, Target::Text { .. }))
                .unwrap();
            assert!(node.geometry.is_some());
            let stale = view
                .snapshot
                .as_ref()
                .unwrap()
                .renaming
                .as_ref()
                .unwrap()
                .text
                .clone();
            view.accessibility
                .sink_for_test()
                .try_dispatch(AccessibilityIntent {
                    node: node.id,
                    stamp: tree.stamp,
                    action: Action::SetValue("é-ax.txt".into()),
                })
                .unwrap();
            view.poll(cx);
            view.accessibility_actions(window, cx);
            assert_eq!(view.native_text(cx).unwrap().text.as_ref(), "é-ax.txt");
            assert_eq!(
                view.snapshot
                    .as_ref()
                    .unwrap()
                    .renaming
                    .as_ref()
                    .unwrap()
                    .text,
                stale,
                "actor snapshot is still older than authoritative native edit"
            );
        });
        // Enter uses the existing eventful native submit path; actor FIFO applies the AX draft first.
        cx.simulate_keystrokes("enter");
        loop {
            if fixture.join("é-ax.txt").exists() {
                break;
            }
            view.update_in(cx, |view, _, cx| view.poll(cx));
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        view.update_in(cx, |view, _, _| {
            view.runtime.stop(&view.controls);
            view.close();
        });
        assert_eq!(std::fs::read(fixture.join("é-ax.txt")).unwrap(), b"fixture");
        // Retain the task fixture until checked write receipts and worker retirement
        // prove cleanup safe; the legacy processing barrier is insufficient.
    }
    #[gpui::test]
    fn actual_prepared_host_footer_expiry_and_feedback_match_painted_status(
        cx: &mut TestAppContext,
    ) {
        let _fixture_scope = crate::test_support::enter();
        use ira_core::model::{STATUS_TTL, Status};
        let mut app = App::default();
        app.window_generation = 7;
        app.status = Some(Status {
            text: "Transient host status".into(),
            is_error: false,
            raised: std::time::Instant::now() - STATUS_TTL + Duration::from_millis(250),
        });
        let runtime = Runtime::with_factory(7, move || app);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let publication = loop {
            if let Some(p) = runtime.try_snapshot() {
                break p;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let mut view = Desktop::new(runtime, crate::platform::geometry::Writer::new(None), cx);
            view.accessibility = accessibility::Host::headless(); // The production worker adapter.
            view.snapshot = Some(publication.snapshot);
            view
        });
        let wait = |cx: &mut gpui::VisualTestContext, expected: &str| {
            loop {
                let ready = view.update_in(cx, |view, _, cx| {
                    view.poll(cx);
                    let Some(frame) = view.accessibility.prepared_frame_for_test() else {
                        return false;
                    };
                    let Some(key) = view.accessibility.authoritative_key() else {
                        return false;
                    };
                    if frame.publication_seq == 0 || frame.key.request != key {
                        return false;
                    }
                    let Some(id) = view
                        .accessibility
                        .id(&Target::Window, Some(Role::Status), None)
                    else {
                        return false;
                    };
                    frame.semantic.tree.nodes[&id].value.as_deref() == Some(expected)
                        && frame.geometry.nodes.contains_key(&id)
                });
                if ready {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "prepared status failed: {expected}"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        wait(cx, "Transient host status");
        wait(
            cx,
            "Enter: rename · Right: open · /: search · Space: select",
        );
        assert!(cx.debug_bounds("ira-status-footer").unwrap().size.height > px(0.));
        view.update_in(cx, |view, _, cx| {
            view.feedback = Some("Native feedback wins".into());
            cx.notify();
        });
        wait(cx, "Native feedback wins");
        view.update_in(cx, |view, _, _| {
            view.runtime.stop(&view.controls);
            view.close();
        });
    }
    #[gpui::test]
    fn actual_prepared_host_large_viewports_keep_foreground_work_sparse(cx: &mut TestAppContext) {
        let _fixture_scope = crate::test_support::enter();
        use ira_core::services::list_files::FEntry;
        for count in [10_000, 100_000] {
            let mut app = App::default();
            app.window_generation = 7;
            app.panes[0].folder = Some(Folder::new(
                "Performance fixture".into(),
                "/tmp/ira-host-perf".into(),
                '#',
            ));
            app.panes[0].files = (0..count)
                .map(|i| FEntry {
                    path: format!("/tmp/ira-host-perf/file-{i:06}"),
                    label: format!("file-{i:06}"),
                    is_dir: false,
                    size: 0,
                    modified: None,
                })
                .collect();
            app.panes[0].selected = vec![false; count];
            app.panes[0].listing_settled = true;
            app.panes[0].state.select(Some(0));
            let runtime = Runtime::with_factory(7, move || app);
            let deadline = std::time::Instant::now() + Duration::from_secs(12);
            let publication = loop {
                if let Some(p) = runtime.try_snapshot() {
                    break p;
                }
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            };
            let (view, cx) = cx.add_window_view(move |_, cx| {
                let mut view =
                    Desktop::new(runtime, crate::platform::geometry::Writer::new(None), cx);
                view.accessibility = accessibility::Host::headless();
                view.snapshot = Some(publication.snapshot);
                view
            });
            loop {
                let ready = view.update_in(cx, |view, _, cx| {
                    view.poll(cx);
                    view.accessibility
                        .prepared_frame_for_test()
                        .is_some_and(|frame| {
                            frame.publication_seq > 0
                                && Some(frame.key.request) == view.accessibility.authoritative_key()
                        })
                });
                if ready {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "{count} row host failed preparation"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            let old_rows = view.update_in(cx, |view, _, _| {
                view.snapshot.as_ref().unwrap().panes[0].rows.clone()
            });
            let mut timings = Vec::new();
            for _ in 0..30 {
                let start = std::time::Instant::now();
                view.update_in(cx, |_, _, cx| cx.notify());
                timings.push(start.elapsed().as_nanos());
            }
            timings.sort_unstable();
            let (target, original_key) = view.update_in(cx, |view, _, _| {
                let snapshot = view.snapshot.as_ref().unwrap();
                let target = Target::Entry {
                    pane: 0,
                    path: snapshot.panes[0].rows[0].entry.path.clone().into(),
                    listing_generation: snapshot.panes[0].listing_generation,
                };
                let id = view
                    .accessibility
                    .id(&target, Some(Role::Row), Some(AxCapability::Selection))
                    .unwrap();
                let frame = view.accessibility.prepared_frame_for_test().unwrap();
                let key = frame.key.request;
                view.accessibility
                    .sink_for_test()
                    .try_dispatch(AccessibilityIntent {
                        node: id,
                        stamp: frame.semantic.tree.stamp,
                        action: Action::SetSelected(true),
                    })
                    .unwrap();
                (target, key)
            });
            view.update_in(cx, |view, _, cx| {
                view.scroll[0].scroll_to_item(80, ScrollStrategy::Center);
                cx.notify();
            });
            view.update_in(cx, |view, window, cx| {
                assert_eq!(
                    view.accessibility.authoritative_key(),
                    Some(original_key),
                    "pure viewport movement cannot expire a semantic action"
                );
                view.poll(cx);
                view.accessibility_actions(window, cx);
            });
            loop {
                let selected = view.update_in(cx, |view, _, cx| { view.poll(cx); view.snapshot.as_ref().unwrap().panes[0].rows.iter().any(|row| matches!(&target, Target::Entry { path, .. } if std::path::Path::new(&row.entry.path) == path) && row.selected) });
                if selected {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "layout-only change rejected valid semantic selection"
                );
                std::thread::yield_now();
            }
            view.update_in(cx, |view, _, _| {
                view.runtime.enqueue(
                    Command::Input(Input::Action(ira_core::input::Command::CycleSort)),
                    None,
                )
            });
            loop {
                let replaced = view.update_in(cx, |view, _, cx| {
                    view.poll(cx);
                    !Arc::ptr_eq(&view.snapshot.as_ref().unwrap().panes[0].rows, &old_rows)
                });
                if replaced && Arc::strong_count(&old_rows) == 1 {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "old actor/worker snapshot rows retained"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            struct RowDropProof {
                rows: Arc<Vec<ira_core::observable::Row>>,
                tx: mpsc::Sender<(std::thread::ThreadId, usize, usize)>,
            }
            impl Drop for RowDropProof {
                fn drop(&mut self) {
                    let _ = self.tx.send((
                        std::thread::current().id(),
                        Arc::strong_count(&self.rows),
                        self.rows.len(),
                    ));
                }
            }
            let (tx, rx) = mpsc::channel();
            view.update_in(cx, |view, _, _| {
                view.accessibility
                    .retire_probe(RowDropProof { rows: old_rows, tx })
            });
            let proof = loop {
                if let Ok(proof) = rx.try_recv() {
                    break proof;
                }
                view.update_in(cx, |view, _, cx| view.poll(cx));
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            };
            assert_ne!(
                proof.0,
                std::thread::current().id(),
                "last real source row allocation dropped on UI"
            );
            assert_eq!(
                proof.1, 1,
                "probe must own the final real row allocation Arc"
            );
            assert_eq!(proof.2, count);
            eprintln!(
                "actual_source_row_last_drop rows={} strong_count={} off_ui={} destructor_thread={:?} foreground_thread={:?}",
                proof.2,
                proof.1,
                proof.0 != std::thread::current().id(),
                proof.0,
                std::thread::current().id()
            );
            view.update_in(cx, |view, _, _| {
                let frame = view.accessibility.prepared_frame_for_test().unwrap();
                assert_eq!(frame.semantic.tree.nodes.values().filter(|n| n.role == Role::Row && matches!(n.target, Target::Entry { .. })).count(), count);
                assert!(!frame.geometry.nodes.is_empty());
                assert!(frame.geometry.nodes.len() < 100, "foreground geometry must be viewport-bounded");
                let (_, visits) = frame.geometry.hit_test_with_visits(-100., -100.);
                assert!(visits <= frame.geometry.nodes.len());
                eprintln!("actual_host rows={count} geometry={} hit_visits={visits} render_ns_p50={} p95={} max={}", frame.geometry.nodes.len(), timings[15], timings[28], timings[29]);
                view.runtime.stop(&view.controls);
                view.close();
            });
        }
    }
    #[gpui::test]
    fn actual_status_expiry_keeps_accessible_rendered_fallback(cx: &mut TestAppContext) {
        let _fixture_scope = crate::test_support::enter();
        use ira_core::model::{STATUS_TTL, Status};
        let mut app = App::default();
        app.window_generation = 7;
        app.status = Some(Status {
            text: "Transient".into(),
            is_error: false,
            raised: std::time::Instant::now() - STATUS_TTL + Duration::from_millis(80),
        });
        let runtime = Runtime::with_factory(7, move || app);
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        let publication = loop {
            if let Some(p) = runtime.try_snapshot() {
                break p;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let mut view = Desktop::new(runtime, crate::platform::geometry::Writer::new(None), cx);
            view.accessibility = accessibility::Host::headless();
            view.snapshot = Some(publication.snapshot);
            view
        });
        loop {
            let ready = view.update_in(cx, |view, _, cx| {
                view.poll(cx);
                view.snapshot.as_ref().is_some_and(|s| s.status.is_none())
                    && view.accessibility.tree.as_ref().is_some_and(|tree| {
                        tree.nodes.values().any(|node| {
                            node.role == Role::Status
                                && node.value.as_deref()
                                    == Some(
                                        "Enter: rename · Right: open · /: search · Space: select",
                                    )
                        })
                    })
            });
            if ready {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "actual painted fallback footer lost its accessible Status value after expiry"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(cx.debug_bounds("ira-status-footer").unwrap().size.height > px(0.));
        view.update_in(cx, |view, _, _| {
            view.runtime.stop(&view.controls);
            view.close();
        });
    }

    #[gpui::test]
    fn tui_parity_layout_and_keybindings_modal_renders(cx: &mut TestAppContext) {
        let _fixture_scope = crate::test_support::enter();
        let mut app = App::default();
        app.window_generation = 12;
        app.drives = Some(vec![Folder::new("Macintosh HD".into(), "/".into(), '1')]);
        app.folders = Some(vec![Folder::new("Home".into(), "/Users/test".into(), 'h')]);
        app.bookmarks = Some(vec![Folder::new("Work".into(), "/Users/test/work".into(), 'w')]);

        let runtime = Runtime::with_factory(12, move || app);
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        let publication = loop {
            if let Some(p) = runtime.try_snapshot() {
                break p;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };

        let (view, cx) = cx.add_window_view(move |_, cx| {
            let mut view = Desktop::new(runtime, crate::platform::geometry::Writer::new(None), cx);
            view.accessibility = accessibility::Host::headless();
            view.snapshot = Some(publication.snapshot);
            view
        });

        // Verify the 5-row panels are painted with valid bounds
        assert!(cx.debug_bounds("ira-drives-row").is_some(), "Row 1 (drives shortcuts) must be painted");
        assert!(cx.debug_bounds("ira-common-folders-row").is_some(), "Row 2 (common folders) must be painted");
        assert!(cx.debug_bounds("ira-bookmarks-and-actions-row").is_some(), "Row 3 (bookmarks & actions) must be painted");
        assert!(cx.debug_bounds("ira-files-area").is_some(), "Row 4 (panes & copy board files area) must be painted");
        assert!(cx.debug_bounds("ira-status-footer").is_some(), "Row 5 (status / shortcuts marquee footer) must be painted");
        assert!(cx.debug_bounds("keybindings-btn").is_some(), "* Keybindings button must be painted in footer");

        // Verify accessibility tree contains the 3 place kinds
        view.update_in(cx, |view, _, _| {
            let tree = view.accessibility.tree.as_ref().unwrap();
            assert!(tree.nodes.values().any(|n| matches!(&n.target, Target::Place { kind: crate::platform::accessibility::model::PlaceKind::Drive, .. })));
            assert!(tree.nodes.values().any(|n| matches!(&n.target, Target::Place { kind: crate::platform::accessibility::model::PlaceKind::Common, .. })));
            assert!(tree.nodes.values().any(|n| matches!(&n.target, Target::Place { kind: crate::platform::accessibility::model::PlaceKind::Bookmark, .. })));
        });

        // Open Keybindings modal via '*' keypress
        view.update_in(cx, |view, _, cx| {
            view.dispatch(KeyCode::Char('*'), cx);
        });

        loop {
            let ready = view.update_in(cx, |view, _, cx| {
                view.poll(cx);
                view.snapshot.as_ref().is_some_and(|s| s.keybindings_visible)
            });
            if ready {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }

        assert!(cx.debug_bounds("ira-modal").is_some(), "13-row keybindings modal must be painted when active");

        view.update_in(cx, |view, _, _| {
            view.runtime.stop(&view.controls);
            view.close();
        });
    }
}
