use super::*;
use ira_desktop::platform::chooser::{ChooserKind, ChooserOutcome};
#[derive(Default)]
struct PromptState {
    calls: Vec<ChooserKind>,
    result: Option<ChooserOutcome>,
    waiter: Option<std::task::Waker>,
}
#[derive(Clone, Default)]
struct PromptFixture(Rc<RefCell<PromptState>>);
impl gpui::Global for PromptFixture {}
fn fixture_prompt(
    kind: ChooserKind,
    cx: &mut App,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ChooserOutcome>>> {
    let state = cx.global::<PromptFixture>().0.clone();
    state.borrow_mut().calls.push(kind);
    Box::pin(std::future::poll_fn(move |cx| {
        let mut state = state.borrow_mut();
        if let Some(result) = state.result.take() {
            std::task::Poll::Ready(result)
        } else {
            state.waiter = Some(cx.waker().clone());
            std::task::Poll::Pending
        }
    }))
}
#[gpui::test]
async fn registered_browse_actions_use_one_retained_receiver_and_shutdown_bypasses_picker(
    cx: &mut gpui::TestAppContext,
) {
    let directory = std::env::temp_dir().join(format!(
        "ira-chooser-session-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let mut app = ira_core::application::App::default();
    app.state_path = Some(directory.join("state"));
    app.bookmarks_path = Some(directory.join("bookmarks"));
    app.panes[0].listing_settled = true;
    let runtime = Runtime::with_factory(7, move || app);
    let witness = runtime.attach(7);
    let prompt = PromptFixture::default();
    cx.update(|cx| {
        cx.set_global(Retirement::default());
        cx.set_global(prompt.clone());
        cx.set_global(Session {
            runtime,
            chooser: Default::default(),
            chooser_prompt: fixture_prompt,
            chooser_deferred: None,
            next_window: 8,
            geometry: Writer::new(None),
            opening: false,
            desktop: None,
            shutdown: None,
            shutdown_status: None,
            gate: Rc::new(RefCell::new(None)),
            native_request: None,
            native_error: Rc::new(RefCell::new(None)),
            quit_requested: false,
            approved: false,
        });
        // Only a simulated test-platform text surface, never Desktop/provider/NSApp.
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| ShutdownStatus {
                message: "synthetic chooser owner".into(),
            })
        })
        .unwrap();
        actions::register(cx);
        register_platform_quit(cx);
        cx.dispatch_action(&actions::BrowseFile);
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while prompt.0.borrow().calls.is_empty() {
        cx.update(tick_chooser);
        assert!(std::time::Instant::now() < deadline);
        cx.background_executor
            .timer(std::time::Duration::from_millis(5))
            .await;
    }
    assert_eq!(prompt.0.borrow().calls, vec![ChooserKind::File]);
    cx.update(|cx| {
        cx.dispatch_action(&actions::BrowseFolder);
        tick_chooser(cx);
    });
    assert_eq!(prompt.0.borrow().calls.len(), 1);
    // Closing/reopening the simulated origin cannot release a pending receiver.
    witness.detach();
    cx.update(|cx| {
        let old = cx.windows()[0];
        old.update(cx, |_, window, _| window.remove_window())
            .unwrap();
        let _ = cx.global::<Session>().runtime.attach(8);
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| ShutdownStatus {
                message: "replacement synthetic owner".into(),
            })
        })
        .unwrap();
        cx.dispatch_action(&actions::BrowseFolder);
        tick_chooser(cx);
        assert!(cx.global::<Session>().chooser.active().is_some());
    });
    assert_eq!(prompt.0.borrow().calls.len(), 1);
    // Actual global Quit still enters checked coordinator while the receiver is pending.
    cx.update(|cx| {
        cx.dispatch_action(&actions::Quit);
    });
    // The active-window action runs during the preceding update's effect flush.
    cx.update(|cx| {
        assert!(cx.global::<Session>().shutdown.is_some());
        assert!(cx.global::<Session>().chooser.active().is_some());
    });
    assert!(witness.is_stopping());
    {
        let mut state = prompt.0.borrow_mut();
        state.result = Some(ChooserOutcome::Canceled);
        if let Some(waiter) = state.waiter.take() {
            waiter.wake();
        }
    }
    while cx.update(|cx| cx.global::<Session>().chooser.active().is_some()) {
        assert!(std::time::Instant::now() < deadline);
        cx.background_executor
            .timer(std::time::Duration::from_millis(5))
            .await;
    }
    // Retain task directory through actual actor/checked receipt completion.
}
