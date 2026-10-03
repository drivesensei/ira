#![allow(clippy::useless_conversion)]
use ira_accessibility_validation::accessibility::{Rejection, model::*};
use ira_core::{
    application::App,
    domain::data::Folder,
    input::InputContext,
    observable::{Row, Snapshot},
    services::list_files::FEntry,
};
use std::{path::PathBuf, sync::Arc};
fn fixture(count: usize) -> Snapshot {
    let mut s = App::default().snapshot();
    s.window_generation = 17;
    s.focus_generation = 3;
    s.document_generation = 9;
    s.revision = 1;
    s.input_context = InputContext::Pane(0);
    s.panes[0].folder = Some(Folder::new("fixture".into(), "/synthetic".into(), 'f'));
    s.panes[0].listing_generation = 5;
    s.panes[0].listing_settled = true;
    s.panes[0].rows = (0..count)
        .map(|i| Row {
            underlying_index: i,
            entry: FEntry {
                path: format!("/synthetic/{i}"),
                label: i.to_string(),
                is_dir: false,
                size: 3,
                modified: None,
            },
            selected: false,
            deleting: false,
        })
        .collect::<Vec<_>>()
        .into();
    s
}
fn key(s: &Snapshot) -> RequestKey {
    RequestKey {
        window_generation: s.window_generation,
        semantic_revision: s.revision,
        document_generation: s.document_generation,
        focus_generation: s.focus_generation,
        native_text_revision: 0,
        host_focus_revision: 0,
    }
}
fn frame(
    semantic: Arc<PreparedSemantic>,
    base: Option<&PreparedFrame>,
    seq: u64,
    layout: LayoutSnapshot,
) -> PreparedFrame {
    let k = FrameKey {
        request: semantic.key,
        request_seq: seq,
        layout_revision: layout.revision,
    };
    prepare_frame(base, semantic, k, layout, seq).unwrap()
}
fn layout(s: &Snapshot, revision: u64) -> LayoutSnapshot {
    LayoutSnapshot {
        window_generation: s.window_generation,
        semantic_revision: s.revision,
        revision,
        ..Default::default()
    }
}
#[test]
fn full_10k_100k_index_and_sparse_hit_visits_stay_visible_bounded() {
    for count in [10_000, 100_000] {
        let s = fixture(count);
        let mut model = AccessibilityModel::default();
        let p = Arc::new(model.prepare(&s, None, key(&s), None).unwrap());
        assert_eq!(
            p.tree
                .nodes
                .values()
                .filter(|n| matches!(n.target, Target::Entry { .. }))
                .count(),
            count
        );
        let mut l = layout(&s, 1);
        for i in 0..40 {
            let id = p
                .index
                .lookup(
                    &Target::Entry {
                        pane: 0,
                        path: PathBuf::from(format!("/synthetic/{i}")),
                        listing_generation: 5,
                    },
                    Some(Role::Row),
                    Some(Capability::Selection),
                )
                .unwrap();
            let r = Rect {
                x: 0.,
                y: i as f64 * 10.,
                width: 100.,
                height: 10.,
            };
            assert!(l.record(id, r, r));
        }
        let offscreen = p
            .index
            .lookup(
                &Target::Entry {
                    pane: 0,
                    path: PathBuf::from(format!("/synthetic/{}", count - 1)),
                    listing_generation: 5,
                },
                None,
                None,
            )
            .unwrap();
        assert!(p.tree.nodes.contains_key(&offscreen));
        let f = frame(p, None, 1, l);
        assert_eq!(f.geometry.nodes.len(), 40);
        assert_eq!(f.geometry.hit_test_with_visits(-1., -1.), (None, 40));
        assert!(f.geometry.hit_test(1., 1.).is_some());
        println!("logical_rows={count} visible=40 miss_visits=40 index_offscreen=true");
    }
}
#[test]
fn duplicate_places_and_refiltered_entries_keep_indexed_identity() {
    let mut s = fixture(2);
    s.bookmarks = Some(vec![
        Folder::new("a".into(), "/same".into(), 'a'),
        Folder::new("b".into(), "/same".into(), 'a'),
    ]);
    let mut m = AccessibilityModel::default();
    let p = m.prepare(&s, None, key(&s), None).unwrap();
    let target = Target::Entry {
        pane: 0,
        path: "/synthetic/0".into(),
        listing_generation: 5,
    };
    let id = p.index.lookup(&target, None, None).unwrap();
    let place = |occurrence| Target::Place {
        kind: PlaceKind::Bookmark,
        path: "/same".into(),
        shortcut: 'a',
        occurrence,
    };
    assert_ne!(
        p.index.lookup(&place(0), None, None),
        p.index.lookup(&place(1), None, None)
    );
    let rows = s.panes[0].rows.clone();
    s.panes[0].rows = Vec::new().into();
    s.revision += 1;
    assert!(
        m.prepare(&s, None, key(&s), None)
            .unwrap()
            .index
            .lookup(&target, None, None)
            .is_none()
    );
    s.panes[0].rows = rows;
    s.revision += 1;
    assert_eq!(
        m.prepare(&s, None, key(&s), None)
            .unwrap()
            .index
            .lookup(&target, None, None),
        Some(id)
    );
}
#[test]
fn notification_diff_uses_last_installed_not_discarded_computation() {
    let mut s = fixture(2);
    let mut m = AccessibilityModel::default();
    let one = frame(
        Arc::new(m.prepare(&s, None, key(&s), None).unwrap()),
        None,
        1,
        layout(&s, 1),
    );
    let mut rows = s.panes[0].rows.to_vec();
    rows[0].selected = true;
    s.panes[0].rows = rows.into();
    s.revision += 1;
    let two = frame(
        Arc::new(m.prepare(&s, None, key(&s), None).unwrap()),
        Some(&one),
        2,
        layout(&s, 2),
    );
    assert_eq!(two.notifications.selected_items.len(), 1);
    let mut rows = s.panes[0].rows.to_vec();
    rows[0].selected = false;
    s.panes[0].rows = rows.into();
    s.revision += 1;
    let three = frame(
        Arc::new(m.prepare(&s, None, key(&s), None).unwrap()),
        Some(&one),
        3,
        layout(&s, 3),
    );
    assert_eq!(three.base_publication_seq, 1);
    assert!(three.notifications.selected_items.is_empty());
    assert!(three.notifications.selected_parents.is_empty());
}
#[test]
fn mismatched_window_semantic_text_and_layout_fail_closed() {
    let s = fixture(1);
    let mut m = AccessibilityModel::default();
    let p = Arc::new(m.prepare(&s, None, key(&s), None).unwrap());
    let k = FrameKey {
        request: p.key,
        request_seq: 1,
        layout_revision: 1,
    };
    let mut l = layout(&s, 1);
    l.window_generation += 1;
    assert!(matches!(
        prepare_frame(None, p.clone(), k, l, 1),
        Err(Rejection::Stale)
    ));
    let mut wrong = k;
    wrong.request.native_text_revision += 1;
    assert!(matches!(
        prepare_frame(None, p.clone(), wrong, layout(&s, 1), 1),
        Err(Rejection::Stale)
    ));
    assert!(matches!(
        prepare_frame(None, p, k, layout(&s, 2), 1),
        Err(Rejection::Stale)
    ));
}
#[test]
fn compatibility_baseline_preserves_real_installed_state() {
    let s = fixture(1);
    let t = Arc::new(AccessibilityModel::default().project(&s, None, &layout(&s, 4)));
    let baseline = PreparedFrame::compatibility_baseline(t.clone(), key(&s)).unwrap();
    assert_eq!(baseline.publication_seq, 0);
    assert_eq!(baseline.key.layout_revision, 4);
    assert!(Arc::ptr_eq(&baseline.semantic.tree, &t));
    let f = frame(baseline.semantic.clone(), Some(&baseline), 1, layout(&s, 4));
    assert!(!f.notifications.structure_changed);
    assert!(!f.notifications.layout_changed);
}
#[test]
fn transactional_install_rejects_unacked_base_without_changing_cache_or_sink() {
    use ira_accessibility_validation::accessibility::ActionSink;
    use std::cell::Cell;
    let s = fixture(1);
    let t = Arc::new(AccessibilityModel::default().project(&s, None, &layout(&s, 0)));
    let (sink, _rx) = ActionSink::channel(t.clone(), 2);
    let baseline = PreparedFrame::compatibility_baseline(t.clone(), key(&s)).unwrap();
    let one = Arc::new(frame(
        baseline.semantic.clone(),
        Some(&baseline),
        1,
        layout(&s, 1),
    ));
    let cache = Cell::new(0);
    let retired = sink
        .install_prepared(&one, one.key, || cache.set(1))
        .unwrap();
    assert!(Arc::ptr_eq(&retired.tree, &t));
    assert_eq!(cache.get(), 1);
    let bad = Arc::new(frame(
        baseline.semantic.clone(),
        Some(&baseline),
        2,
        layout(&s, 2),
    ));
    assert!(matches!(
        sink.install_prepared(&bad, bad.key, || cache.set(2)),
        Err(Rejection::Stale)
    ));
    assert_eq!(cache.get(), 1);
    assert_eq!(sink.current_frame().unwrap().unwrap().publication_seq, 1);
    let two = Arc::new(frame(one.semantic.clone(), Some(&one), 2, layout(&s, 2)));
    let mut wrong = two.key;
    wrong.request.host_focus_revision += 1;
    assert!(matches!(
        sink.install_prepared(&two, wrong, || cache.set(2)),
        Err(Rejection::Stale)
    ));
    assert_eq!(cache.get(), 1);
    let retirement = sink
        .install_prepared(&two, two.key, || cache.set(2))
        .unwrap();
    assert_eq!(retirement.frame.unwrap().publication_seq, 1);
    assert_eq!(cache.get(), 2);
}
