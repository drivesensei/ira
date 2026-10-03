//! Executes the exact Windows Navigate body with only native COM construction replaced.
//! Source equality prevents this host-side adapter probe drifting from production logic.
#![allow(non_snake_case, non_upper_case_globals, clippy::useless_conversion)]
use ira_accessibility_validation::accessibility::{
    ActionSink, Rejection,
    model::{self, *},
};
use ira_core::{
    application::App,
    domain::data::Folder,
    input::InputContext,
    observable::{Row, Snapshot},
    services::list_files::FEntry,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
#[path = "modal_navigation/support.rs"]
mod support;
use support::*;
struct Provider {
    state: Arc<State>,
    id: NodeId,
}
mod native_navigation_probe {
    use super::*;
    impl Provider {
        // Preserve token identity with the compiled Windows adapter body.
    #[rustfmt::skip]
        pub     fn Navigate(&self, direction: NavigateDirection) -> Result<IRawElementProviderFragment> {
        let cache = self.state.cache.try_lock().map_err(|_| unavailable())?;
        if self.state.is_closing() {
            return Err(unavailable());
        }
        let direction = match direction {
            NavigateDirection_Parent => super::model::NavigationDirection::Parent,
            NavigateDirection_FirstChild => super::model::NavigationDirection::FirstChild,
            NavigateDirection_LastChild => super::model::NavigationDirection::LastChild,
            NavigateDirection_NextSibling => super::model::NavigationDirection::NextSibling,
            NavigateDirection_PreviousSibling => super::model::NavigationDirection::PreviousSibling,
            _ => return Err(Error::from_hresult(E_INVALIDARG)),
        };
        let id = super::model::navigation_destination(
            &cache.tree,
            cache.prepared.as_ref().map(|frame| &*frame.semantic),
            self.id,
            direction,
        )
        .map_err(|error| {
            if error == Rejection::Unsupported {
                unsupported()
            } else {
                dispatch_error(error)
            }
        })?;
        drop(cache);
        self.state.fragment(id)
    }
    }
}
fn extract_method(source: &str) -> &str {
    let start = source
        .find("fn Navigate(")
        .expect("production Navigate method");
    let open = source[start..].find('{').unwrap() + start;
    let mut depth = 0;
    for (offset, c) in source[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[start..=open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unclosed Navigate method");
}
#[test]
fn executable_navigation_body_is_exact_production_source() {
    let production = extract_method(include_str!(
        "../../../src/platform/accessibility/windows.rs"
    ));
    let probe = extract_method(include_str!("modal_navigation.rs"));
    let compact = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    assert_eq!(
        compact(production),
        compact(probe),
        "regenerate mirror when production Navigate changes; test executes actual logic"
    );
}
#[test]
fn retained_background_provider_cannot_navigate_after_modal_activation() {
    for prepared in [false, true] {
        let mut s = fixture(3);
        let mut m = AccessibilityModel::default();
        let state = make_state(&s, &mut m, prepared);
        let tree = state.tree().unwrap();
        let pane = tree
            .nodes
            .values()
            .find(|n| n.role == Role::List)
            .unwrap()
            .id;
        let row = tree.nodes[&pane].children[1];
        s.input_context = InputContext::Confirmation;
        s.revision += 1;
        let modal = make_state(&s, &mut m, prepared);
        *state.cache.lock().unwrap() = Cached {
            tree: modal.tree().unwrap(),
            prepared: modal.cache.lock().unwrap().prepared.clone(),
        };
        for id in [pane, row] {
            for direction in 0..=4 {
                assert_eq!(
                    Provider {
                        state: state.clone(),
                        id
                    }
                    .Navigate(direction),
                    Err(Error::Unavailable),
                    "prepared={prepared}, retained={id:?}, direction={direction}"
                );
            }
        }
    }
}
#[test]
fn modal_and_root_navigation_stay_available_in_modal_scope() {
    for prepared in [false, true] {
        let mut s = fixture(3);
        s.input_context = InputContext::Confirmation;
        let state = make_state(&s, &mut AccessibilityModel::default(), prepared);
        let tree = state.tree().unwrap();
        let modal = tree.active_modal.unwrap();
        let root = Provider {
            state: state.clone(),
            id: tree.root,
        };
        assert_eq!(root.Navigate(NavigateDirection_FirstChild), Ok(modal));
        assert_eq!(root.Navigate(NavigateDirection_LastChild), Ok(modal));
        assert_eq!(
            Provider {
                state: state.clone(),
                id: modal
            }
            .Navigate(NavigateDirection_Parent),
            Ok(tree.root)
        );
        let children = &tree.nodes[&modal].children;
        assert_eq!(
            Provider {
                state: state.clone(),
                id: modal
            }
            .Navigate(NavigateDirection_FirstChild),
            Ok(children[0])
        );
        assert_eq!(
            Provider {
                state: state.clone(),
                id: modal
            }
            .Navigate(NavigateDirection_LastChild),
            Ok(*children.last().unwrap())
        );
        for id in children {
            assert!(tree.in_modal_scope(*id));
        }
    }
}
#[test]
fn offscreen_100k_navigation_preserves_complete_logical_tree_without_modal() {
    let s = fixture(100_000);
    let state = make_state(&s, &mut AccessibilityModel::default(), true);
    let tree = state.tree().unwrap();
    let pane = tree.nodes.values().find(|n| n.role == Role::List).unwrap();
    assert_eq!(pane.children.len(), 100_000);
    let last = *pane.children.last().unwrap();
    assert!(tree.nodes[&last].geometry.is_none());
    assert_eq!(
        Provider {
            state: state.clone(),
            id: pane.id
        }
        .Navigate(NavigateDirection_LastChild),
        Ok(last)
    );
    assert_eq!(
        Provider {
            state: state.clone(),
            id: last
        }
        .Navigate(NavigateDirection_PreviousSibling),
        Ok(pane.children[99_998])
    );
    assert_eq!(
        Provider {
            state: state.clone(),
            id: last
        }
        .Navigate(NavigateDirection_Parent),
        Ok(pane.id)
    );
}

// AppKit scalar resolver is pure Rust; ivars/Rc state are the same publication data.
struct MacState {
    tree: std::cell::RefCell<Arc<SemanticTree>>,
    attached: std::cell::Cell<bool>,
    sink: ActionSink,
}
impl MacState {
    fn is_attached(&self) -> bool {
        self.attached.get() && !self.sink.is_closing()
    }
}
struct MacIvars {
    state: std::rc::Weak<MacState>,
    id: NodeId,
}
struct MacProvider {
    ivars: MacIvars,
}
impl MacProvider {
    fn ivars(&self) -> &MacIvars {
        &self.ivars
    }
    fn node(&self) -> Option<Node> {
        let state = self.ivars().state.upgrade()?;
        if !state.is_attached() {
            return None;
        };

        state
            .tree
            .borrow()
            .query_node(self.ivars().id)
            .map(Node::clone_metadata)
    }
}
#[test]
fn executable_mac_metadata_body_is_exact_production_source() {
    let extract = |source: &str| {
        let start = source.find("fn node(&self)").unwrap();
        let open = source[start..].find('{').unwrap() + start;
        let mut depth = 0;
        for (i, c) in source[open..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return source[start..=open + i]
                            .chars()
                            .filter(|c| !c.is_whitespace())
                            .collect::<String>();
                    }
                }
                _ => {}
            }
        }
        panic!("unclosed node method");
    };
    assert_eq!(
        extract(include_str!("../../../src/platform/accessibility/macos.rs")),
        extract(include_str!("modal_navigation.rs"))
    );
}
#[test]
fn mac_retained_background_metadata_is_unavailable_during_modal() {
    let mut s = fixture(3);
    let mut m = AccessibilityModel::default();
    let old = m.project(&s, None, &LayoutSnapshot::default());
    let background = old
        .nodes
        .values()
        .find(|n| n.role == Role::List)
        .unwrap()
        .id;
    s.input_context = InputContext::Confirmation;
    s.revision += 1;
    let current = Arc::new(m.project(&s, None, &LayoutSnapshot::default()));
    let state = std::rc::Rc::new(MacState {
        tree: std::cell::RefCell::new(current.clone()),
        attached: std::cell::Cell::new(true),
        sink: ActionSink::channel(current.clone(), 1).0,
    });
    let retained = MacProvider {
        ivars: MacIvars {
            state: std::rc::Rc::downgrade(&state),
            id: background,
        },
    };
    assert!(
        retained.node().is_none(),
        "retained background label/value resolver remains unavailable during modal"
    );
    for id in [current.root, current.active_modal.unwrap()] {
        assert!(
            MacProvider {
                ivars: MacIvars {
                    state: std::rc::Rc::downgrade(&state),
                    id
                }
            }
            .node()
            .is_some()
        );
    }
}

