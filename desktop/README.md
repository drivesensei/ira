# IRA Desktop (GPUI starter)

This is a minimal desktop UI experiment alongside IRA's existing terminal app.
It uses [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui),
Zed's open-source, GPU-accelerated Rust UI framework. GPUI is pre-1.0 and its
APIs may change between releases.

From the repository root, run:

```sh
cargo run --manifest-path desktop/Cargo.toml
```

The starter opens a window with a button. Clicking it updates the message.
On Linux, the GPUI crate needs a working Wayland or X11 desktop session and the
system development libraries required by its Wayland/X11 and font-kit backends.

For downloadable production binaries, run **Desktop test binaries** from the
repository's GitHub Actions tab. It uploads separate macOS arm64 and Windows
x86_64 executable artifacts for 14 days; it does not create a GitHub release.
The macOS artifact ZIP contains a `.tar.gz` with the `IRA.app` bundle. Extract
both archives, then open the app in Finder. Since this test build is not signed
or notarized, macOS may ask you to confirm the first launch: Control-click the
app, choose **Open**, then choose **Open** again. The Windows artifact ZIP
contains a GUI-subsystem `.exe`; extract it and open the executable directly.

Framework sources and examples:

- [GPUI source in the Zed repository](https://github.com/zed-industries/zed/tree/main/crates/gpui)
- [GPUI examples](https://github.com/zed-industries/zed/tree/main/crates/gpui/examples)
- [Zed's GPUI app starter](https://github.com/zed-industries/create-gpui-app)
