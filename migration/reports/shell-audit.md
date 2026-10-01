# GPUI shell audit

Scope: Phase 0 audit of the existing desktop shell, its manifests, build workflows, and available tests. This report is advisory; it does not claim functional parity.

## Verdict: PROCEED WITH CHANGES

The GPUI target is a real native-window application and useful launch skeleton, but it is not yet a file manager shell. The TUI remains a separate root Cargo package. No shared behavior, tests, or parity instrumentation currently connects the two.

## Evidence and observations

- `desktop/Cargo.toml` defines the standalone `ira-desktop` package (edition 2024), with target-specific `gpui = 0.2.2` registry dependencies. The independent `desktop/Cargo.lock` pins `gpui` 0.2.2; the root `Cargo.toml` is the separate IRA Ratatui/Crossterm TUI package.
- `desktop/src/main.rs` is the only desktop source file. `IraDesktop` implements `Render`, draws a fixed “IRA Desktop” label, a “Click me” element and a message that changes after click. State is a single boolean held by the view. It uses `Application::new().run`, `App::open_window`, `Context::new`, `Context::listener`, `cx.notify()`, `Bounds::centered`, and `cx.activate(true)`.
- The app registers `Application::on_reopen(open_main_window)` for macOS Dock reopens. The reopen callback currently opens a new instance of the same starter view.
- There is no app/core crate split, filesystem service, directory model, action registry, key binding, focus model, text input, theme abstraction, modal host, async job handling, persisted state, or feature module structure. The starter hard-codes colors and has no keyboard or accessibility semantics.
- `desktop/README.md` documents `cargo run --manifest-path desktop/Cargo.toml`. Release builds can use `cargo build --manifest-path desktop/Cargo.toml --release --locked`.
- `.github/workflows/desktop-build.yml` builds and uploads an arm64 macOS app bundle and x86_64 Windows GUI executable. It triggers on manual dispatch and changes in `desktop/**` or that workflow. It does not run fmt, clippy, unit/integration tests, or root TUI tests.
- `.github/workflows/release.yml` is a separate tag-triggered release pipeline for the TUI. It builds root `ira` across platforms, then creates/releases distribution packages. This is not desktop CI.
- Existing root tests are `tests/drives_refresh_test.rs`, `tests/sort_test.rs`, and `tests/operations_test.rs`. They exercise the TUI package; there are no desktop tests, no `gpui` `test-support` feature in desktop dependencies, and no workspace linking desktop and TUI.
- The GPUI source ships examples under `~/.cargo/registry/src/index.crates.io-*/gpui-0.2.2/examples/`, including `hello_world.rs`, `uniform_list.rs`, `input.rs`, `on_window_close_quit.rs`, and `window.rs`.

## Ranked recommendations

1. **R1 [must] Keep the TUI package and its behavior intact as the oracle.** First create a tagged baseline and characterization evidence before extracting any shared logic. Evidence: root `Cargo.toml` is still a separate TUI app and existing tests are exclusively for it. Ignoring this risks changing the only trustworthy behavioral reference while porting.
2. **R2 [must] Establish a UI-independent core before implementing parity features.** Use a core crate/module for state, commands, errors, filesystem operations, and persisted formats; keep GPUI as an adapter/view layer. Preserve `Path`/`OsString` and verify disk formats byte-for-byte. Evidence: current desktop has only presentation state; root TUI depends on Ratatui/Crossterm and cannot be imported as reusable UI-independent behavior.
3. **R3 [must] Add desktop test support and a build/lint/test CI job before feature work.** Pin `gpui = 0.2.2` and enable its `test-support` feature for tests as appropriate; use `#[gpui::test]`, `TestAppContext`, `VisualTestContext`, simulated keystrokes, and `uniform_list` tests from this version's source. Keep behavior-heavy tests in a pure core. Evidence: gpui 0.2.2 `Cargo.toml` defines `test-support`; `src/gpui.rs` exports `test`, and `src/app/test_context.rs` defines test contexts, but the desktop manifest and workflow currently have none.
4. **R4 [should] Build shared interaction foundations before feature views.** An explicit core mode/state model, stable action names, keymap registry, focus strategy, virtualized file list, modal/text-input host, theme tokens, and effect/job boundary are high-fan-in dependencies. Validate TUI key semantics against discovery before selecting bindings. Ignoring this creates collision-prone feature-local behavior and makes parallel integration expensive.
5. **R5 [should] Treat background filesystem/process work as jobs.** Do not perform unbounded filesystem/process work in a GPUI event callback; use a cancellable background runner and deliver results to UI-owned state. This will matter as soon as directory enumeration and file operations arrive; the current shell has no such boundary.
6. **R6 [could] Defer external GPUI component crates until compatibility and licensing are checked against the exact lockfile.** Start with minimal in-house components and GPUI primitives. `gpui-component` has evolved/renamed to `gpui-kit`; the current upstream is a large separate workspace and release compatibility with pinned 0.2.2 is unproven. The base GPUI crate declares Apache-2.0, but that does not establish the license of all Zed UI crates or transitive dependencies. Reassess a component crate when requirements exist, inspecting its exact release Cargo metadata, license files, dependency version, and maintenance before adoption.

Risks not covered: visual behavior has not been tested on macOS/Windows during this code-only audit; CI build success proves compilation/package staging, not interaction semantics or runtime parity. Context7 was not exposed as a callable tool in this environment; exact GPUI APIs below were checked against the pinned local crate source instead.
