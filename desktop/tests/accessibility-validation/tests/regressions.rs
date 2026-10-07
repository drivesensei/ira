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
fn duplicate_bookmark_slots_remain_distinct_nodes() {
    let mut s = fixture();
    s.bookmarks = Some(vec![
        Folder::new("first".into(), "/same/path".into(), 'a'),
        Folder::new("second".into(), "/same/path".into(), 'b'),
    ]);
    let t = tree(&s);
    let group = t.nodes.values().find(|n| n.name == "Bookmarks").unwrap();
    assert_eq!(group.children.len(), 2);
    assert_ne!(
        group.children[0], group.children[1],
        "distinct bookmark slots must not alias native element identity"
    );
}
#[test]
fn advertised_focusable_place_accepts_focus() {
    let mut s = fixture();
    s.bookmarks = Some(vec![Folder::new(
        "bookmark".into(),
        "/same/path".into(),
        'a',
    )]);
    let t = Arc::new(tree(&s));
    let place = t
        .nodes
        .values()
        .find(|n| matches!(n.target, Target::Place { .. }))
        .unwrap();
    assert!(place.focusable);
    let (sink, _rx) = ActionSink::channel(t.clone(), 1);
    assert_eq!(
        sink.try_dispatch(intent(&t, place.id, Action::Focus)),
        Ok(())
    );
}

#[test]
fn older_layout_cannot_roll_back_current_geometry() {
    let s = fixture();
    let mut model = AccessibilityModel::default();
    let t = model.project(&s, None, &LayoutSnapshot::default());
    let a = entry(&t, "a.txt");
    let mut layout = LayoutSnapshot {
        window_generation: s.window_generation,
        semantic_revision: s.revision,
        revision: 10,
        ..Default::default()
    };
    let clip = Rect {
        x: 0.,
        y: 0.,
        width: 300.,
        height: 300.,
    };
    layout.record(
        a,
        Rect {
            x: 0.,
            y: 100.,
            width: 20.,
            height: 10.,
        },
        clip,
    );
    let (sink, _rx) = ActionSink::channel(Arc::new(model.project(&s, None, &layout)), 1);
    assert_eq!(sink.current().unwrap().hit_test(1., 101.), Some(a));
    layout.revision = 9;
    layout.record(
        a,
        Rect {
            x: 0.,
            y: 0.,
            width: 20.,
            height: 10.,
        },
        clip,
    );
    let result = sink.publish(Arc::new(model.project(&s, None, &layout)));
    println!(
        "older layout result={result:?}; installed revision={}; hit old position={:?}; hit current position={:?}",
        sink.current().unwrap().layout_revision,
        sink.current().unwrap().hit_test(1., 1.),
        sink.current().unwrap().hit_test(1., 101.)
    );
    assert_eq!(result, Err(Rejection::Stale));
}
#[test]
fn distinct_bookmark_paths_control() {
    let mut s = fixture();
    s.bookmarks = Some(vec![
        Folder::new("first".into(), "/first".into(), 'a'),
        Folder::new("second".into(), "/second".into(), 'b'),
    ]);
    let t = tree(&s);
    let group = t.nodes.values().find(|n| n.name == "Bookmarks").unwrap();
    assert_ne!(group.children[0], group.children[1]);
}
#[test]
fn disabled_text_value_and_selection_rejected_control() {
    let mut s = fixture();
    s.input_context = InputContext::Editor;
    let text = NativeTextSnapshot {
        document_generation: s.document_generation,
        focus_generation: s.focus_generation,
        revision: 1,
        text: Arc::from("value"),
        selection_utf16: 0..0,
        marked_utf16: None,
        read_only: false,
        disabled: true,
        multiline: true,
    };
    let t = Arc::new(AccessibilityModel::default().project(
        &s,
        Some(&text),
        &LayoutSnapshot::default(),
    ));
    let id = t.focused.unwrap();
    let (sink, _rx) = ActionSink::channel(t.clone(), 2);
    assert_eq!(
        sink.try_dispatch(intent(&t, id, Action::SetValue("x".into()))),
        Err(Rejection::Disabled)
    );
    assert_eq!(
        sink.try_dispatch(intent(&t, id, Action::SetSelection(0..1))),
        Err(Rejection::Disabled)
    );
}

