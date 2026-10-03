use gpui::{EntityInputHandler, TestAppContext};
use ira_native_input_validation::text_input::{self, InputAccess, InputOptions, TextInput};
#[gpui::test]
fn native_entity_input_composition_and_readonly(cx: &mut TestAppContext) {
    cx.update(text_input::register);
    let (input, cx) =
        cx.add_window_view(|_, cx| TextInput::new("prefix suffix", InputOptions::default(), cx));
    input.update(cx, |_, cx| cx.notify());
    input.update_in(cx, |input, w, cx| {
        input.focus(w, cx);
        input.replace_and_mark_text_in_range(Some(7..7), "😀あ", Some(2..3), w, cx);
        assert_eq!(input.snapshot().selection, 11..14);
        assert_eq!(input.marked_text_range(w, cx), Some(7..10));
        input.replace_text_in_range(None, "😀う", w, cx);
        assert_eq!(input.snapshot().text, "prefix 😀うsuffix");
        assert_eq!(input.stamp().value_revision, 2);
        input.set_access(InputAccess::ReadOnly, cx);
        input.replace_text_in_range(None, "MUTATE", w, cx);
        assert_eq!(input.snapshot().text, "prefix 😀うsuffix");
    });
}
#[gpui::test]
fn simulated_editor_ctrl_a_plain_s_and_native_undo(cx: &mut TestAppContext) {
    cx.update(text_input::register);
    let (input, cx) = cx.add_window_view(|_, cx| {
        TextInput::new(
            "first\nsecond",
            InputOptions {
                multiline: true,
                ..Default::default()
            },
            cx,
        )
    });
    input.update_in(cx, |input, w, cx| input.focus(w, cx));
    cx.simulate_keystrokes("ctrl-a s");
    assert_eq!(
        input.read_with(cx, |input, _| input.snapshot().text),
        "first\nssecond"
    );
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(
        input.read_with(cx, |input, _| input.snapshot().text),
        "first\nsecond"
    );
    cx.simulate_keystrokes("cmd-shift-z");
    assert_eq!(
        input.read_with(cx, |input, _| input.snapshot().text),
        "first\nssecond"
    );
}
#[gpui::test]
fn native_clipboard_submit_cancel_quit_events_are_generation_stamped(cx: &mut TestAppContext) {
    use ira_native_input_validation::text_input::{InputEvent, InputEventKind};
    use std::{cell::RefCell, rc::Rc};
    cx.update(text_input::register);
    let (input, cx) = cx.add_window_view(|_, cx| {
        TextInput::new(
            "😀 hello",
            InputOptions {
                document_generation: 19,
                focus_generation: 7,
                ..Default::default()
            },
            cx,
        )
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let output = events.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&input, move |_, event: &InputEvent, _| {
            output.borrow_mut().push(event.clone())
        })
    });
    input.update_in(cx, |input, w, cx| input.focus(w, cx));
    cx.simulate_keystrokes("cmd-a cmd-c cmd-x cmd-v enter escape ctrl-c");
    assert_eq!(
        input.read_with(cx, |input, _| input.snapshot().text),
        "😀 hello"
    );
    let events = events.borrow();
    assert!(
        events
            .iter()
            .all(|e| e.stamp.document_generation == 19 && e.stamp.focus_generation == 7)
    );
    assert!(events.iter().any(
        |e| matches!(&e.kind,InputEventKind::Submit(s) if s=="😀 hello")
            && e.stamp.value_revision == 2
    ));
    assert!(events.iter().any(|e| e.kind == InputEventKind::Cancel));
    assert!(events.iter().any(|e| e.kind == InputEventKind::Quit));
}
#[gpui::test]
fn multiline_enter_save_and_disabled_input(cx: &mut TestAppContext) {
    use ira_native_input_validation::text_input::{InputEvent, InputEventKind};
    use std::{cell::RefCell, rc::Rc};
    cx.update(text_input::register);
    let (input, cx) = cx.add_window_view(|_, cx| {
        TextInput::new(
            "a",
            InputOptions {
                multiline: true,
                ..Default::default()
            },
            cx,
        )
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let output = events.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&input, move |_, event: &InputEvent, _| {
            output.borrow_mut().push(event.clone())
        })
    });
    input.update_in(cx, |input, w, cx| input.focus(w, cx));
    cx.simulate_keystrokes("enter s ctrl-s");
    assert!(
        events
            .borrow()
            .iter()
            .any(|e| matches!(&e.kind,InputEventKind::Save(s) if s=="a\ns")
                && e.stamp.value_revision == 2)
    );
    assert!(
        !events
            .borrow()
            .iter()
            .any(|e| matches!(e.kind, InputEventKind::Submit(_)))
    );
    input.update(cx, |input, cx| input.set_access(InputAccess::Disabled, cx));
    cx.simulate_keystrokes("s backspace cmd-a cmd-x");
    assert_eq!(
        input.read_with(cx, |input, _| input.snapshot().text),
        "a\ns"
    );
}
#[gpui::test]
fn search_arrow_keys_emit_oracle_navigation_without_changing_query(cx: &mut TestAppContext) {
    use ira_native_input_validation::text_input::{InputEvent, InputEventKind, SearchNavigation};
    use std::{cell::RefCell, rc::Rc};
    cx.update(text_input::register);
    let (input, cx) = cx.add_window_view(|_, cx| {
        TextInput::new(
            "query",
            InputOptions {
                search: true,
                ..Default::default()
            },
            cx,
        )
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let output = events.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&input, move |_, e: &InputEvent, _| {
            output.borrow_mut().push(e.kind.clone())
        })
    });
    input.update_in(cx, |input, w, cx| input.focus(w, cx));
    cx.simulate_keystrokes("right up down alt-up alt-down");
    assert_eq!(
        *events.borrow(),
        [
            SearchNavigation::Open,
            SearchNavigation::Previous,
            SearchNavigation::Next,
            SearchNavigation::Top,
            SearchNavigation::Bottom
        ]
        .map(InputEventKind::SearchNavigate)
    );
    assert_eq!(input.read_with(cx, |i, _| i.snapshot().text), "query");
}
