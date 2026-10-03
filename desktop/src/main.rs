#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, prelude::*, px, size};
use ira_desktop::{actions, components::text_input, runtime::Runtime, views::Desktop};
struct Session {
    runtime: Runtime,
    next_window: u64,
}
impl gpui::Global for Session {}
fn open_main_window(cx: &mut App) {
    let runtime = cx.update_global::<Session, _>(|session, _| {
        let generation = session.next_window;
        session.next_window += 1;
        session.runtime.attach(generation)
    });
    let bounds = Bounds::centered(None, size(px(1080.), px(720.)), cx);
    match cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |window, cx| {
            let entity = cx.new(|cx| Desktop::new(runtime, cx));
            entity.update(cx, |this, cx| this.focus_main(window, cx));
            let weak = entity.downgrade();
            window.on_window_should_close(cx, move |_, cx| {
                let _ = weak.update(cx, |this, _| this.close());
                true
            });
            entity
        },
    ) {
        Ok(_) => cx.activate(true),
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
            "IRA desktop host: GPUI 0.2.2\nTerminal graphics protocols are replaced by native desktop previews."
        );
        return;
    }
    let app = Application::new();
    app.on_reopen(open_main_window);
    app.run(|cx| {
        actions::register(cx);
        text_input::register(cx);
        cx.set_global(Session {
            runtime: Runtime::start(0),
            next_window: 1,
        });
        open_main_window(cx)
    });
}