#[test]
fn duplicate_occurrences_preserve_names_geometry_and_reorder_identity() {
    let mut s = fixture();
    s.bookmarks = Some(vec![
        Folder::new("first".into(), "/same/path".into(), 'a'),
        Folder::new("second".into(), "/same/path".into(), 'b'),
        Folder::new("first duplicate".into(), "/same/path".into(), 'a'),
    ]);
    let mut model = AccessibilityModel::default();
    let first = model.project(&s, None, &LayoutSnapshot::default());
    let places: Vec<_> = first
        .nodes
        .values()
        .filter(|n| {
            matches!(
                n.target,
                Target::Place {
                    kind: PlaceKind::Bookmark,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(places.len(), 3);
    let find =
        |tree: &SemanticTree, name: &str| tree.nodes.values().find(|n| n.name == name).unwrap().id;
    let a = find(&first, "first (a)");
    let b = find(&first, "second (b)");
    let duplicate = find(&first, "first duplicate (a)");
    assert_ne!(a, duplicate);
    assert_ne!(first.nodes[&a].target, first.nodes[&duplicate].target);
    let mut layout = LayoutSnapshot {
        window_generation: s.window_generation,
        semantic_revision: s.revision,
        revision: 1,
        ..Default::default()
    };
    let clip = Rect {
        x: 0.,
        y: 0.,
        width: 100.,
        height: 100.,
    };
    for (id, x) in [(a, 0.), (b, 30.), (duplicate, 60.)] {
        assert!(layout.record(
            id,
            Rect {
                x,
                y: 0.,
                width: 20.,
                height: 20.
            },
            clip
        ));
    }
    let measured = model.project(&s, None, &layout);
    assert_eq!(measured.hit_test(1., 1.), Some(a));
    assert_eq!(measured.hit_test(61., 1.), Some(duplicate));
    s.bookmarks.as_mut().unwrap().swap(0, 1);
    s.revision += 1;
    let reordered = model.project(&s, None, &LayoutSnapshot::default());
    assert_eq!(find(&reordered, "first (a)"), a);
    assert_eq!(find(&reordered, "second (b)"), b);
    assert_eq!(find(&reordered, "first duplicate (a)"), duplicate);
}
#[test]
fn all_place_families_focus_without_navigation_and_reject_old_modal_focus() {
    let mut s = fixture();
    s.drives = Some(vec![Folder::new("drive".into(), "/drive".into(), 'd')]);
    s.folders = Some(vec![Folder::new("common".into(), "/common".into(), 'c')]);
    s.bookmarks = Some(vec![Folder::new(
        "bookmark".into(),
        "/bookmark".into(),
        'b',
    )]);
    let mut model = AccessibilityModel::default();
    let initial = model.project(&s, None, &LayoutSnapshot::default());
    for place in initial
        .nodes
        .values()
        .filter(|n| matches!(n.target, Target::Place { .. }))
    {
        assert!(place.capabilities.contains(&Capability::Focus));
        let (sink, rx) = ActionSink::channel(Arc::new(initial.clone()), 1);
        sink.try_dispatch(intent(&initial, place.id, Action::Focus))
            .unwrap();
        let dispatched = rx.try_next().unwrap().unwrap();
        assert_eq!(dispatched.action, Action::Focus);
        assert_eq!(dispatched.target, place.target);
        let focused =
            model.project_with_host_focus(&s, None, &LayoutSnapshot::default(), Some(place.id));
        assert_eq!(focused.focused, Some(place.id));
        assert_eq!(s.panes[0].cursor, Some(0));
        assert_eq!(
            s.panes[0].folder.as_ref().unwrap().path,
            "/synthetic-fixture"
        );
        let mut modal = s.clone();
        modal.input_context = InputContext::Confirmation;
        modal.revision += 1;
        let blocked =
            model.project_with_host_focus(&modal, None, &LayoutSnapshot::default(), Some(place.id));
        assert_ne!(blocked.focused, Some(place.id));
    }
    let invalid = NodeId {
        window: 999,
        serial: 999,
    };
    assert_eq!(
        model
            .project_with_host_focus(&s, None, &LayoutSnapshot::default(), Some(invalid))
            .focused,
        initial.focused
    );
}
#[test]
fn newer_semantic_revision_invalidates_old_layout_even_with_lower_layout_revision() {
    let mut s = fixture();
    let mut model = AccessibilityModel::default();
    let first = model.project(&s, None, &LayoutSnapshot::default());
    let a = entry(&first, "a.txt");
    let mut layout = LayoutSnapshot {
        window_generation: s.window_generation,
        semantic_revision: s.revision,
        revision: 10,
        ..Default::default()
    };
    let rect = Rect {
        x: 0.,
        y: 0.,
        width: 20.,
        height: 20.,
    };
    layout.record(a, rect, rect);
    let (sink, _rx) = ActionSink::channel(Arc::new(model.project(&s, None, &layout)), 1);
    s.revision += 1;
    sink.publish(Arc::new(model.project(
        &s,
        None,
        &LayoutSnapshot::default(),
    )))
    .unwrap();
    assert_eq!(sink.current().unwrap().hit_test(1., 1.), None);
}
