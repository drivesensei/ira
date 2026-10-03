#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use ira_desktop::{
    actions,
    components::text_input,
    platform::geometry::{self, Geometry, Writer},
    runtime::Runtime,
    views::{Desktop, accessibility_retirement::Retirement},
};
struct Session {
    runtime: Runtime,
    next_window: u64,
    geometry: Writer,
    opening: bool,
}
impl gpui::Global for Session {}
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
            let weak = entity.downgrade();
            window.on_window_should_close(cx, move |_, cx| {
                ira_desktop::lifecycle_trace("OS window should-close callback");
                let _ = weak.update(cx, |this, _| this.close());
                true
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
    app.run(move |cx| {
        cx.set_global(owned_queue.clone());
        let pump = owned_queue.clone();
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
                if cx.update(|_| pump.pump(64)).is_err() {
                    break;
                }
            }
        })
        .detach();
        actions::register(cx);
        text_input::register(cx);
        cx.set_global(Session {
            runtime: Runtime::start_with_fonts(0, cx.text_system().clone()),
            next_window: 1,
            geometry: Writer::new(geometry::path()),
            opening: false,
        });
        open_main_window(cx)
    });
    keeper.retain_at_process_exit();
}
