use gpui::{EntityInputHandler, TestAppContext};
use ira_native_input_validation::text_input::{
    self, InputAccess, InputEditError, InputEvent, InputEventKind, InputOptions, TextInput,
};
use std::{cell::RefCell, rc::Rc};

#[gpui::test]
fn accessibility_value_edit_preserves_undo_and_stamped_event(cx: &mut TestAppContext) {
    cx.update(text_input::register);
    let (input, cx) = cx.add_window_view(|_, cx| {
        TextInput::new(
            "old 😀e\u{301}",
            InputOptions {
                document_generation: 44,
                focus_generation: 9,
                ..Default::default()
            },
            cx,
        )
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let output = events.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&input, move |_, e: &InputEvent, _| {
            output.borrow_mut().push(e.clone())
        })
    });
    input.update_in(cx, |i, w, cx| {
        i.focus(w, cx);
        i.edit_value("new 🦀e\u{301}", w, cx).unwrap();
    });
    assert_eq!(events.borrow().len(), 1);
    let event = events.borrow()[0].clone();
    assert_eq!(event.stamp.document_generation, 44);
    assert_eq!(event.stamp.focus_generation, 9);
    assert_eq!(event.stamp.value_revision, 1);
    assert!(
        matches!(event.kind,InputEventKind::Changed(s) if s.text=="new 🦀e\u{301}" && s.marked.is_none())
    );
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(
        input.read_with(cx, |i, _| i.snapshot().text),
        "old 😀e\u{301}"
    );
    cx.simulate_keystrokes("cmd-shift-z");
    assert_eq!(
        input.read_with(cx, |i, _| i.snapshot().text),
        "new 🦀e\u{301}"
    );
}

#[gpui::test]
fn accessibility_value_commits_composition_as_separate_undo_boundary(cx: &mut TestAppContext) {
    cx.update(text_input::register);
    let (input, cx) = cx.add_window_view(|_, cx| TextInput::new("a", InputOptions::default(), cx));
    input.update_in(cx, |i, w, cx| {
        i.focus(w, cx);
        i.replace_and_mark_text_in_range(None, "😀", None, w, cx);
        assert!(i.snapshot().marked.is_some());
        i.edit_value("AX", w, cx).unwrap();
        assert!(i.snapshot().marked.is_none());
    });
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(input.read_with(cx, |i, _| i.snapshot().text), "a😀");
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(input.read_with(cx, |i, _| i.snapshot().text), "a");
}

#[gpui::test]
fn accessibility_selection_rejects_invalid_unicode_without_mutation(cx: &mut TestAppContext) {
    let (input, cx) =
        cx.add_window_view(|_, cx| TextInput::new("a😀e\u{301}z", InputOptions::default(), cx));
    input.update_in(cx, |i, w, cx| {
        let before = i.snapshot();
        for range in [std::ops::Range { start: 3, end: 2 }, 0..99] {
            assert_eq!(
                i.set_selection_utf16(range, false, w, cx),
                Err(InputEditError::InvalidRange)
            );
            assert_eq!(i.snapshot(), before);
        }
        for range in [2..3, 3..4, 4..4] {
            assert_eq!(
                i.set_selection_utf16(range, false, w, cx),
                Err(InputEditError::InvalidBoundary)
            );
            assert_eq!(i.snapshot(), before);
        }
        i.set_selection_utf16(1..5, true, w, cx).unwrap();
        assert_eq!(i.snapshot().selection, 1..8);
        assert!(i.snapshot().reversed);
        let selected = i.selected_text_range(false, w, cx).unwrap();
        assert_eq!(selected.range, 1..5);
        assert!(selected.reversed);
    });
}

#[gpui::test]
fn accessibility_selection_is_eventful_without_content_revision_or_dirty_edit(
    cx: &mut TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|_, cx| {
        TextInput::new(
            "😀a",
            InputOptions {
                document_generation: 12,
                focus_generation: 4,
                ..Default::default()
            },
            cx,
        )
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let output = events.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&input, move |_, e: &InputEvent, _| {
            output.borrow_mut().push(e.clone())
        })
    });
    input.update_in(cx, |i, w, cx| {
        i.set_selection_utf16(0..2, false, w, cx).unwrap()
    });
    assert_eq!(events.borrow().len(), 1);
    let event = events.borrow()[0].clone();
    assert_eq!(event.stamp.value_revision, 0);
    assert_eq!(event.stamp.document_generation, 12);
    assert_eq!(event.stamp.focus_generation, 4);
    assert!(
        matches!(event.kind,InputEventKind::Changed(s) if s.text=="😀a" && s.selection== (0..4))
    );
}

#[gpui::test]
fn accessibility_readonly_value_refusal_selection_allowed_and_disabled_refuses_all(
    cx: &mut TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|_, cx| {
        TextInput::new(
            "😀a",
            InputOptions {
                access: InputAccess::ReadOnly,
                ..Default::default()
            },
            cx,
        )
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let output = events.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&input, move |_, e: &InputEvent, _| {
            output.borrow_mut().push(e.clone())
        })
    });
    input.update_in(cx, |i, w, cx| {
        let before = i.snapshot();
        assert_eq!(i.edit_value("bad", w, cx), Err(InputEditError::ReadOnly));
        assert_eq!(i.snapshot(), before);
        i.set_selection_utf16(0..2, false, w, cx).unwrap();
        assert_eq!(i.snapshot().selection, 0..4);
        i.set_access(InputAccess::Disabled, cx);
        let before = i.snapshot();
        assert_eq!(i.edit_value("bad", w, cx), Err(InputEditError::Disabled));
        assert_eq!(
            i.set_selection_utf16(0..2, false, w, cx),
            Err(InputEditError::Disabled)
        );
        assert_eq!(i.snapshot(), before);
        assert_eq!(i.stamp().value_revision, 0);
    });
    assert_eq!(
        events.borrow().len(),
        1,
        "only permitted selection emits an event"
    );
}

#[gpui::test]
fn accessibility_normalization_noop_and_selection_commit_composition(cx: &mut TestAppContext) {
    cx.update(text_input::register);
    let (input, cx) = cx.add_window_view(|_, cx| TextInput::new("a", InputOptions::default(), cx));
    input.update_in(cx, |i, w, cx| {
        i.focus(w, cx);
        i.edit_value("a\r\n", w, cx).unwrap();
        assert_eq!(i.stamp().value_revision, 0);
        i.replace_and_mark_text_in_range(None, "😀", None, w, cx);
        let stamp = i.stamp();
        let composing = i.snapshot();
        assert_eq!(
            i.set_selection_utf16(1..2, false, w, cx),
            Err(InputEditError::InvalidBoundary)
        );
        assert_eq!(i.snapshot(), composing);
        i.set_selection_utf16(0..1, false, w, cx).unwrap();
        assert_eq!(i.stamp(), stamp);
        assert!(i.snapshot().marked.is_none());
    });
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(input.read_with(cx, |i, _| i.snapshot().text), "a");
    let (multiline, cx) = cx.add_window_view(|_, cx| {
        TextInput::new(
            "",
            InputOptions {
                multiline: true,
                ..Default::default()
            },
            cx,
        )
    });
    multiline.update_in(cx, |i, w, cx| {
        i.edit_value("a\r\nb\rc", w, cx).unwrap();
        assert_eq!(i.snapshot().text, "a\nb\nc");
    });
}
