pub mod accessibility;
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
    accessibility_actions: Vec<ResolvedAction>,
    snapshot: Option<Arc<Snapshot>>,
    controls: Vec<Arc<JobControl>>,
    focus: FocusHandle,
    place_focus: BTreeMap<AxTarget, FocusHandle>,
    scroll: [UniformListScrollHandle; 2],
    input: Option<Entity<TextInput>>,
    input_mode: Option<InputMode>,
    input_generation: u64,
    input_subscription: Option<Subscription>,
    polling: Option<Task<()>>,
    feedback: Option<String>,
    theme: ira_core::theme::Theme,
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
            accessibility: accessibility::Host::default(),
            accessibility_actions: Vec::new(),
            snapshot: None,
            controls: Vec::new(),
            focus: cx.focus_handle(),
            place_focus: BTreeMap::new(),
            scroll: std::array::from_fn(|_| UniformListScrollHandle::new()),
            input: None,
            input_mode: None,
            input_generation: 0,
            input_subscription: None,
            polling: Some(polling),
            feedback: None,
            theme: ira_core::theme::ThemePreset::default().theme(),
            font_family: None,
            geometry,
            geometry_subscription: None,
            host_tx,
            host_rx,
        }
    }
    pub fn focus_main(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        self.geometry_subscription = Some(cx.observe_window_bounds(window, |this, window, _| {
            this.geometry
                .save(crate::platform::geometry::Geometry::from_bounds(
                    window.window_bounds(),
                ));
        }));
        cx.notify();
    }
    pub fn close(&mut self) {
        self.accessibility.close();
        self.accessibility_actions.clear();
        self.runtime.detach();
        self.runtime.cancel_jobs(&self.controls);
        self.polling.take();
    }
    fn poll(&mut self, cx: &mut Context<Self>) {
        self.runtime.flush();
        if let Some(error) = self.geometry.try_error() {
            self.runtime.enqueue(Command::HostResult(Err(error)), None);
        }
        let mut changed = false;
        if let Some(publication) = self.runtime.try_snapshot()
            && publication.snapshot.window_generation == self.runtime.window_generation
        {
            for (index, pane) in publication.snapshot.panes.iter().enumerate() {
                if self.snapshot.as_ref().is_none_or(|old| {
                    old.panes[index].cursor != pane.cursor
                        || old.panes[index].listing_generation != pane.listing_generation
                }) && let Some(cursor) = pane.cursor
                {
                    self.scroll[index].scroll_to_item(cursor, ScrollStrategy::Center);
                }
            }
            self.theme = publication.theme;
            self.font_family = publication.font_family;
            self.snapshot = Some(publication.snapshot);
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
                    let barrier = self.geometry.barrier();
                    cx.spawn(async move |_, cx| {
                        cx.background_executor()
                            .spawn(async move {
                                let _ = barrier.recv_timeout(Duration::from_secs(1));
                            })
                            .await;
                        let _ = cx.update(|cx| cx.quit());
                    })
                    .detach();
                    return;
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
            HostRequest::InvalidatePreview(_) => {}
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
        self.runtime
            .enqueue(Command::Input(actions::input(code)), None);
        cx.notify();
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
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
        let input = cx.new(|cx| TextInput::new(text, options, cx));
        self.input_subscription = Some(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
            this.input_event(event, cx)
        }));
        input.update(cx, |input, cx| input.focus(window, cx));
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
        self.runtime.enqueue(command, generation);
        cx.notify();
    }
    fn native_text(
        &self,
        cx: &gpui::App,
    ) -> Option<crate::platform::accessibility::model::NativeTextSnapshot> {
        Some(accessibility::native_text(
            self.input.as_ref()?,
            self.input_mode?,
            self.snapshot
                .as_ref()
                .and_then(|s| s.edit.as_ref())
                .is_some_and(|e| e.read_only),
            cx,
        ))
    }
    fn accessibility_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for action in std::mem::take(&mut self.accessibility_actions) {
            let Some(snapshot) = &self.snapshot else {
                continue;
            };
            let text = self.native_text(cx);
            if (
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
            ) {
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
                        self.scroll[*pane].scroll_to_item(index, ScrollStrategy::Center);
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
            .rounded_md()
            .bg(native_color(self.theme.surface))
            .cursor_pointer()
            .child(label)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.dispatch(code, cx)));
        if label == "Confirm" || label == "Cancel" {
            self.accessibility.measured(
                button,
                AxTarget::Modal,
                Some(AxRole::Button),
                Some(if label == "Confirm" {
                    AxCapability::Activate
                } else {
                    AxCapability::Dismiss
                }),
            )
        } else {
            button.into_any_element()
        }
    }
    fn pane(&self, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(snapshot) = &self.snapshot else {
            return div().flex_1().child("Loading…").into_any_element();
        };
        let pane = &snapshot.panes[index];
        let title = pane
            .folder
            .as_ref()
            .map(|f| f.path.clone())
            .unwrap_or_else(|| "No folder open".into());
        let header = div()
            .px_3()
            .py_2()
            .bg(native_color(if snapshot.active_pane == index {
                self.theme.border_active
            } else {
                self.theme.surface
            }))
            .child(title);
        let mut body = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .border_1()
            .border_color(native_color(self.theme.border))
            .child(header);
        if pane.rows.is_empty() {
            body = body.child(div().p_4().child(if !pane.listing_settled {
                "Loading…"
            } else if pane.filter_query.is_some() {
                "No matches"
            } else {
                "Empty folder"
            }));
        } else {
            body=body.child(uniform_list(("files",index),pane.rows.len(),cx.processor(move|this,range:std::ops::Range<usize>,_,cx|{
                let Some(snapshot)=this.snapshot.as_ref() else{return Vec::new();};
                let pane=&snapshot.panes[index];
                range.filter_map(|row_index|pane.rows.get(row_index).map(|row|{
                    let target=EntryTarget{pane:index,path:PathBuf::from(&row.entry.path),listing_generation:pane.listing_generation};
                    let (glyph, category) = ira_core::theme::icons::icon_for(&row.entry, snapshot.icons);
                    let label=format!("{} {} {}",if row.selected{"☑"}else{"☐"},glyph,row.entry.label);
                    let foreground = if pane.cursor == Some(row_index) && snapshot.active_pane == index { this.theme.cursor_fg } else { this.theme.color_for(category) };
                    let size=if row.entry.is_dir{"Folder".to_string()}else{format!("{} B",row.entry.size)};
                    let element=div().id(("row",row_index)).h(px(30.)).px_3().flex().justify_between().items_center().bg(native_color(if pane.cursor==Some(row_index)&&snapshot.active_pane==index{this.theme.cursor_bg}else if row.selected{this.theme.selection}else{this.theme.bg})).text_color(native_color(foreground)).cursor_pointer().child(label).child(div().text_sm().text_color(native_color(this.theme.text_muted)).child(size))
                        .on_click(cx.listener(move|this,event:&ClickEvent,window,cx|{
                            let double=matches!(event,ClickEvent::Mouse(e) if e.down.click_count>=2);
                            let modifiers=event.modifiers();
                            let verb=if double{TargetVerb::Open}else if modifiers.platform||modifiers.control{TargetVerb::Toggle}else{TargetVerb::Focus};
                            this.target(target.clone(),verb,window,cx);
                        }));
                    this.accessibility.measured(element,AxTarget::Entry{pane:index,path:PathBuf::from(&row.entry.path),listing_generation:pane.listing_generation},Some(AxRole::Row),None)
                })).collect::<Vec<_>>()
            })).track_scroll(self.scroll[index].clone()).flex_1().min_h_0());
        }
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
    fn places(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut list = div()
            .w(px(190.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(native_color(self.theme.surface_alt));
        if let Some(snapshot) = &self.snapshot {
            for (heading, folders) in [
                ("Drives", &snapshot.drives),
                ("Places", &snapshot.folders),
                ("Bookmarks", &snapshot.bookmarks),
            ] {
                list = list.child(
                    div()
                        .text_sm()
                        .text_color(native_color(self.theme.text_muted))
                        .child(heading),
                );
                if let Some(folders) = folders {
                    let mut occurrences = BTreeMap::new();
                    for (i, folder) in folders.iter().enumerate() {
                        let key = if heading == "Drives" {
                            char::from_digit((i + 1) as u32, 10).unwrap_or(' ')
                        } else {
                            folder.shortcut
                        };
                        let label = format!("{}  {}", key, folder.label);
                        let path = folder.path.clone();
                        let kind = match heading {
                            "Drives" => PlaceKind::Drive,
                            "Places" => PlaceKind::Common,
                            _ => PlaceKind::Bookmark,
                        };
                        let ax_kind = match kind {
                            PlaceKind::Drive => {
                                crate::platform::accessibility::model::PlaceKind::Drive
                            }
                            PlaceKind::Common => {
                                crate::platform::accessibility::model::PlaceKind::Common
                            }
                            PlaceKind::Bookmark => {
                                crate::platform::accessibility::model::PlaceKind::Bookmark
                            }
                        };
                        let count = occurrences
                            .entry((folder.path.clone(), folder.shortcut))
                            .or_insert(0usize);
                        let occurrence = *count;
                        *count += 1;
                        let ax_target = AxTarget::Place {
                            kind: ax_kind,
                            path: PathBuf::from(&path),
                            shortcut: folder.shortcut,
                            occurrence,
                        };
                        let handle = self
                            .place_focus
                            .entry(ax_target.clone())
                            .or_insert_with(|| cx.focus_handle())
                            .clone();
                        let shortcut = folder.shortcut;
                        list = list.child(
                            self.accessibility.measured(
                                div()
                                    .id((heading, i))
                                    .track_focus(&handle)
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .cursor_pointer()
                                    .child(label)
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        this.runtime.enqueue(
                                            Command::PlaceExact {
                                                kind,
                                                path: path.clone(),
                                                shortcut,
                                                occurrence,
                                            },
                                            None,
                                        );
                                        cx.notify();
                                    })),
                                ax_target,
                                Some(AxRole::Button),
                                Some(AxCapability::Activate),
                            ),
                        );
                    }
                }
            }
        }
        self.place_focus.retain(|target, _| {
            self.accessibility
                .id(target, Some(AxRole::Button), Some(AxCapability::Focus))
                .is_some()
        });
        list.into_any_element()
    }
    fn modal(&self, height: gpui::Pixels, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let snapshot = self.snapshot.as_ref()?;
        let mut modal = div()
            .id("ira-modal")
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
            if self.input_mode == Some(InputMode::Search) {
                return None;
            }
            let label = match self.input_mode {
                Some(InputMode::Rename) => "Rename",
                Some(InputMode::Create) => "Create file or folder",
                Some(InputMode::Goto) => "Go to path",
                Some(InputMode::Editor) => "Text editor",
                _ => "Input",
            };
            return Some(
                modal
                    .child(label)
                    .child(self.accessibility.measured(
                        div().child(input.clone()),
                        AxTarget::Text {
                            document: snapshot.document_generation,
                        },
                        None,
                        None,
                    ))
                    .child("Enter to confirm · Esc to cancel")
                    .into_any_element(),
            );
        }
        if let Some(confirm) = &snapshot.confirming {
            modal = modal
                .child(format!("{:?} {}", confirm.action, confirm.label))
                .child(format!("Collision policy: {:?}", confirm.policy))
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .child(self.button("Confirm", KeyCode::Enter, cx))
                        .child(self.button("Cancel", KeyCode::Esc, cx))
                        .child(self.button("Policy", KeyCode::Char('o'), cx)),
                );
        } else if let Some(status) = snapshot.status.as_ref().filter(|s| s.is_error) {
            modal = modal.child(status.text.clone()).child("Any key dismisses");
        } else if let Some(info) = &snapshot.info {
            modal = modal
                .child(
                    div()
                        .id("info-scroll")
                        .max_h(px((f32::from(height) - 200.).max(60.)))
                        .overflow_y_scroll()
                        .children(info.lines.clone()),
                )
                .child("x: stop · r: recalculate · any other key: close");
        } else if let Some(info) = &snapshot.multi_info {
            modal = modal
                .child(format!(
                    "{} folders · {} files · {} bytes",
                    info.folders, info.files, info.aggregate.1
                ))
                .child("Any key closes");
        } else if let Some(deletion) = snapshot.deletion.as_ref().filter(|d| !d.hidden) {
            modal = modal
                .child(format!("Deleting {} / {}", deletion.done, deletion.total))
                .child("Any key hides; deletion continues");
        } else if snapshot.keybindings_visible {
            modal=modal.child("Arrows: navigate · Enter: rename · Space: select · /: search · c/m: copy/move · +: split · Tab: focus · `: jobs · q: quit").child("Any key closes");
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
        self.sync_input(window, cx);
        self.accessibility_actions(window, cx);
        self.accessibility.focused_target = self
            .place_focus
            .iter()
            .find(|(_, handle)| handle.is_focused(window))
            .map(|(target, _)| target.clone());
        if let Some(snapshot) = &self.snapshot {
            let text = self.native_text(cx);
            self.accessibility.begin(snapshot, text.as_ref());
        }
        let mut panes = div().flex().flex_1().min_h_0().child(self.pane(0, cx));
        if self.snapshot.as_ref().is_some_and(|s| s.split) {
            panes = panes.child(self.pane(1, cx));
        }
        let mut content = div().flex().flex_col().flex_1().min_w_0().h_full();
        if self.input_mode == Some(InputMode::Search)
            && let Some(input) = &self.input
        {
            content = content.child(self.accessibility.measured(
                div().px_3().py_2().child(input.clone()),
                AxTarget::Text {
                    document: self.snapshot.as_ref().map_or(0, |s| s.document_generation),
                },
                None,
                None,
            ));
        }
        content = content.child(panes);
        if let Some(snapshot) = &self.snapshot
            && snapshot.copy_board
        {
            let mut board = div()
                .p_3()
                .bg(native_color(self.theme.surface_alt))
                .child("Copy Board");
            for (index, job) in snapshot.jobs.iter().enumerate() {
                let id = job.id;
                let mut row = div()
                    .id(("job", index))
                    .p_2()
                    .flex()
                    .gap_3()
                    .items_center()
                    .bg(native_color(
                        if snapshot.board_focused && snapshot.copy_board_cursor == Some(index) {
                            self.theme.cursor_bg
                        } else {
                            self.theme.surface_alt
                        },
                    ))
                    .child(format!(
                        "{} · {:?} · {} / {} B",
                        job.label,
                        job.status,
                        job.copied_bytes,
                        job.total_bytes.unwrap_or(0)
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
                for (label, verb) in [
                    ("Pause / Resume", JobVerb::Pause),
                    ("Cancel", JobVerb::Cancel),
                ] {
                    row = row.child(
                        self.accessibility.measured(
                            div()
                                .id((label, index))
                                .px_2()
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
            content = content.child(board);
        }
        let status = self
            .feedback
            .clone()
            .or_else(|| {
                self.snapshot
                    .as_ref()
                    .and_then(|s| s.status.as_ref().map(|s| s.text.clone()))
            })
            .unwrap_or_else(|| "Enter: rename · Right: open · /: search · Space: select".into());
        let mut root =
            div()
                .id("ira-root")
                .key_context("IraDesktop")
                .track_focus(&self.focus)
                .relative()
                .size_full()
                .flex()
                .flex_col()
                .bg(native_color(self.theme.bg))
                .text_color(native_color(self.theme.text))
                .on_key_down(cx.listener(Self::key))
                .on_action(cx.listener(|this, _: &actions::Quit, _, _cx| {
                    this.runtime.stop(&this.controls);
                }))
                .on_action(
                    cx.listener(|this, _: &actions::Rename, _, cx| {
                        this.dispatch(KeyCode::Enter, cx)
                    }),
                )
                .on_action(
                    cx.listener(|this, _: &actions::Open, _, cx| this.dispatch(KeyCode::Right, cx)),
                )
                .on_action(
                    cx.listener(|this, _: &actions::Parent, _, cx| {
                        this.dispatch(KeyCode::Left, cx)
                    }),
                )
                .on_action(cx.listener(|this, _: &actions::Search, _, cx| {
                    this.dispatch(KeyCode::Char('/'), cx)
                }))
                .on_action(cx.listener(|this, _: &actions::NewEntry, _, cx| {
                    this.dispatch(KeyCode::Char('n'), cx)
                }))
                .on_action(cx.listener(|this, _: &actions::Copy, _, cx| {
                    this.dispatch(KeyCode::Char('c'), cx)
                }))
                .on_action(cx.listener(|this, _: &actions::Move, _, cx| {
                    this.dispatch(KeyCode::Char('m'), cx)
                }))
                .on_action(cx.listener(|this, _: &actions::Delete, _, cx| {
                    this.dispatch(KeyCode::Delete, cx)
                }))
                .on_action(cx.listener(|this, _: &actions::ToggleSplit, _, cx| {
                    this.dispatch(KeyCode::Char('+'), cx)
                }))
                .on_action(cx.listener(|this, _: &actions::ToggleBoard, _, cx| {
                    this.dispatch(KeyCode::Char('`'), cx)
                }))
                .on_action(cx.listener(|this, _: &actions::Sort, _, cx| {
                    this.dispatch(KeyCode::Char(','), cx)
                }))
                .on_action(cx.listener(|this, _: &actions::Hidden, _, cx| {
                    this.dispatch(KeyCode::Char('.'), cx)
                }))
                .on_action(cx.listener(|this, _: &actions::Bookmark, _, cx| {
                    this.dispatch(KeyCode::Char('b'), cx)
                }))
                .on_action(cx.listener(|this, _: &actions::Info, _, cx| {
                    this.dispatch(KeyCode::Char('?'), cx)
                }))
                .on_action(cx.listener(|this, _: &actions::Theme, _, cx| {
                    this.dispatch(KeyCode::Char('\\'), cx)
                }))
                .on_action(cx.listener(|this, _: &actions::FocusNext, _, cx| {
                    this.dispatch(KeyCode::Tab, cx)
                }))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .p_2()
                        .flex_wrap()
                        .child(self.button("Parent", KeyCode::Left, cx))
                        .child(self.button("Rename", KeyCode::Enter, cx))
                        .child(self.button("New", KeyCode::Char('n'), cx))
                        .child(self.button("Search", KeyCode::Char('/'), cx))
                        .child(self.button("Copy", KeyCode::Char('c'), cx))
                        .child(self.button("Move", KeyCode::Char('m'), cx))
                        .child(self.button("Delete", KeyCode::Delete, cx))
                        .child(self.button("Split", KeyCode::Char('+'), cx))
                        .child(self.button("Sort", KeyCode::Char(','), cx))
                        .child(self.button("Hidden", KeyCode::Char('.'), cx))
                        .child(self.button("Jobs", KeyCode::Char('`'), cx)),
                )
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .child(self.places(cx))
                        .child(content),
                )
                .child(
                    self.accessibility.measured(
                        div()
                            .p_2()
                            .text_sm()
                            .bg(native_color(self.theme.surface_alt))
                            .child(status),
                        AxTarget::Window,
                        Some(AxRole::Status),
                        None,
                    ),
                );
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
    fn duplicate_place_focus_uses_real_handle_without_navigation(cx: &mut TestAppContext) {
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
            view.accessibility = accessibility::Host::headless();
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
            view.accessibility = accessibility::Host::headless();
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
        std::fs::remove_dir_all(fixture).unwrap();
    }
    #[gpui::test]
    fn accessibility_rename_value_uses_native_draft_and_same_confirmation_gateway(
        cx: &mut TestAppContext,
    ) {
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
            view.accessibility = accessibility::Host::headless();
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
        std::fs::remove_dir_all(fixture).unwrap();
    }
}
