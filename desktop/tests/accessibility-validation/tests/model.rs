// Keep fixtures compatible with both Vec and the agreed Arc<Vec> snapshot contract.
#![allow(clippy::useless_conversion)]
use ira_accessibility_validation::accessibility::{model::*, *};
use ira_core::{
    application::App,
    domain::data::Folder,
    input::InputContext,
    observable::{Row, Snapshot},
    services::list_files::FEntry,
};
use std::{path::PathBuf, sync::Arc};
fn fixture() -> Snapshot {
    // Default does not initialize or persist anything; snapshot-only, no real user-data reads/writes.
    let mut s = App::default().snapshot();
    s.window_generation = 17;
    s.focus_generation = 3;
    s.document_generation = 9;
    s.revision = 1;
    s.input_context = InputContext::Pane(0);
    s.panes[0].folder = Some(Folder::new(
        "fixture".into(),
        "/synthetic-fixture".into(),
        'f',
    ));
    s.panes[0].listing_generation = 5;
    s.panes[0].listing_settled = true;
    s.panes[0].cursor = Some(0);
    s.panes[0].rows = vec![row("a.txt", false), row("日本語.txt", true)].into();
    s.panes[0].selected_paths = vec![PathBuf::from("/synthetic-fixture/日本語.txt")].into();
    s
}
fn row(name: &str, selected: bool) -> Row {
    Row {
        underlying_index: 0,
        entry: FEntry {
            path: format!("/synthetic-fixture/{name}"),
            label: name.into(),
            is_dir: false,
            size: 3,
            modified: None,
        },
        selected,
        deleting: false,
    }
}
fn tree(s: &Snapshot) -> SemanticTree {
    AccessibilityModel::default().project(s, None, &LayoutSnapshot::default())
}
fn entry(t: &SemanticTree, name: &str) -> NodeId {
    t.nodes
        .values()
        .find(|n| matches!(&n.target,Target::Entry{path,..} if path.ends_with(name)))
        .unwrap()
        .id
}
fn intent(t: &SemanticTree, id: NodeId, action: Action) -> AccessibilityIntent {
    AccessibilityIntent {
        node: id,
        stamp: t.stamp,
        action,
    }
}
#[test]
fn stable_ids_across_sorting_and_refresh() {
    let mut m = AccessibilityModel::default();
    let mut s = fixture();
    let first = m.project(&s, None, &LayoutSnapshot::default());
    let id = entry(&first, "a.txt");
    s.panes[0].rows = s.panes[0]
        .rows
        .iter()
        .cloned()
        .rev()
        .collect::<Vec<_>>()
        .into();
    s.panes[0].listing_generation += 1;
    s.revision += 1;
    let after = m.project(&s, None, &LayoutSnapshot::default());
    assert_eq!(id, entry(&after, "a.txt"));
    assert!(matches!(
        &after.nodes[&id].target,
        Target::Entry {
            listing_generation: 6,
            ..
        }
    ));
}
#[test]
fn cursor_focus_differs_from_multi_selection() {
    let t = tree(&fixture());
    let a = entry(&t, "a.txt");
    let b = entry(&t, "日本語.txt");
    assert_eq!(t.focused, Some(a));
    assert!(!t.nodes[&a].selected);
    assert!(t.nodes[&b].selected);
}
#[test]
fn selected_true_is_idempotent_intent_not_toggle() {
    let t = Arc::new(tree(&fixture()));
    let b = entry(&t, "日本語.txt");
    let (sink, rx) = ActionSink::channel(t.clone(), 2);
    for _ in 0..2 {
        sink.try_dispatch(intent(&t, b, Action::SetSelected(true)))
            .unwrap();
    }
    for _ in 0..2 {
        let applied = rx.try_next().unwrap().unwrap();
        assert_eq!(applied.action, Action::SetSelected(true));
        assert!(matches!(applied.target, Target::Entry { .. }));
    }
}
#[test]
fn stale_target_rejected_on_enqueue_and_dequeue() {
    let mut m = AccessibilityModel::default();
    let mut s = fixture();
    let t = Arc::new(m.project(&s, None, &LayoutSnapshot::default()));
    let a = entry(&t, "a.txt");
    let (sink, rx) = ActionSink::channel(t.clone(), 2);
    sink.try_dispatch(intent(&t, a, Action::Activate)).unwrap();
    s.panes[0].listing_generation += 1;
    s.revision += 1;
    let newer = Arc::new(m.project(&s, None, &LayoutSnapshot::default()));
    sink.publish(newer).unwrap();
    assert_eq!(
        sink.try_dispatch(intent(&t, a, Action::Activate)),
        Err(Rejection::Stale)
    );
    assert_eq!(rx.try_next().unwrap().unwrap_err(), Rejection::Stale);
}
#[test]
fn modal_background_excluded_and_non_actionable() {
    let mut s = fixture();
    s.input_context = InputContext::Confirmation;
    let t = Arc::new(tree(&s));
    let a = entry(&t, "a.txt");
    let modal = t.active_modal.unwrap();
    assert_eq!(t.nodes[&t.root].children, vec![modal]);
    assert!(!t.nodes[&a].enabled);
    let (sink, _rx) = ActionSink::channel(t.clone(), 1);
    assert_eq!(
        sink.try_dispatch(intent(&t, a, Action::Activate)),
        Err(Rejection::Disabled)
    );
    assert!(
        sink.try_dispatch(intent(&t, modal, Action::Dismiss))
            .is_ok()
    );
}
#[test]
fn authoritative_host_draft_and_unicode_utf16_selection() {
    let mut s = fixture();
    s.input_context = InputContext::Editor;
    let text = NativeTextSnapshot {
        document_generation: 9,
        focus_generation: 3,
        revision: 10,
        text: Arc::from("a😀日本語"),
        selection_utf16: 1..3,
        marked_utf16: Some(3..6),
        read_only: false,
        disabled: false,
        multiline: true,
    };
    let t = Arc::new(AccessibilityModel::default().project(
        &s,
        Some(&text),
        &LayoutSnapshot::default(),
    ));
    let n = &t.nodes[&t.focused.unwrap()];
    assert_eq!(n.value.as_deref(), Some("a😀日本語"));
    assert_eq!(n.text_selection, Some(1..3));
    assert_eq!(n.marked_text, Some(3..6));
    let (sink, rx) = ActionSink::channel(t.clone(), 2);
    sink.try_dispatch(intent(&t, n.id, Action::SetSelection(1..3)))
        .unwrap();
    assert!(matches!(
        rx.try_next().unwrap().unwrap().action,
        Action::SetSelection(_)
    ));
    assert_eq!(
        sink.try_dispatch(intent(&t, n.id, Action::SetSelection(5..7))),
        Err(Rejection::InvalidRange)
    );
}
#[test]
fn logical_navigation_includes_virtual_rows_without_fake_geometry() {
    let mut s = fixture();
    s.panes[0].rows = (0..10000)
        .map(|i| row(&format!("{i}.txt"), false))
        .collect();
    let t = tree(&s);
    let list = t
        .nodes
        .values()
        .find(|n| matches!(n.target, Target::Pane(0)))
        .unwrap();
    assert_eq!(list.children.len(), 10000);
    assert!(
        list.children
            .iter()
            .all(|id| t.nodes[id].geometry.is_none())
    );
    assert_eq!(t.hit_test(0.0, 0.0), None);
}
#[test]
fn callbacks_after_close_rejected_and_old_window_ids_retired() {
    let mut m = AccessibilityModel::default();
    let mut s = fixture();
    let t = Arc::new(m.project(&s, None, &LayoutSnapshot::default()));
    let a = entry(&t, "a.txt");
    let (sink, rx) = ActionSink::channel(t.clone(), 1);
    sink.try_dispatch(intent(&t, a, Action::Focus)).unwrap();
    sink.close();
    sink.close();
    assert_eq!(
        sink.try_dispatch(intent(&t, a, Action::Focus)),
        Err(Rejection::Closing)
    );
    assert_eq!(rx.try_next().unwrap().unwrap_err(), Rejection::Closing);
    s.window_generation += 1;
    let newer = m.project(&s, None, &LayoutSnapshot::default());
    assert_ne!(entry(&newer, "a.txt"), a);
}
#[test]
fn bounds_clipped_and_invalidated_by_semantic_revision() {
    let mut m = AccessibilityModel::default();
    let mut s = fixture();
    let t = m.project(&s, None, &LayoutSnapshot::default());
    let a = entry(&t, "a.txt");
    let mut layout = LayoutSnapshot {
        window_generation: 17,
        semantic_revision: 1,
        revision: 7,
        ..Default::default()
    };
    assert!(layout.record(
        a,
        Rect {
            x: -5.0,
            y: 0.0,
            width: 20.0,
            height: 10.0
        },
        Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0
        }
    ));
    assert!(!layout.record(
        a,
        Rect {
            x: f64::NAN,
            y: 0.0,
            width: 10.0,
            height: 10.0
        },
        Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0
        }
    ));
    assert!(!layout.nodes.contains_key(&a));
    assert!(layout.record(
        a,
        Rect {
            x: -5.0,
            y: 0.0,
            width: 20.0,
            height: 10.0
        },
        Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0
        }
    ));
    let current = m.project(&s, None, &layout);
    assert_eq!(current.hit_test(1.0, 1.0), Some(a));
    assert_eq!(current.hit_test(-1.0, 1.0), None);
    s.revision += 1;
    let stale = m.project(&s, None, &layout);
    assert_eq!(stale.hit_test(1.0, 1.0), None);
}
#[test]
fn filtering_does_not_expose_hidden_file_names() {
    let mut m = AccessibilityModel::default();
    let mut s = fixture();
    let first = m.project(&s, None, &LayoutSnapshot::default());
    let visible = entry(&first, "a.txt");
    s.panes[0].rows.pop();
    s.panes[0].filter_query = Some("a".into());
    let filtered = m.project(&s, None, &LayoutSnapshot::default());
    assert!(filtered.nodes.values().all(|n| !n.name.contains("日本語")));
    assert_eq!(entry(&filtered, "a.txt"), visible);
    assert!(
        filtered
            .nodes
            .values()
            .any(|n| n.help.as_ref().is_some_and(|h| h.contains("1 selected")))
    );
}
#[test]
fn read_only_and_stale_draft_cannot_edit() {
    let mut s = fixture();
    s.input_context = InputContext::Rename;
    let mut text = NativeTextSnapshot {
        document_generation: 9,
        focus_generation: 3,
        revision: 1,
        text: Arc::from("draft"),
        selection_utf16: 0..5,
        marked_utf16: None,
        read_only: true,
        disabled: false,
        multiline: false,
    };
    let mut m = AccessibilityModel::default();
    let t = Arc::new(m.project(&s, Some(&text), &LayoutSnapshot::default()));
    let id = t.focused.unwrap();
    let (sink, _) = ActionSink::channel(t.clone(), 1);
    assert_eq!(
        sink.try_dispatch(intent(&t, id, Action::SetValue("new".into()))),
        Err(Rejection::Unsupported)
    );
    text.document_generation += 1;
    let stale = m.project(&s, Some(&text), &LayoutSnapshot::default());
    assert!(
        stale
            .nodes
            .values()
            .all(|n| !matches!(n.target, Target::Text { .. }))
    );
}
#[test]
fn nonblocking_backpressure_and_disconnect_are_explicit() {
    let t = Arc::new(tree(&fixture()));
    let a = entry(&t, "a.txt");
    let (sink, rx) = ActionSink::channel(t.clone(), 1);
    sink.try_dispatch(intent(&t, a, Action::Focus)).unwrap();
    assert_eq!(
        sink.try_dispatch(intent(&t, a, Action::Focus)),
        Err(Rejection::Backpressure)
    );
    drop(rx);
    assert_eq!(
        sink.try_dispatch(intent(&t, a, Action::Focus)),
        Err(Rejection::Closing)
    );
}

