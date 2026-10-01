# GPUI notes for IRA desktop migration

## Pin and source of truth

- The authoritative desktop dependency is crates.io `gpui 0.2.2`, pinned in `desktop/Cargo.lock` (`name = "gpui"`, version `0.2.2`). The manifest uses `version = "0.2.2"`, `default-features = false`, and selects backend features by target: macOS `font-kit`; Linux `font-kit`, `wayland`, `x11`; Windows no explicit feature. No git override or component library is present.
- Local source inspected: `~/.cargo/registry/src/index.crates.io-*/gpui-0.2.2/src/app.rs`, `src/app/context.rs`, `src/elements/uniform_list.rs`, `src/app/test_context.rs`, `src/gpui.rs`, and `examples/`.
- Context7 was requested by the project procedure but no Context7 tool/connector is available in this agent session. API claims below come from the exact locally locked GPUI source, not current `main` docs. Recheck the source whenever the lockfile changes.

## Verified signatures and examples

The APIs actually exercised by `desktop/src/main.rs`:

```rust
Application::new() -> Application
Application::on_reopen<F>(&self, callback: F) -> &Self
where F: 'static + FnMut(&mut App)
Application::run<F>(self, on_finish_launching: F)
where F: 'static + FnOnce(&mut App)
App::open_window<V: 'static + Render>(
    &mut self,
    options: WindowOptions,
    build_root_view: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> anyhow::Result<WindowHandle<V>>
Context::listener<E: ?Sized>(
    &self,
    f: impl Fn(&mut T, &E, &mut Window, &mut Context<T>) + 'static,
) -> impl Fn(&E, &mut Window, &mut App) + 'static
```

These signatures are in GPUI 0.2.2 `src/app.rs` and `src/app/context.rs` (listener around line 252; application APIs around lines 132, 174, 198, and `open_window` around 943). `Context::new` is provided through GPUI's app context API; existing desktop source confirms it compiles at `cx.new(|_| IraDesktop { ... })`. The click handler mutates the view and calls `cx.notify()` through the view context. `App::activate(&self, ignoring_other_apps: bool)` is defined at `app.rs` around 979.

Local examples worth copying only after checking the locked source:

- `examples/hello_world.rs`: minimal application and window startup.
- `examples/uniform_list.rs` and `src/elements/uniform_list.rs`: `uniform_list(id, item_count, render_range)` lazily builds visible rows; appropriate candidate for large file lists when row heights are uniform.
- `examples/input.rs`: GPUI low-level text input, focus and key binding handling; substantially more than a trivial `TextInput` widget.
- `examples/on_window_close_quit.rs`: window-close and application lifecycle patterns.
- `examples/window.rs`: action declaration and binding patterns.

The implementation guide's general action/focus/state ideas are conceptual; GPUI API spellings can differ across releases. This 0.2.2 source exposes `actions!`, `KeyBinding::new`, `App::bind_keys`, `FocusHandle`, `uniform_list`, `Application::on_reopen`, `App::spawn`, and the test context APIs. Verify specific methods/signatures before implementing against them.

## Test support

- GPUI 0.2.2 declares a `test-support` feature in its `Cargo.toml`; its feature enables test helpers including test dispatcher support and additional support dependencies.
- `src/gpui.rs` reexports `gpui_macros::test`. `src/app/test_context.rs` contains `TestAppContext`, `VisualTestContext`, `add_window`, and `simulate_keystrokes` APIs. The source explicitly documents `#[gpui::test]` and the test context. This makes headless interaction tests available with the GPUI test-support feature.
- The current `desktop/Cargo.toml` has no dev-dependencies/features for test support and no `desktop/tests` or unit test files. Add test-support only in the test dependency configuration that Cargo will unify with the target-specific runtime dependency, then validate on the intended CI targets; do not enable it on shipped release binaries without reason.
- Keep most parity rules in the pure core crate, tested independently of GPUI. Use GPUI tests for UI wiring, focus, action dispatch, modal routing, and list rendering. The GPUI test contexts simulate keystrokes; do not assume they alone prove native macOS/Windows window manager behavior.

## Component library options

- **GPUI primitives / small IRA-owned components (recommended starting point):** no extra GPUI compatibility layer and APIs are exactly those in the lockfile. This minimizes API and dependency risk while the required components are not yet understood. Keep theme tokens and components reusable in the desktop layer.
- **`longbridge/gpui-component`, now the broader `longbridge/gpui-kit` project:** upstream advertises Apache-2.0 and recent 0.7.x releases/activity (checked 2026-10-01). The repo now contains multiple packages (`gpui-kit`, `gpui-base`, `gpui-component`, etc.) and release notes describe a layered toolkit. Current upstream release metadata/source mentions exact `gpui-pre` snapshots, so its currently advertised APIs are not evidence of compatibility with this app's crates.io `gpui 0.2.2`. It is actively maintained, but adopting it would need an explicit compatibility decision, inspection of exact published package/license/dependency closure, and Windows/macOS build validation. Do not pin an arbitrary current version into this starter.
- **Zed's own `ui`/`editor` crates:** source lives inside the Zed monorepo and has its own internal dependencies and crate-specific licensing; it is not a ready, pinned drop-in component package for this app. Do not infer that GPUI's Apache-2.0 license covers those crates. Import only after dependency closure, license, and API compatibility review.

## Build, runtime, and CI support

- Local desktop debug run: `cargo run --manifest-path desktop/Cargo.toml`.
- Local desktop release build: `cargo build --manifest-path desktop/Cargo.toml --release --locked`.
- `desktop/Cargo.lock` is separate from root `Cargo.lock`; updates and reproducibility need to be maintained separately.
- `.github/workflows/desktop-build.yml` builds/package uploads the desktop app for macOS arm64 and Windows x86_64. It is artifact/build CI only; it does not run formatting, clippy, tests, startup checks, or UI interaction checks.
- `.github/workflows/release.yml` covers the original TUI release workflow and is tag-triggered. It should not be mistaken for GPUI desktop CI.

## Architecture direction

Maintain an explicit boundary between a platform-neutral behavioral core and the GPUI executable. The GPUI view should translate input into stable actions, render core state, and execute returned effects. Keep blocking filesystem/process tasks off the UI thread; make job progress and cancellation explicit. Model paths as `Path`/`OsString` in the core. Use a virtualized list for large directory views. Centralize theme tokens and input/focus ownership rather than letting individual views invent them. Existing shell idioms to preserve include `Application::on_reopen`, window creation via `App::open_window`, view-local state via `Entity`/`Context`, event handlers via `cx.listener`, and repainting via `cx.notify()`.
