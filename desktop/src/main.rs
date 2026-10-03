#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, div, prelude::*, px, size};
use ira_desktop::{
    actions,
    components::text_input,
    platform::{
        application_quit::{ApplicationQuitGate, QuitDecision, QuitRequest},
        geometry::{self, Geometry, Writer},
        shutdown::{Coordinator, Status},
    },
    runtime::Runtime,
    views::{Desktop, accessibility_retirement::Retirement},
};
use std::{cell::RefCell, rc::Rc};
struct Session {
    runtime: Runtime,
    chooser: ira_desktop::platform::chooser::SingleFlight,
    chooser_prompt: ChooserPrompt,
    chooser_deferred: Option<ira_desktop::runtime::ChooserEvent>,
    next_window: u64,
    geometry: Writer,
    opening: bool,
    desktop: Option<gpui::WeakEntity<Desktop>>,
    shutdown: Option<Coordinator>,
    shutdown_status: Option<gpui::WeakEntity<ShutdownStatus>>,
    gate: Rc<RefCell<Option<ApplicationQuitGate>>>,
    native_request: Option<QuitRequest>,
    native_error: Rc<RefCell<Option<String>>>,
    quit_requested: bool,
    approved: bool,
}
impl gpui::Global for Session {}
// This surface has no filesystem/domain/native-provider initialization.
struct ShutdownStatus {
    message: String,
}
impl Render for ShutdownStatus {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_4()
            .child(self.message.clone())
            .child(
                div()
                    .id("retry-shutdown")
                    .child("Retry shutdown saves")
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(actions::RetryShutdown), cx)
                    }),
            )
    }
}
fn prepare_shutdown_surface(cx: &mut App) -> bool {
    if !cx.windows().is_empty() {
        return true;
    }
    let result = cx.open_window(WindowOptions::default(), |window, cx| {
        let status = cx.new(|_| ShutdownStatus {
            message: "Shutdown pending: waiting for checked write receipts".into(),
        });
        cx.update_global::<Session, _>(|session, _| {
            session.shutdown_status = Some(status.downgrade());
        });
        window.on_window_should_close(cx, |_, cx| {
            let session = cx.global::<Session>();
            session.shutdown.is_none() && !session.runtime.is_stopping()
        });
        status
    });
    match result {
        Ok(_) => true,
        Err(error) => {
            eprintln!("Shutdown not started: cannot create status window: {error}");
            false
        }
    }
}
fn start_checked_shutdown(cx: &mut App) {
    if cx.global::<Session>().shutdown.is_some() || !prepare_shutdown_surface(cx) {
        return;
    }
    cx.global::<Retirement>().begin_quit();
    cx.update_global::<Session, _>(|session, _| {
        session.opening = false;
        session.runtime.stop(&[]);
        session.shutdown = Some(Coordinator::new(&session.geometry));
    });
}
fn open_main_window(cx: &mut App) {
    if cx.global::<Retirement>().quit_committed() {
        return;
    }
    if let Some(window) = cx.windows().first().copied() {
        let _ = window.update(cx, |_, window, _| window.activate_window());
        return;
    }
    if !cx.global::<Retirement>().can_open() || cx.global::<Session>().opening {
        return;
    }
    cx.update_global::<Session, _>(|session, _| session.opening = true);
    let load = cx.global::<Session>().geometry.load();
    cx.spawn(async move |cx| {
        let geometry = cx
            .background_executor()
            .spawn(async move { load.recv().ok().flatten() })
            .await;
        let _ = cx.update(|cx| open_loaded_window(geometry, cx));
    })
    .detach();
}
fn open_loaded_window(geometry: Option<Geometry>, cx: &mut App) {
    if !cx.global::<Retirement>().can_open() {
        cx.update_global::<Session, _>(|session, _| session.opening = false);
        return;
    }
    ira_desktop::lifecycle_trace("native window opening");
    cx.update_global::<Session, _>(|session, _| session.opening = false);
    let runtime = cx.update_global::<Session, _>(|session, _| {
        let generation = session.next_window;
        session.next_window += 1;
        session.runtime.attach(generation)
    });
    let bounds = Bounds::centered(None, size(px(1080.), px(720.)), cx);
    let displays: Vec<_> = cx
        .displays()
        .iter()
        .map(|display| display.bounds())
        .collect();
    let bounds = geometry
        .map(|geometry| geometry.restore(&displays, bounds))
        .unwrap_or(WindowBounds::Windowed(bounds));
    let geometry_writer = cx.global::<Session>().geometry.clone();
    match cx.open_window(
        WindowOptions {
            window_bounds: Some(bounds),
            window_min_size: Some(size(px(320.), px(200.))),
            ..Default::default()
        },
        |window, cx| {
            let entity = cx.new(|cx| Desktop::new(runtime, geometry_writer, cx));
            entity.update(cx, |this, cx| this.focus_main(window, cx));
            cx.update_global::<Session, _>(|session, _| session.desktop = Some(entity.downgrade()));
            let weak = entity.downgrade();
            window.on_window_should_close(cx, move |_, cx| {
                ira_desktop::lifecycle_trace("OS window should-close callback");
                weak.update(cx, |this, _| {
                    if this.is_stopping() {
                        false
                    } else {
                        this.close();
                        true
                    }
                })
                .unwrap_or(true)
            });
            entity
        },
    ) {
        Ok(_) => {
            ira_desktop::lifecycle_trace("native window opened");
            cx.activate(true);
        }
        Err(error) => eprintln!("Failed to open IRA desktop window: {error}"),
    }
}
fn tick_shutdown(cx: &mut App) {
    let retirement = cx.global::<Retirement>().clone();
    cx.update_global::<Session, _>(|session, _| {
        if let Some(gate) = session.gate.borrow().as_ref() {
            match gate.take_request() {
                Ok(Some(request)) => session.native_request = Some(request),
                Ok(None) => {}
                Err(error) => {
                    *session.native_error.borrow_mut() = Some(format!(
                        "Shutdown paused: native termination gate {error:?}"
                    ))
                }
            }
        }
    });
    if cx.global::<Session>().native_request.is_some()
        || cx.global::<Session>().runtime.is_stopping()
    {
        start_checked_shutdown(cx);
    }
    let (desktop, message, approve, gate, request, errors) =
        cx.update_global::<Session, _>(|session, _| {
            let status = session
                .shutdown
                .as_mut()
                .map(|shutdown| shutdown.poll(&session.runtime));
            let status = if let Some(error) = session.native_error.borrow().clone() {
                Some(Status::PendingError(error))
            } else {
                status
            };
            let message = match &status {
                Some(Status::Waiting) => {
                    Some("Shutdown pending: waiting for checked write receipts".into())
                }
                Some(Status::PendingError(error)) => Some(error.clone()),
                _ => None,
            };
            let ready = status == Some(Status::Ready);
            let approve = ready && !session.approved && session.native_request.is_some();
            if approve {
                session.approved = true;
            }
            if ready && session.native_request.is_none() && !session.quit_requested {
                session.quit_requested = true;
            } else if session.native_request.is_none() {
                return (
                    session.desktop.clone(),
                    message,
                    false,
                    session.gate.clone(),
                    None,
                    session.native_error.clone(),
                );
            }
            (
                session.desktop.clone(),
                message,
                approve,
                session.gate.clone(),
                session.native_request,
                session.native_error.clone(),
            )
        });
    if let Some(message) = message {
        if let Some(status) = cx.global::<Session>().shutdown_status.clone() {
            let _ = status.update(cx, |this, cx| {
                this.message = message.clone();
                cx.notify();
            });
        }
        if let Some(desktop) = &desktop {
            let _ = desktop.update(cx, |this, cx| this.shutdown_feedback(message, cx));
        }
        return;
    }
    let ready = cx.global::<Session>().shutdown.is_some() && cx.global::<Session>().quit_requested;
    if approve || ready {
        // Keep the visible presentation until native approval actually succeeds.
        // GPUI window teardown will retire it after the irrevocable native reply.
        retirement.pump(64);
        if approve {
            // Native reply may synchronously reenter GPUI willTerminate. This
            // future runs AFTER the mutable App/Entity update borrow has ended.
            cx.foreground_executor()
                .spawn(async move {
                    if let (Some(gate), Some(request)) = (gate.borrow().as_ref(), request)
                        && let Err(error) = gate.reply(request, QuitDecision::Approve)
                    {
                        *errors.borrow_mut() =
                            Some(format!("Shutdown paused: native approval {error:?}"));
                    }
                })
                .detach();
        } else if !cx.global::<Session>().approved
            && cx.global::<Session>().native_request.is_none()
        {
            cx.quit();
        }
    }
}
type ChooserPrompt = fn(
    ira_desktop::platform::chooser::ChooserKind,
    &mut App,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = ira_desktop::platform::chooser::ChooserOutcome>>,
>;
fn native_chooser_prompt(
    kind: ira_desktop::platform::chooser::ChooserKind,
    cx: &mut App,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = ira_desktop::platform::chooser::ChooserOutcome>>,
> {
    let received = cx.prompt_for_paths(ira_desktop::platform::chooser::options(kind));
    Box::pin(async move {
        ira_desktop::platform::chooser::normalize_received(
            received
                .await
                .map_err(|e| e.to_string())
                .map(|r| r.map_err(|e| e.to_string())),
        )
    })
}
fn request_browse(kind: ira_desktop::platform::chooser::ChooserKind, cx: &mut App) {
    if cx.global::<Retirement>().quit_committed() {
        return;
    }
    cx.update_global::<Session, _>(|session, _| {
        if session.shutdown.is_some() || session.chooser.active().is_some() {
            return;
        }
        let epoch = session.runtime.attached_epoch();
        if epoch == 0 {
            return;
        }
        session
            .runtime
            .enqueue_current(ira_desktop::runtime::Command::Browse(kind));
    });
}
fn tick_chooser(cx: &mut App) {
    use ira_desktop::{
        platform::chooser,
        runtime::{ChooserEvent, Command},
    };
    cx.update_global::<Session, _>(|session, _| session.runtime.flush());
    for _ in 0..8 {
        let Some(event) = cx.update_global::<Session, _>(|s, _| {
            s.chooser_deferred
                .take()
                .or_else(|| s.runtime.try_chooser_event())
        }) else {
            break;
        };
        match event {
            ChooserEvent::Prompt(request) => {
                let admission = cx.global::<Session>().runtime.chooser_is_current(&request);
                if admission.is_none() {
                    cx.update_global::<Session, _>(|s, _| {
                        s.chooser_deferred = Some(ChooserEvent::Prompt(request))
                    });
                    break;
                }
                let acquired = cx.update_global::<Session, _>(|s, _| {
                    s.chooser.try_acquire(request.ticket).is_ok()
                });
                let current = acquired
                    && !cx.global::<Retirement>().quit_committed()
                    && admission == Some(true);
                // The actor admitted the currently attached originating window.
                let origin = cx.windows().first().copied();
                if !current || origin.is_none() {
                    cx.update_global::<Session, _>(|s, _| {
                        if acquired {
                            s.chooser.release(request.ticket);
                        }
                        s.runtime.enqueue_current(Command::ChooserResult {
                            ticket: request.ticket,
                            outcome: std::sync::Arc::new(chooser::ChooserOutcome::Canceled),
                        });
                    });
                    continue;
                }
                let activated = origin
                    .unwrap()
                    .update(cx, |_, window, _| window.activate_window())
                    .is_ok();
                let admission = cx.global::<Session>().runtime.chooser_is_current(&request);
                if activated && admission.is_none() {
                    cx.update_global::<Session, _>(|s, _| {
                        s.chooser.release(request.ticket);
                        s.chooser_deferred = Some(ChooserEvent::Prompt(request));
                    });
                    break;
                }
                if !activated || admission != Some(true) {
                    cx.update_global::<Session, _>(|s, _| {
                        s.chooser.release(request.ticket);
                        s.runtime.enqueue_current(Command::ChooserResult {
                            ticket: request.ticket,
                            outcome: std::sync::Arc::new(chooser::ChooserOutcome::Canceled),
                        });
                    });
                    continue;
                }
                let prompt = cx.global::<Session>().chooser_prompt;
                let received = prompt(request.ticket.kind, cx);
                cx.spawn(async move |cx| {
                    let outcome = received.await;
                    let _ = cx.update(|cx| {
                        cx.update_global::<Session, _>(|s, _| {
                            // Receiver ownership survives a vanished Desktop/window. Only exact release.
                            if s.chooser.release(request.ticket) {
                                s.runtime.enqueue_current(Command::ChooserResult {
                                    ticket: request.ticket,
                                    outcome: std::sync::Arc::new(outcome),
                                });
                            }
                        })
                    });
                })
                .detach();
            }
            ChooserEvent::RestoreFocus(permit) => {
                let current = cx
                    .global::<Session>()
                    .runtime
                    .chooser_focus_is_current(&permit);
                if current.is_none() {
                    cx.update_global::<Session, _>(|s, _| {
                        s.chooser_deferred = Some(ChooserEvent::RestoreFocus(permit))
                    });
                    break;
                }
                if !cx.global::<Retirement>().quit_committed()
                    && current == Some(true)
                    && let Some(desktop) = cx.global::<Session>().desktop.clone()
                    && let Some(window) = cx.windows().first().copied()
                {
                    let _ = window.update(cx, |_, window, cx| {
                        desktop.update(cx, |this, cx| this.restore_chooser_focus(window, cx))
                    });
                }
            }
        }
    }
}
fn register_platform_quit(cx: &mut App) {
    cx.on_action(|_: &actions::BrowseFile, cx| {
        request_browse(ira_desktop::platform::chooser::ChooserKind::File, cx)
    });
    cx.on_action(|_: &actions::BrowseFolder, cx| {
        request_browse(ira_desktop::platform::chooser::ChooserKind::Folder, cx)
    });
    // Global registration is required when the ordinary last window is closed.
    cx.on_action(|_: &actions::Quit, cx| start_checked_shutdown(cx));
    // Late GPUI observer is cancellation/retention fallback only. It never
    // fabricates successful checked receipts within the pinned100ms budget.
    cx.on_app_quit(|cx| {
        let retirement = cx.global::<Retirement>().clone();
        retirement.begin_quit();
        cx.update_global::<Session, _>(|session, _| {
            session.opening = false;
            session.runtime.stop(&[]);
            if session.shutdown.is_none() {
                session.shutdown = Some(Coordinator::new(&session.geometry));
            }
        });
        retirement.pump(64);
        async {}
    })
    .detach();
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("ira 0.1.21");
        return;
    }
    if args.iter().any(|arg| arg == "--check-terminal") {
        println!(
            "IRA desktop host: GPUI 0.2.2\nThis native host does not probe terminal graphics capabilities."
        );
        return;
    }
    let app = Application::new();
    app.on_reopen(open_main_window);
    let keeper = Retirement::default();
    let owned_queue = keeper.clone();
    let gate_keeper = Rc::new(RefCell::new(None));
    let owned_gate = gate_keeper.clone();
    app.run(move |cx| {
        #[cfg(target_os = "macos")]
        match ApplicationQuitGate::install() {
            Ok(gate) => *owned_gate.borrow_mut() = Some(gate),
            Err(error) => eprintln!("Native deferred quit unavailable: {error:?}"),
        }
        cx.set_global(owned_queue.clone());
        let pump = owned_queue.clone();
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
                if cx
                    .update(|cx| {
                        pump.pump(64);
                        tick_shutdown(cx);
                        tick_chooser(cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        actions::register(cx);
        cx.on_action(|_: &actions::RetryShutdown, cx| {
            cx.update_global::<Session, _>(|session, _| {
                if let Some(shutdown) = &mut session.shutdown {
                    shutdown.retry(&session.runtime);
                }
            });
        });
        text_input::register(cx);
        cx.set_global(Session {
            chooser: Default::default(),
            chooser_prompt: native_chooser_prompt,
            chooser_deferred: None,
            runtime: Runtime::start_with_fonts(0, cx.text_system().clone()),
            next_window: 1,
            geometry: Writer::new(geometry::path()),
            opening: false,
            desktop: None,
            shutdown: None,
            shutdown_status: None,
            gate: owned_gate,
            native_request: None,
            native_error: Rc::new(RefCell::new(None)),
            quit_requested: false,
            approved: false,
        });
        register_platform_quit(cx);
        open_main_window(cx)
    });
    if keeper.quit_committed() {
        std::mem::forget(gate_keeper);
    }
    keeper.retain_at_process_exit();
}

#[cfg(test)]
#[path = "main_quit_tests.rs"]
mod quit_tests;

#[cfg(test)]
#[path = "main_chooser_tests.rs"]
mod chooser_tests;
