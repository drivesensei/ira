#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use gpui::{
    App, Application, Bounds, ClickEvent, Context, Render, Window, WindowBounds, WindowOptions,
    div, prelude::*, px, rgb, size,
};

struct IraDesktop {
    button_was_clicked: bool,
}

impl Render for IraDesktop {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let label = if self.button_was_clicked {
            "Button clicked"
        } else {
            "Hello from IRA"
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_center()
            .items_center()
            .gap_4()
            .bg(rgb(0x171923))
            .text_color(rgb(0xf4f4f5))
            .child(div().text_2xl().child("IRA Desktop"))
            .child(
                div()
                    .id("hello-button")
                    .px_4()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0x5b6ee1))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                        this.button_was_clicked = true;
                        cx.notify();
                    }))
                    .child("Click me"),
            )
            .child(div().child(label))
    }
}

fn open_main_window(cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(640.0), px(420.0)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |_window, cx| {
            cx.new(|_cx| IraDesktop {
                button_was_clicked: false,
            })
        },
    )
    .expect("failed to open IRA desktop window");

    cx.activate(true);
}

fn main() {
    let app = Application::new();
    app.on_reopen(open_main_window);
    app.run(|cx: &mut App| open_main_window(cx));
}