#[test]
fn split_surrogate_range_and_changed_host_revision_rejected() {
    let mut s = fixture();
    s.input_context = InputContext::Editor;
    let mut text = NativeTextSnapshot {
        document_generation: 9,
        focus_generation: 3,
        revision: 1,
        text: Arc::from("a😀z"),
        selection_utf16: 0..0,
        marked_utf16: None,
        read_only: false,
        disabled: false,
        multiline: true,
    };
    let mut m = AccessibilityModel::default();
    let t = Arc::new(m.project(&s, Some(&text), &LayoutSnapshot::default()));
    let id = t.focused.unwrap();
    let (sink, rx) = ActionSink::channel(t.clone(), 2);
    assert_eq!(
        sink.try_dispatch(intent(&t, id, Action::SetSelection(1..2))),
        Err(Rejection::InvalidRange)
    );
    sink.try_dispatch(intent(&t, id, Action::SetValue("new".into())))
        .unwrap();
    text.revision += 1;
    text.text = Arc::from("new draft");
    sink.publish(Arc::new(m.project(
        &s,
        Some(&text),
        &LayoutSnapshot::default(),
    )))
    .unwrap();
    assert_eq!(rx.try_next().unwrap().unwrap_err(), Rejection::Stale);
}
#[test]
fn action_sink_is_send_sync_and_close_rejects_foreign_thread_callbacks() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ActionSink>();
    let t = Arc::new(tree(&fixture()));
    let a = entry(&t, "a.txt");
    let (sink, rx) = ActionSink::channel(t.clone(), 1);
    let remote = sink.clone();
    std::thread::spawn(move || remote.try_dispatch(intent(&t, a, Action::Focus)))
        .join()
        .unwrap()
        .unwrap();
    sink.close();
    assert_eq!(rx.try_next().unwrap().unwrap_err(), Rejection::Closing);
}

