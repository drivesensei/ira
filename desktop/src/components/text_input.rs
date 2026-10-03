//! Native GPUI text input. Register bindings once; retain event subscriptions in host.
use super::text_buffer::{BufferSnapshot, TextBuffer};
use gpui::{prelude::*, *};
use std::ops::Range;
actions!(
    ira_native_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        Up,
        Down,
        SelectUp,
        SelectDown,
        Home,
        End,
        SelectAll,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        Enter,
        Cancel,
        Quit,
        Save,
        Traverse
    ]
);
#[derive(Clone, Debug)]
pub struct InputOptions {
    pub multiline: bool,
    pub placeholder: SharedString,
    pub access: InputAccess,
    pub document_generation: u64,
    pub focus_generation: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputAccess {
    #[default]
    Editable,
    ReadOnly,
    Disabled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputStamp {
    pub document_generation: u64,
    pub focus_generation: u64,
    pub value_revision: u64,
}
impl Default for InputOptions {
    fn default() -> Self {
        Self {
            multiline: false,
            placeholder: "".into(),
            access: InputAccess::Editable,
            document_generation: 0,
            focus_generation: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEventKind {
    Changed(BufferSnapshot),
    Submit(String),
    Cancel,
    Quit,
    Save(String),
    Traverse,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputEvent {
    pub stamp: InputStamp,
    pub kind: InputEventKind,
}
pub fn register(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("IraNativeInput")),
        KeyBinding::new("delete", Delete, Some("IraNativeInput")),
        KeyBinding::new("left", Left, Some("IraNativeInput")),
        KeyBinding::new("right", Right, Some("IraNativeInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("IraNativeInput")),
        KeyBinding::new("shift-right", SelectRight, Some("IraNativeInput")),
        KeyBinding::new("up", Up, Some("IraNativeInput")),
        KeyBinding::new("down", Down, Some("IraNativeInput")),
        KeyBinding::new("shift-up", SelectUp, Some("IraNativeInput")),
        KeyBinding::new("shift-down", SelectDown, Some("IraNativeInput")),
        KeyBinding::new("home", Home, Some("IraNativeInput")),
        KeyBinding::new("end", End, Some("IraNativeInput")),
        KeyBinding::new("ctrl-a", Home, Some("IraNativeEditor")),
        KeyBinding::new("ctrl-e", End, Some("IraNativeEditor")),
        KeyBinding::new("ctrl-c", Quit, Some("IraNativeInput")),
        KeyBinding::new("ctrl-s", Save, Some("IraNativeEditor")),
        KeyBinding::new("cmd-a", SelectAll, Some("IraNativeInput")),
        KeyBinding::new("cmd-c", Copy, Some("IraNativeInput")),
        KeyBinding::new("cmd-x", Cut, Some("IraNativeInput")),
        KeyBinding::new("cmd-v", Paste, Some("IraNativeInput")),
        KeyBinding::new("cmd-z", Undo, Some("IraNativeInput")),
        KeyBinding::new("cmd-shift-z", Redo, Some("IraNativeInput")),
        KeyBinding::new("ctrl-shift-c", Copy, Some("IraNativeInput")),
        KeyBinding::new("ctrl-shift-x", Cut, Some("IraNativeInput")),
        KeyBinding::new("ctrl-shift-v", Paste, Some("IraNativeInput")),
        KeyBinding::new("ctrl-z", Undo, Some("IraNativeInput")),
        KeyBinding::new("ctrl-y", Redo, Some("IraNativeInput")),
        KeyBinding::new("enter", Enter, Some("IraNativeInput")),
        KeyBinding::new("escape", Cancel, Some("IraNativeInput")),
        KeyBinding::new("tab", Traverse, Some("IraNativeInput")),
    ]);
}
pub struct TextInput {
    buffer: TextBuffer,
    options: InputOptions,
    focus_handle: FocusHandle,
    lines: Vec<(usize, ShapedLine, Bounds<Pixels>)>,
    selecting: bool,
    revision: u64,
    last_text: String,
}
impl EventEmitter<InputEvent> for TextInput {}
impl TextInput {
    pub fn new(text: impl Into<String>, options: InputOptions, cx: &mut Context<Self>) -> Self {
        let text = text.into();
        let text = if options.multiline {
            text
        } else {
            text.replace(['\r', '\n'], "")
        };
        Self {
            buffer: TextBuffer::new(text.clone()),
            last_text: text,
            revision: 0,
            options,
            focus_handle: cx.focus_handle(),
            lines: vec![],
            selecting: false,
        }
    }
    pub fn snapshot(&self) -> BufferSnapshot {
        self.buffer.snapshot()
    }
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        let text = text.into();
        let text = self.normalize(&text);
        self.buffer = TextBuffer::new(text);
        self.lines.clear();
        self.changed(cx);
    }
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        cx.notify();
    }
    fn normalize(&self, text: &str) -> String {
        if self.options.multiline {
            text.replace("\r\n", "\n").replace('\r', "\n")
        } else {
            text.replace(['\r', '\n'], "")
        }
    }
    pub fn stamp(&self) -> InputStamp {
        InputStamp {
            document_generation: self.options.document_generation,
            focus_generation: self.options.focus_generation,
            value_revision: self.revision,
        }
    }
    pub fn set_access(&mut self, access: InputAccess, cx: &mut Context<Self>) {
        self.buffer.unmark();
        self.options.access = access;
        cx.notify();
    }
    pub fn set_focus_generation(&mut self, generation: u64, cx: &mut Context<Self>) {
        self.options.focus_generation = generation;
        cx.notify();
    }
    fn emit(&self, kind: InputEventKind, cx: &mut Context<Self>) {
        cx.emit(InputEvent {
            stamp: self.stamp(),
            kind,
        });
    }
    fn editable(&self) -> bool {
        self.options.access == InputAccess::Editable
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        if self.last_text != self.buffer.text() {
            self.revision = self
                .revision
                .checked_add(1)
                .expect("input revision exhausted");
            self.last_text = self.buffer.text().to_owned();
        }
        self.emit(InputEventKind::Changed(self.snapshot()), cx);
        cx.notify();
        cx.stop_propagation();
    }
    fn at(&self, p: Point<Pixels>) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        for (offset, line, bounds) in &self.lines {
            if p.y < bounds.bottom() {
                return (*offset + line.closest_index_for_x(p.x - bounds.left()))
                    .min(self.buffer.text().len());
            }
        }
        self.buffer.text().len()
    }
    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let r = self.buffer.selection();
        if !r.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.buffer.text()[r].to_string()));
        }
        self.buffer.break_group();
        cx.stop_propagation();
    }
    fn cut(&mut self, _: &Cut, w: &mut Window, cx: &mut Context<Self>) {
        if !self.editable() {
            cx.stop_propagation();
            return;
        }
        self.copy(&Copy, w, cx);
        if !self.buffer.selection().is_empty() {
            self.buffer.replace(None, "", false);
            self.changed(cx);
        }
    }
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if !self.editable() {
            cx.stop_propagation();
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
            let text = self.normalize(&text);
            self.buffer.replace(None, &text, false);
            self.changed(cx);
        }
        cx.stop_propagation();
    }
    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if !self.editable() {
            cx.stop_propagation();
            return;
        }
        self.buffer.delete(false);
        self.changed(cx);
    }
    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if !self.editable() {
            cx.stop_propagation();
            return;
        }
        self.buffer.delete(true);
        self.changed(cx);
    }
    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.horizontal(false, false);
        self.changed(cx);
    }
    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.horizontal(true, false);
        self.changed(cx);
    }
    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.horizontal(false, true);
        self.changed(cx);
    }
    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.horizontal(true, true);
        self.changed(cx);
    }
    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        if self.options.multiline {
            self.buffer.vertical(false, false);
            self.changed(cx);
        }
    }
    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        if self.options.multiline {
            self.buffer.vertical(true, false);
            self.changed(cx);
        }
    }
    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.options.multiline {
            self.buffer.vertical(false, true);
            self.changed(cx);
        }
    }
    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        if self.options.multiline {
            self.buffer.vertical(true, true);
            self.changed(cx);
        }
    }
    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_to(self.buffer.line_start(), false);
        self.changed(cx);
    }
    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_to(self.buffer.line_end(), false);
        self.changed(cx);
    }
    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select(0, self.buffer.text().len());
        self.changed(cx);
    }
    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if !self.editable() {
            cx.stop_propagation();
            return;
        }
        self.buffer.undo();
        self.changed(cx);
    }
    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if !self.editable() {
            cx.stop_propagation();
            return;
        }
        self.buffer.redo();
        self.changed(cx);
    }
    fn enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.marked().is_some() {
            self.buffer.unmark();
            self.changed(cx);
            return;
        }
        if self.options.multiline {
            if !self.editable() {
                cx.stop_propagation();
                return;
            }
            self.buffer.replace(None, "\n", false);
            self.changed(cx);
        } else {
            self.emit(InputEventKind::Submit(self.buffer.text().to_string()), cx);
            cx.stop_propagation();
        }
    }
    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        self.emit(InputEventKind::Cancel, cx);
        cx.stop_propagation();
    }
    fn quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        self.emit(InputEventKind::Quit, cx);
        cx.stop_propagation();
    }
    fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        self.emit(InputEventKind::Save(self.buffer.text().to_string()), cx);
        cx.stop_propagation();
    }
    fn traverse(&mut self, _: &Traverse, _: &mut Window, cx: &mut Context<Self>) {
        self.emit(InputEventKind::Traverse, cx);
        cx.stop_propagation();
    }
}
impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        r: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let r = self.buffer.from_utf16(r);
        *actual = Some(self.buffer.to_utf16(r.clone()));
        Some(self.buffer.text()[r].to_string())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if self.options.access == InputAccess::Disabled {
            return None;
        }
        Some(UTF16Selection {
            range: self.buffer.to_utf16(self.buffer.selection()),
            reversed: self.buffer.reversed(),
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.buffer.marked().map(|r| self.buffer.to_utf16(r))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.unmark();
        self.changed(cx);
    }
    fn replace_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editable() {
            return;
        }
        let text = self.normalize(text);
        self.buffer.replace(r, &text, true);
        self.changed(cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        r: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editable() {
            return;
        }
        let text = self.normalize(text);
        self.buffer.compose(r, &text, selected);
        self.changed(cx);
    }
    fn bounds_for_range(
        &mut self,
        r: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let r = self.buffer.from_utf16(r);
        let (offset, line, b) = self
            .lines
            .iter()
            .rev()
            .find(|(offset, _, _)| *offset <= r.start)?;
        let start = (r.start - offset).min(line.text.len());
        let end = (r.end - offset).min(line.text.len());
        Some(Bounds::from_corners(
            point(b.left() + line.x_for_index(start), b.top()),
            point(b.left() + line.x_for_index(end), b.bottom()),
        ))
    }
    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.buffer.utf16(self.at(p)))
    }
}
struct InputElement {
    input: Entity<TextInput>,
}
struct InputPaint {
    lines: Vec<(usize, ShapedLine, Bounds<Pixels>)>,
    quads: Vec<PaintQuad>,
    caret: Option<PaintQuad>,
}
impl IntoElement for InputElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for InputElement {
    type RequestLayoutState = ();
    type PrepaintState = InputPaint;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        w: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let input = self.input.read(cx);
        let count = if input.options.multiline {
            input.buffer.text().split('\n').count()
        } else {
            1
        };
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = (w.line_height() * count as f32).into();
        (w.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        w: &mut Window,
        cx: &mut App,
    ) -> InputPaint {
        let input = self.input.read(cx);
        let text = input.buffer.text();
        let style = w.text_style();
        let height = w.line_height();
        let font_size = style.font_size.to_pixels(w.rem_size());
        let mut result = InputPaint {
            lines: vec![],
            quads: vec![],
            caret: None,
        };
        let mut offset = 0;
        for (row, part) in text.split('\n').enumerate() {
            let placeholder = text.is_empty();
            let display: SharedString = if placeholder {
                input.options.placeholder.clone()
            } else {
                part.to_string().into()
            };
            let mut runs = vec![];
            let mark = input.buffer.marked();
            let a = mark
                .as_ref()
                .map_or(0, |r| r.start.saturating_sub(offset).min(part.len()));
            let b = mark
                .as_ref()
                .map_or(0, |r| r.end.saturating_sub(offset).min(part.len()));
            for (len, marked) in [
                (a, false),
                (b.saturating_sub(a), true),
                (display.len().saturating_sub(b), false),
            ] {
                if len > 0 {
                    runs.push(TextRun {
                        len,
                        font: style.font(),
                        color: style.color,
                        background_color: None,
                        underline: marked.then_some(UnderlineStyle {
                            color: Some(style.color),
                            thickness: px(1.),
                            wavy: false,
                        }),
                        strikethrough: None,
                    });
                }
            }
            let line = w.text_system().shape_line(display, font_size, &runs, None);
            let origin = point(bounds.left(), bounds.top() + height * row as f32);
            let lb = Bounds::new(origin, size(bounds.size.width, height));
            let sel = input.buffer.selection();
            let start = sel.start.saturating_sub(offset).min(part.len());
            let end = sel.end.saturating_sub(offset).min(part.len());
            if end > start {
                result.quads.push(fill(
                    Bounds::from_corners(
                        point(lb.left() + line.x_for_index(start), lb.top()),
                        point(lb.left() + line.x_for_index(end), lb.bottom()),
                    ),
                    rgba(0x4488ff55),
                ));
            }
            let caret = input.buffer.caret();
            if caret >= offset && caret <= offset + part.len() {
                result.caret = Some(fill(
                    Bounds::new(
                        point(lb.left() + line.x_for_index(caret - offset), lb.top()),
                        size(px(1.), height),
                    ),
                    style.color,
                ));
            }
            result.lines.push((offset, line, lb));
            offset += part.len() + 1;
        }
        result
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        paint: &mut InputPaint,
        w: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.input.read(cx).focus_handle.clone();
        if self.input.read(cx).editable() {
            w.handle_input(
                &focus,
                ElementInputHandler::new(bounds, self.input.clone()),
                cx,
            );
        }
        for q in paint.quads.drain(..) {
            w.paint_quad(q);
        }
        for (_, line, b) in &paint.lines {
            let _ = line.paint(b.origin, w.line_height(), w, cx);
        }
        if focus.is_focused(w) {
            if let Some(caret) = paint.caret.take() {
                w.paint_quad(caret);
            }
        }
        let lines = std::mem::take(&mut paint.lines);
        self.input.update(cx, |input, _| input.lines = lines);
    }
}
impl Render for TextInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .key_context(if self.options.access == InputAccess::Disabled {
                "IraDisabledInput"
            } else if self.options.multiline {
                "IraNativeInput IraNativeEditor"
            } else {
                "IraNativeInput"
            })
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::quit))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::traverse))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, w, cx| {
                    if this.options.access == InputAccess::Disabled {
                        return;
                    }
                    w.focus(&this.focus_handle);
                    let b = this.at(event.position);
                    this.buffer.move_to(b, event.modifiers.shift);
                    this.selecting = true;
                    this.changed(cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.selecting && this.options.access != InputAccess::Disabled {
                    this.buffer.move_to(this.at(event.position), true);
                    this.changed(cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| this.selecting = false),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| this.selecting = false),
            )
            .child(InputElement { input: cx.entity() })
    }
}
impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
