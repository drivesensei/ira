# Distribution Plan — GitHub Releases, Homebrew, winget, Linux packages

Planning for distributing IRA (Rust/ratatui TUI file manager) to Windows, macOS, and Linux testers.

## Current state

- Single `Cargo.toml` crate, `ira` 0.1.0. Cross-platform code already in place:
  - `src/services/drives.rs` — macOS `/Volumes` enumeration, Windows drive letters (`winapi`), Linux `lsblk` + `udisksctl`.
  - `src/services/windows_drives_labels.rs` — `GetVolumeInformationW` (windows-only).
  - `src/app.rs` `open_file` — linux-only `gio` path.
- Linux extra deps are only for the drive panel: `lsblk` (util-linux), `udisks2`. Absent elsewhere → degrade gracefully; packages should list them as optional/recommended deps.
- `.github/workflows/release.yml` is broken and will be replaced, not patched:
  - `actions-rs/toolchain@v1` — archived.
  - `actions/upload-artifact@v3` — retired; workflow fails today.
  - `create_release` globs `*.tar.gz` but the runner never downloaded artifacts (missing `download-artifact` step).

## Analysis (2026-09-03): automated per-OS workflow, hand-rolled

**Is automating per-OS release worth it?** Yes, unconditionally: Homebrew formulas and winget
manifests are built from versioned URLs + SHA256 checksums, which only exist if builds are
reproducible per tag. Manual builds on 3 OSes per release do not scale to tester iteration.

**cargo-dist vs hand-rolled:** the original plan favored [cargo-dist](https://opensource.axo.dev/cargo-dist/),
but its model covers GitHub release + Homebrew tap + install scripts only. The goal spans
brew + winget + Ubuntu deb + Arch AUR — 3 of 4 channels outside cargo-dist's scope, and extending
its generated YAML is harder than writing the pipeline directly. The crate is pure Rust
(ratatui/crossterm, no C dependencies), so a hand-rolled workflow stays boring:

- musl targets self-link via rust-lld — no cross toolchain needed for aarch64 Linux.
- macOS universal binary is two rustup targets + `lipo`.
- deb is `dpkg-deb` on an already-built binary.

Decision: single tag-triggered workflow (`.github/workflows/release.yml`), one matrix job per OS
target, with universal/deb/checksum/release/tap/winget/AUR/crates jobs in the same file.

## Release targets

| Target | Runner | Notes |
|---|---|---|
| `x86_64-unknown-linux-musl` | `ubuntu-22.04` | static binary, no glibc floor — runs on any distro |
| `aarch64-unknown-linux-musl` | `ubuntu-22.04` | static, rust-lld links; RPi/ARM servers |
| `x86_64-apple-darwin` + `aarch64-apple-darwin` | `macos-14` | combined into universal via `lipo` |
| `x86_64-pc-windows-msvc` | `windows-latest` | `.zip` with `ira.exe` |

## Release source

Releases are cut from whatever commit is tagged, not automatically from master. Convention:

- Only tag releases on `master` (`git describe --tags` must match the released artifact; the workflow builds exactly the tagged commit and derives the version from the tag).
- Flow: merge feature branches → `master` → bump version → commit → tag `v*.*.*` → push.
- Prereleases: tag `v*.*.*-rc.N` on a feature branch (e.g. `update-dependencies`) to let testers try it before merging; the workflow marks tags containing `-` as prereleases and they never become the "latest" release.

## Distribution channels (rollout order)

1. **GitHub Releases** (works with zero secrets) — tag `v*.*.*` → per-OS tarballs, `.zip`, `amd64`/`arm64` `.deb`, `SHA256SUMS`, auto-generated notes. No accounts, no review.
2. **Homebrew tap** — repo `drivesensei/homebrew-ira`; users: `brew install drivesensei/ira/ira`. Workflow pushes the updated formula + sha256 on every tag (needs `HOMEBREW_TAP_TOKEN` PAT secret). homebrew-core requires notable popularity; a tap is instant with identical UX.
3. **crates.io** — `cargo install ira`. Prereq done: `Cargo.toml` has `description`/`repository`. Needs `CARGO_REGISTRY_TOKEN` secret; skipped for rc tags.
4. **winget** — first submission is manual: fork `microsoft/winget-pkgs`, run `wingetcreate new <zip-url>`, open a PR with ID `drivesensei.IRA`. After acceptance, the workflow updates the manifest per release (needs `WINGET_PAT` secret). Portable package type (zip + exe) — no MSI.
5. **AUR** — package `ira-bin` (release binary). The workflow creates/updates it via `AUR_SSH_KEY` secret (SSH key registered on an AUR account; first push creates the package).

## Known risks (deferred)

- **macOS notarization** — unsigned browser-downloaded binaries hit Gatekeeper; `curl`/`brew` installs don't get the quarantine attribute. Defer Apple Developer ID ($99/yr) + notarytool until users complain; document `xattr -d com.apple.quarantine` workaround in release notes.
- **Windows SmartScreen** — unsigned `.exe` shows "unknown publisher". Cosmetic for a CLI; Authenticode cert deferred.
- **Version discipline** — the workflow derives the version from the tag; keep `Cargo.toml` `version` in sync (a mismatch confuses crates.io publish). Flow: bump version → commit → tag `v*.*.*` → push.

## Phases

1. **Phase 1 — pipeline**: DONE — `Cargo.toml` metadata, `--version` flag, `.github/workflows/release.yml` rewritten. Remaining: tag `v0.1.0` on `master`; verify all assets + checksums land on the release.
2. **Phase 2 — Homebrew tap**: create repo `drivesensei/homebrew-ira`, add `HOMEBREW_TAP_TOKEN` PAT secret; formula then auto-published per tag.
3. **Phase 3 — crates.io**: add `CARGO_REGISTRY_TOKEN` secret; publish job runs per tag.
4. **Phase 4 — winget + AUR**: first winget submission manual (`wingetcreate new`, PR with ID `drivesensei.IRA`); create AUR account, register SSH key, add `AUR_SSH_KEY` secret; workflow handles all later updates.
5. **Phase 5 (deferred)**: Apple notarization, Windows Authenticode.

Verification per phase: fresh-VM smoke tests in CI — `brew install` on a macOS runner, `winget install` on a Windows runner, tarball extract + `ira --version` on Ubuntu — confirm the TUI launches and the drive panel degrades cleanly where udisks is absent.

## Account/secret prerequisites (owner-only)

- GitHub PAT (repo scope on the tap repo) → repo secret `HOMEBREW_TAP_TOKEN`.
- Tap repo `drivesensei/homebrew-ira` (can be empty; the workflow creates `Formula/ira.rb`).
- crates.io token → repo secret `CARGO_REGISTRY_TOKEN`.
- winget: fork of `microsoft/winget-pkgs` + PAT (fine-grained, allow PR to that fork) → repo secret `WINGET_PAT`; first submission manual.
- AUR account + SSH key (Package maintainer privileges) → repo secret `AUR_SSH_KEY`.
- Later: Apple Developer ID, Windows code-signing cert.

