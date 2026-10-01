# Advisory: platform risks by platform-advisor round 1

Scope reviewed: `src/app.rs`, `src/handler.rs`, `src/services/{drives,folders,list_files,state,bookmarks,transfer,clipboard}.rs`, `src/theme/mod.rs`, `src/main.rs`, `desktop/`, `.github/workflows/desktop-build.yml`, `docs/features.md`, `docs/distribution-plan.md`. Baseline currently tagged `tui-oracle-baseline` at `1cad4ce`. `migration/STATE.md`, inventory, and parity matrix do not yet exist; findings below are provisional until the explorer captures actual TUI behavior and the manager creates feature rows.

Summary verdict: **PROCEED WITH CHANGES**. The TUI has significant platform-specific behavior already, but the current GPUI crate is an independent button-only executable (`desktop/Cargo.toml`, `desktop/src/main.rs`) and does not depend on or call the TUI/core crate. The current desktop CI therefore proves only that this shell compiles on its two artifact targets.

## Ranked recommendations

### R1 [must] Establish a platform-neutral path contract before moving features

**Evidence:** `src/domain/data.rs` and `src/services/list_files.rs` represent paths as `String`; `FEntry::path` is built with `to_string_lossy()`, directory entries whose `file_name().to_str()` fails are skipped (`list_files.rs`). `src/services/state.rs`, `bookmarks.rs`, `thumbnails.rs`, and `theme/mod.rs` build files beneath `dirs_next` directories. `README.md` documents per-OS config paths; `docs/features.md` describes persisted paths, state, and cache.

**macOS hazards:** APFS/HFS name equivalence and case behavior can make visually equivalent NFC/NFD or case-only names collide; filesystem case-sensitivity is volume-specific. Do not normalize persisted names or assume case sensitivity/insensitivity. Apple's file-system guidance describes filesystem-dependent name behavior ([APFS FAQ](https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/APFS_Guide/FAQ/FAQ.html)).