#[cfg(target_os = "macos")]
#[test]
fn native_wrong_thread_rejects_before_reading_raw_handle() {
    use ira_accessibility_validation::accessibility::macos::{BridgeError, NativeBridge};
    use raw_window_handle::{HandleError, HasWindowHandle, WindowHandle};
    struct NeverRead;
    impl HasWindowHandle for NeverRead {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            panic!("wrong-thread guard must run before handle access")
        }
    }
    let tree = Arc::new(tree(&fixture()));
    let (sink, _rx) = ActionSink::channel(tree, 1);
    std::thread::spawn(move || {
        assert!(matches!(
            NativeBridge::attach(&NeverRead, sink),
            Err(BridgeError::WrongThread)
        ));
    })
    .join()
    .unwrap();
}

#[test]
fn older_host_draft_publication_cannot_rollback_current_text() {
    let mut initial = tree(&fixture());
    initial.stamp.text_revision = 7;
    let initial = Arc::new(initial);
    let (sink, _rx) = ActionSink::channel(initial.clone(), 1);
    let mut older = (*initial).clone();
    older.stamp.text_revision = 6;
    assert_eq!(sink.publish(Arc::new(older)), Err(Rejection::Stale));
    assert_eq!(sink.current().unwrap().stamp.text_revision, 7);
}