#[test]
fn navigation_rejects_mixed_publication_and_closing_source() {
    let mut s = fixture(3);
    let mut m = AccessibilityModel::default();
    let initial = make_state(&s, &mut m, true);
    let previous = initial.cache.lock().unwrap().prepared.clone().unwrap();
    s.input_context = InputContext::Confirmation;
    s.revision += 1;
    let current = make_state(&s, &mut m, true);
    let tree = current.tree().unwrap();
    assert_eq!(
        model::navigation_destination(
            &tree,
            Some(&previous.semantic),
            tree.root,
            NavigationDirection::FirstChild
        ),
        Err(Rejection::Stale)
    );
    current.closing.store(true, Ordering::Release);
    assert_eq!(
        Provider {
            state: current,
            id: tree.root
        }
        .Navigate(NavigateDirection_FirstChild),
        Err(Error::Unavailable)
    );
}

#[test]
fn indexed_sibling_destination_cannot_escape_modal_even_if_index_is_stale() {
    let mut s = fixture(3);
    s.input_context = InputContext::Confirmation;
    let state = make_state(&s, &mut AccessibilityModel::default(), true);
    let tree = state.tree().unwrap();
    let mut prepared = PreparedSemantic::from_tree(
        tree.clone(),
        state
            .cache
            .lock()
            .unwrap()
            .prepared
            .as_ref()
            .unwrap()
            .key
            .request,
    )
    .unwrap();
    let background = tree
        .nodes
        .values()
        .find(|n| n.role == Role::List)
        .unwrap()
        .id;
    let modal = tree.active_modal.unwrap();
    let source = tree.nodes[&modal].children[1];
    prepared.siblings.insert(source, (Some(background), None));
    assert_eq!(
        model::navigation_destination(
            &tree,
            Some(&prepared),
            source,
            NavigationDirection::PreviousSibling
        ),
        Err(Rejection::Stale)
    );
}

