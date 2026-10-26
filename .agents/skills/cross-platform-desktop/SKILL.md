---
name: cross-platform-desktop
description: macOS and Windows specifics for a Rust GPUI file manager that was previously a unix-centric TUI. Paths, case sensitivity, permissions, key modifiers, trash, opening files, file locking, packaging, CI matrix. Use for platform-advisor, developers touching platform code, and manager CI setup.
---

# Cross-platform desktop checklist (macOS + Windows)

Assume the TUI has unix assumptions until the oracle proves otherwise. Windows is the likeliest place for
divergence; macOS differs in case-insensitivity, key conventions, and sandbox/permission prompts.

## 1. Paths and names

- Use `Path`, `PathBuf`, `OsStr(ing)`. Convert to `String` only for display via an explicit helper that documents
  lossy behavior. Never store paths as `String` in core or persisted formats unless the TUI did (then keep format).
- Windows: drive letters and "current directory per drive", UNC paths, `\\?\` long paths (use `dunce` or
  explicit handling), reserved device names, trailing dot/space names, forbidden characters `<>:"/\|?*`, alternate
  data streams, 260-char limit when long paths are not enabled, case-insensitive but case-preserving.
- macOS: APFS usually case-insensitive and Unicode-normalization-insensitive (NFD on disk historically). Case-only
  rename needs a two-step rename. Names may appear NFD while users type NFC.
- Separators: store and compare with `Path` components; show with the OS separator unless the TUI always showed `/`
  (decide per oracle, record in the spec).
- Home dir and special folders: use `dirs`/`directories` crates or whatever the TUI used; mirror `~` expansion rules.
- Hidden files: Unix dotfiles; Windows hidden attribute and system attribute; macOS `UF_HIDDEN` flag and
  `.hidden`-style conventions. Match the TUI's rule, then extend sensibly and document.

## 2. Metadata and permissions

- Unix permission bits do not exist on Windows. If the TUI shows or edits modes (chmod/chown), define the Windows
  mapping (read-only attribute, ACL summary) or mark `EQUIVALENT_VERIFIED` with decision record.
- Symlinks on Windows need privileges or Developer Mode; junctions and reparse points differ. Detect and handle.
- Executable detection: unix mode bit vs Windows extension list (`PATHEXT`).
- Timestamps: creation/birth time availability differs; precision differs (NTFS 100ns, HFS+ 1s, APFS ns).
- Disk space and mount enumeration: use platform APIs through a crate already vetted by architecture-advisor.

## 3. Operations

- Move across volumes: rename fails (`EXDEV` on unix, different error on Windows): copy + delete with the same
  conflict and error behavior as the TUI.
- Delete/trash: macOS Finder trash (`NSFileManager trashItem`) and Windows Recycle Bin; use a maintained crate
  (for example `trash`) and mirror TUI's rules about trash vs permanent delete. Put-back metadata is OS-managed;
  if the TUI had its own trash directory, preserve its format and behavior.
- File locking: Windows denies deletion/rename of open files (sharing violations). Retry policy and messages must be
  explicit and tested. macOS does not lock the same way.
- Atomic writes for config/state: write temp + rename (Windows needs replace semantics via `ReplaceFileW` or
  `persist` from `tempfile`); preserve TUI's behavior on crash.
- Open with default app: `open` (macOS), `ShellExecute` (Windows) through a crate such as `opener`/`open`. Template
  commands from TUI config (for example `xdg-open`) need platform translation rules in the spec.
- External editor/shell spawn: quoting differs (cmd.exe vs PowerShell vs sh). Never build shell strings from file names;
  pass arguments as arrays. Document the `EDITOR` / `VISUAL` handling the TUI used and the GUI equivalent.
- Clipboard: GPUI clipboard for text; file-list clipboard (copy files as OS objects) is an enrichment unless the TUI
  integrated the OS clipboard.

## 4. Input

- macOS: `cmd` is the primary shortcut modifier; `ctrl` TUI bindings stay valid but some collide with system
  behavior (for example `ctrl-space`, `ctrl-arrows` for Spaces, `cmd-q`, `cmd-h`, `cmd-w`, `cmd-m`). Provide aliases
  and a documented conflict table. Option key composes characters; ensure `alt` bindings work with
  option-as-meta decisions.
- Windows: `alt` activates menus; `ctrl-alt` equals AltGr on many layouts (conflicts with typing); `win` key is
  reserved; `alt-f4` closes. Menu mnemonics.
- Keyboard layouts: bindings should be defined by key, not assumed US layout. Test with a non-US layout for symbol keys
  like `?`, `/`, `:`.
- IME: ensure text inputs support composition; key bindings must not steal keystrokes during composition.
- Key repeat rates differ; debounce logic must not depend on terminal timings.

## 5. Windowing and packaging

- macOS: app bundle, menu bar (app menu, Edit, Window), full-screen, window restoration, notarization is out of
  scope unless the repo already does it. Info.plist document types if "open with" matters.
- Windows: window chrome, DPI awareness (GPUI handles), taskbar, `windows_subsystem = "windows"` for release to avoid
  a console window, but keep console attach for CLI flag output (parity with `--help`, `--version`, stdout contracts
  such as chooser-file or print-cwd-on-exit). Test CLI behavior from both console and double-click.
- CLI parity: if the TUI is launched from a shell with arguments and prints on exit, the desktop binary must honor the
  same flags and contracts when launched from a terminal. Define behavior for launching from GUI without a terminal.
- Single-instance handling and second-launch forwarding if the TUI had multiple-instance rules.
- Config locations: respect the TUI's per-OS locations. Windows `%APPDATA%` vs `%LOCALAPPDATA%`, macOS
  `~/Library/Application Support` vs `~/.config` if the TUI used XDG on mac. Do not move user data.

## 6. CI and test infrastructure

- Required matrix: `macos-latest` (and an Intel runner if the repo ships x86_64 mac), `windows-latest`,
  plus `ubuntu-latest` if the repo targets it. Jobs: build, `cargo test --workspace`, clippy `-D warnings`, fmt check.
- GPUI on CI: tests that need a display/GPU must be headless-capable via gpui test support, or tagged
  `#[ignore = "env: needs display"]` and run in a dedicated job with a virtual display where possible.
  The gate counts `env:` ignores as acceptable only if a CI job actually runs them.
- Windows toolchain: MSVC target; ensure long path support and git `core.autocrlf` does not corrupt fixtures
  (use `.gitattributes` with `-text` for binary fixtures and explicit eol for text goldens).
- Cache cargo registry and `target/`. Keep runtime of CI reasonable; split jobs instead of dropping tests.
- Read CI results yourself with the forge CLI available in the environment (for example `gh run list/view` or the
  authenticated equivalent). Record run ids in the matrix Evidence column.

## 7. platform-advisor deliverables

- `reports/advisory/platform-risks.md`: per feature, the macOS and Windows hazards, with test suggestions.
- `reports/advisory/platform-keymap-conflicts.md`.
- `reports/advisory/platform-ci.md`: runner matrix, flaky points, fixture handling.
- Reviews of any feature touching fs semantics, process spawn, clipboard, config locations, or key handling.