**Windows hazards:** drive-relative paths, UNC shares, reserved names, separators, long-path opt-in, and case-preserving/case-insensitive defaults. Rust path values should stay `PathBuf`/`OsString` until display edges. Windows documents the legacy 260-character limit (subject to API/manifest/policy) and opaque UTF-16 path handling ([Maximum Path Length](https://learn.microsoft.com/en-us/windows/win32/fileio/maximum-file-path-limitation), [Naming Files](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file)).

**Tests:** round-trip Unicode and non-Unicode Unix names; NFC/NFD and case-only rename on a case-sensitive and case-insensitive macOS volume where available; Windows drive-root, drive-relative, UNC, reserved-name rejection, trailing dot/space, and long path behavior. Verify persisted bookmarks/session/theme paths byte-compatible with the TUI. The current `String` representation makes these unverified risks, not assumed failures.

### R2 [must] Define file access and permission behavior for packaged macOS apps

**Evidence:** `desktop/macos/Info.plist` packages as an app bundle. `.github/workflows/desktop-build.yml` ad-hoc signs it (`codesign --sign -`) but does not notarize; `desktop/README.md` documents Gatekeeper approval. The current bundle has no sandbox entitlements or file-open integration. The TUI directly browses arbitrary paths and persists plain paths, so sandboxing or document-picker-only access would alter parity.

**macOS hazards:** If sandboxing is introduced, arbitrary traversal and later reuse of bookmarked paths require user-selected access and security-scoped bookmarks; AppKit open panels grant scoped access to selected URLs, while stored resources need explicit scoped bookmarks ([Apple sandbox file access](https://developer.apple.com/documentation/security/accessing-files-from-the-macos-app-sandbox?language=objc)). Finder/Dock drag-open behavior and full-volume traversal need explicit treatment. Unsigned/non-notarized test binaries can trigger Gatekeeper; the current workflow is intentionally ad-hoc signed only.

**Windows hazards:** ACL denial, read-only attributes, locked/open files and sharing violations differ from POSIX permission semantics. Preserve actionable error kinds/messages and avoid presenting Unix mode bits as equivalent Windows permissions.

**Tests:** launch downloaded app on a clean macOS account; browse home, removable volume, external volume and protected folders; test deny/restore permissions and restart with persisted paths. On Windows test ACL denial, read-only files, and rename/delete of an open file. Apple confirms that sandbox access remains subject to POSIX ACL and mandatory controls (same Apple doc above).

### R3 [must] Treat transfers, rename, delete, and save as cross-volume / collision-sensitive operations

**Evidence:** `src/services/transfer.rs` has async jobs, collision policies, symlink logic, metadata preservation and `cfg(unix)` versus Windows branches. `src/app.rs` implements rename and editor save with temp-file + `std::fs::rename`; editor temp path currently uses `format!("{}.ira-tmp", fs_path.display())`. `docs/features.md` says recursive deletion is confirmed and backgrounded; the distribution plan notes Windows and macOS support.

**macOS hazards:** case-only rename on case-insensitive volumes; symlinks and aliases; volume boundaries (APFS volumes/external drives); ACLs and file flags; timestamp precision; same-directory atomic replace behavior. Verify path identity and symlink retargeting behavior across volumes.

**Windows hazards:** rename across volumes returns a different error and needs copy/delete parity; open-file sharing violations can block rename/delete; atomic replacement differs from Unix rename; symlink creation may require Developer Mode or privilege and junction/reparse-point semantics are distinct. `transfer.rs` contains a Windows symlink fallback, but its parity/evidence on a real Windows runner is not present in this scaffold.

**Tests:** conflict matrix (existing file/folder, case-only collision, overwrite/skip/auto-rename), same- and cross-volume move, cancel mid-file and mid-tree, read-only/locked files, symlink and dangling link behavior, metadata preservation and partial-copy cleanup on both OSes. Test temp-file replacement when destination exists and when antivirus/indexing has a handle open. Do not use only Linux unit tests as evidence for these contracts.

### R4 [must] Move default-app/open-with and terminal launch behind a platform service

**Evidence:** `src/app.rs::open_file` uses `gio open` on Linux then `open::that_detached`; `spawn_native_terminal` uses OS-specific candidate programs and argument vectors. `src/services/drives.rs` uses `lsblk`/`udisksctl` on Linux, `/Volumes` on macOS and drive probing plus `GetVolumeInformationW` on Windows. The desktop shell does not currently implement these actions.

**macOS hazards:** `open` can target GUI apps from a GUI process but launch-time working directory/environment differs from a terminal launch; Terminal/iTerm/WezTerm candidates may not be on `PATH` for GUI apps. The existing code has dedicated macOS `open -na` handling for terminal apps; retain tests around quoting and working directory.

**Windows hazards:** avoid shell command concatenation; Explorer `/select,` and Windows Terminal/cmd argument contracts require exact argv/cwd. UNC locations and paths containing spaces/Unicode need explicit argv tests. `windows_subsystem = "windows"` in `desktop/src/main.rs` intentionally removes a console, so CLI `stdout` contracts cannot silently be assumed for the desktop binary.

**Tests:** fake executable capture of argv and current directory, then manual app-open smoke on macOS/Windows with filenames containing spaces, Unicode and punctuation; test missing opener and denied target errors. Define GUI-launch behavior when no console exists.

### R5 [should] Preserve config and session locations, but verify actual `dirs-next` resolution and compatibility

**Evidence:** the TUI uses `dirs_next::config_dir()` for session/bookmarks/theme and `cache_dir()` for thumbnail cache (`src/services/state.rs`, `bookmarks.rs`, `theme/mod.rs`, `thumbnails.rs`). README says Linux `~/.config/ira`, macOS `~/Library/Application Support/ira`, Windows `%APPDATA%\ira`; feature doc calls out persisted state and bookmarks. Session path values are serialized as strings.

**macOS hazards:** do not relocate already persisted data merely because the desktop bundle has its own identifier; the TUI's Application Support path must remain visible. If sandboxed later, App Sandbox container paths differ.

**Windows hazards:** distinguish Roaming `%APPDATA%` config from `%LOCALAPPDATA%` cache; handle environment variables absent/malformed and roaming profile/network paths. Never assume `~/.config` on Windows.

**Tests:** assert concrete resolved config/cache directories under controlled HOME/APPDATA/LOCALAPPDATA and compatibility-load TUI-generated files. Exact environment-to-directory behavior is not verified in this review.

### R6 [should] Clarify hidden-file, symlink and metadata semantics by filesystem

**Evidence:** `list_files.rs` uses `label.starts_with('.')` as hidden-file behavior and `metadata` (follows symlinks) for directory classification. `docs/features.md` describes dotfile visibility. `transfer.rs` uses `symlink_metadata` and platform-conditioned symlink creation. Windows hidden/system attributes are not represented by a leading dot.

**macOS hazards:** Finder-hidden flag and package/bundle presentation do not equal dotfile names; aliases are not POSIX symlinks. Preserve current TUI dotfile rule for parity unless a documented additive visibility control is separately introduced.

**Windows hazards:** Hidden/system attributes, junctions and reparse points need coverage. Do not silently change dotfile filtering to include Windows hidden attributes during port; treat attribute display as additive or decision-backed equivalence.

**Tests:** dotfile and hidden-attribute entries, symlink-to-file/dir, dangling symlink, junction and reparse point behavior, recursive size/traversal loop handling.

### R7 [should] Replace terminal clipboard behavior with GPUI clipboard at the adapter edge

**Evidence:** `src/services/clipboard.rs::copy_text` shells out to `wl-copy`, `xclip`, `pbcopy`, Windows `clip`, then writes OSC 52 to `/dev/stderr`. This is correct for its TUI host but is not a desktop clipboard contract. `src/app.rs::copy_folder_path` calls it.

**macOS hazards:** GUI app has a native pasteboard; terminal OSC 52 can mutate the terminal's clipboard rather than the app clipboard and is unavailable without a terminal. Windows GUI `clip` generally expects a console process; no-console builds may fail or have no console stdin.

**Tests:** clipboard set/read via GPUI/platform API, Unicode and multiline strings, unavailable clipboard errors, and no terminal attachment. Preserve TUI OSC-52 path only in TUI adapter.

Risks not yet reviewable: drag/drop, native trash vs permanent delete, context menus, open-with chooser, single-instance behavior, recent files, file association, shell-integration flags and CLI output. No discovery inventory exists yet, so do not interpret this list as complete. `docs/features.md` and `README.md` are useful feature references, not behavioral captures.