#[test]
fn sink_close_after_attachment_disables_retained_native_callbacks() {
    let s = fixture(3);
    let state = make_state(&s, &mut AccessibilityModel::default(), true);
    let tree = state.tree().unwrap();
    let provider = Provider {
        state: state.clone(),
        id: tree.root,
    };
    assert!(provider.Navigate(NavigateDirection_FirstChild).is_ok());
    state.sink.close();
    assert!(
        !state.closing.load(Ordering::Acquire),
        "sink closure is independent of native teardown flag"
    );
    assert_eq!(
        provider.Navigate(NavigateDirection_FirstChild),
        Err(Error::Unavailable)
    );
    let mac = std::rc::Rc::new(MacState {
        tree: std::cell::RefCell::new(tree.clone()),
        attached: std::cell::Cell::new(true),
        sink: ActionSink::channel(tree.clone(), 1).0,
    });
    let retained = MacProvider {
        ivars: MacIvars {
            state: std::rc::Rc::downgrade(&mac),
            id: tree.root,
        },
    };
    assert!(retained.node().is_some());
    mac.sink.close();
    assert!(
        mac.attached.get(),
        "sink closure is independent of native teardown flag"
    );
    assert!(retained.node().is_none());
}
#[test]
fn callback_lifetime_predicates_match_native_source() {
    fn body<'a>(source: &'a str, needle: &str) -> &'a str {
        let start = source.find(needle).unwrap();
        let open = start + source[start..].find('{').unwrap();
        let end = open + source[open..].find('}').unwrap();
        &source[open..=end]
    }
    let compact = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    assert_eq!(
        compact(body(
            include_str!("../../../src/platform/accessibility/windows.rs"),
            "fn is_closing(&self)"
        )),
        compact(body(
            include_str!("modal_navigation/support.rs"),
            "fn is_closing(&self)"
        ))
    );
    assert_eq!(
        compact(body(
            include_str!("../../../src/platform/accessibility/macos.rs"),
            "fn is_attached(&self)"
        )),
        compact(body(
            include_str!("modal_navigation.rs"),
            "fn is_attached(&self)"
        ))
    );
}
