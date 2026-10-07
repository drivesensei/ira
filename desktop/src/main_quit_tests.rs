use super::*;
#[gpui::test]
fn pinned_platform_quit_stops_actor_and_acknowledges_temporary_persistence_drain(
    cx: &mut gpui::TestAppContext,
) {
    let fixture = std::env::temp_dir().join(format!(
        "ira-platform-quit-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&fixture).unwrap();
    let mut app = ira_core::application::App::default();
    app.state_path = Some(fixture.join("state"));
    app.bookmarks_path = Some(fixture.join("bookmarks"));
    let runtime = Runtime::with_factory(7, move || app);
    let witness = runtime.attach(7);
    let guard = witness.effect_guard(7);
    let retirement = Retirement::default();
    cx.update(|cx| {
        cx.set_global(retirement.clone());
        cx.set_global(Session {
            runtime,
            chooser: Default::default(),
            chooser_prompt: native_chooser_prompt,
            chooser_deferred: None,
            next_window: 8,
            geometry: Writer::new(None),
            opening: true,
            desktop: None,
            shutdown: None,
            shutdown_status: None,
            gate: Rc::new(RefCell::new(None)),
            native_request: None,
            native_error: Rc::new(RefCell::new(None)),
            quit_requested: false,
            approved: false,
        });
        register_platform_quit(cx);
        cx.shutdown();
    });
    assert!(retirement.quit_committed());
    assert!(!retirement.can_open());
    assert!(!guard.is_current());
    assert!(witness.is_stopping());
    // Late observer does not promise write completion in GPUI's100ms budget.
    // Original receipt remains owned and is polled by the ordinary coordinator.
    // Retain the task fixture until checked write receipts and worker retirement
    // prove cleanup safe; the legacy processing barrier is insufficient.
}

#[gpui::test]
fn global_quit_without_active_window_prepares_visible_checked_shutdown(
    cx: &mut gpui::TestAppContext,
) {
    let fixture = std::env::temp_dir().join(format!(
        "ira-platform-quit-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&fixture).unwrap();
    let mut app = ira_core::application::App::default();
    app.state_path = Some(fixture.join("state"));
    app.bookmarks_path = Some(fixture.join("bookmarks"));
    let runtime = Runtime::with_factory(7, move || app);
    let witness = runtime.attach(7);
    let guard = witness.effect_guard(7);
    let retirement = Retirement::default();
    cx.update(|cx| {
        cx.set_global(retirement.clone());
        cx.set_global(Session {
            runtime,
            chooser: Default::default(),
            chooser_prompt: native_chooser_prompt,
            chooser_deferred: None,
            next_window: 8,
            geometry: Writer::new(None),
            opening: true,
            desktop: None,
            shutdown: None,
            shutdown_status: None,
            gate: Rc::new(RefCell::new(None)),
            native_request: None,
            native_error: Rc::new(RefCell::new(None)),
            quit_requested: false,
            approved: false,
        });
        actions::register(cx);
        register_platform_quit(cx);
        assert!(cx.windows().is_empty());
        cx.dispatch_action(&actions::Quit);
        assert_eq!(
            cx.windows().len(),
            1,
            "Quit must prepare a visible status surface before stopping"
        );
        assert!(cx.global::<Session>().shutdown.is_some());
        assert!(cx.global::<Session>().runtime.is_stopping());
        // A second Quit retains the same surface and checked coordinator.
        cx.dispatch_action(&actions::Quit);
        assert_eq!(cx.windows().len(), 1);
    });
    assert!(retirement.quit_committed());
    assert!(!retirement.can_open());
    assert!(!guard.is_current());
    assert!(witness.is_stopping());
    // Fixture is retained: the actual actor owns its checked write receipt.
}
